use crate::config::Config;
use anyhow::{bail, Context, Result};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

// Dockerfile template embedded in the binary. The {binary} placeholder is
// replaced at build time with the consumer's binary name.
const DOCKERFILE_TEMPLATE: &str = "\
FROM --platform=linux/arm64 rust:1.83-slim-bookworm AS builder
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY . .
RUN cargo build --release --bin {binary}

FROM --platform=linux/arm64 debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/{binary} /usr/local/bin/agent
EXPOSE 8080
CMD [\"agent\"]
";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Run a command, capture stdout, fail on non-zero exit.
async fn cmd_output(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .with_context(|| format!("{program} not found. Is it installed and on PATH?"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{program} failed:\n{}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run a command, stream stdout/stderr to the terminal.
async fn cmd_exec(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .await
        .with_context(|| format!("{program} not found. Is it installed and on PATH?"))?;
    if !status.success() {
        bail!("{program} exited with {status}");
    }
    Ok(())
}

/// Run a command, return whether it succeeded (no output).
async fn cmd_ok(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

fn dockerfile_for(binary: &str) -> String {
    DOCKERFILE_TEMPLATE.replace("{binary}", binary)
}

/// Build a Docker image by piping the generated Dockerfile via stdin,
/// using the project root as the build context.
async fn docker_build(cfg: &Config, tag: &str, platform: Option<&str>) -> Result<()> {
    let dockerfile = dockerfile_for(&cfg.binary);
    let context = cfg.project_root.to_str().unwrap();

    let mut args = vec!["buildx", "build"];
    if let Some(p) = platform {
        args.extend_from_slice(&["--platform", p]);
    }
    args.extend_from_slice(&["-f", "-", "-t", tag, "--load", context]);

    let mut child = Command::new("docker")
        .args(&args)
        .stdin(Stdio::piped())
        .spawn()
        .context("docker not found. Install Docker: https://docs.docker.com/get-docker/")?;

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(dockerfile.as_bytes())
        .await?;

    let status = child.wait().await?;
    if !status.success() {
        bail!("Docker build failed");
    }
    Ok(())
}

fn ecr_uri(account_id: &str, region: &str, name: &str) -> String {
    format!("{account_id}.dkr.ecr.{region}.amazonaws.com/{name}")
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Full deploy pipeline: ECR repo -> Docker build -> push -> AgentCore runtime.
pub async fn deploy(cfg: &Config) -> Result<()> {
    println!(
        "Deploying {} to AgentCore ({})\n",
        cfg.agent_name, cfg.region
    );

    // Resolve AWS account
    let account_id = cmd_output(
        "aws",
        &[
            "sts",
            "get-caller-identity",
            "--query",
            "Account",
            "--output",
            "text",
        ],
    )
    .await
    .context("Failed to get AWS account ID. Run `aws configure` to set up credentials.")?;

    let repo = ecr_uri(&account_id, &cfg.region, &cfg.agent_name);
    let tag = format!("{repo}:latest");

    // 1. ECR repository
    print!("  Creating ECR repository... ");
    if !cmd_ok(
        "aws",
        &[
            "ecr",
            "describe-repositories",
            "--repository-names",
            &cfg.agent_name,
            "--region",
            &cfg.region,
        ],
    )
    .await
    {
        cmd_output(
            "aws",
            &[
                "ecr",
                "create-repository",
                "--repository-name",
                &cfg.agent_name,
                "--region",
                &cfg.region,
                "--image-scanning-configuration",
                "scanOnPush=true",
            ],
        )
        .await
        .context("Failed to create ECR repository")?;
        println!("created");
    } else {
        println!("exists");
    }

    // 2. Build ARM64 image
    println!("  Building ARM64 image...");
    docker_build(cfg, &tag, Some("linux/arm64")).await?;
    println!("  Image: {tag}");

    // 3. Push to ECR
    print!("  Pushing to ECR... ");
    let registry = format!("{account_id}.dkr.ecr.{}.amazonaws.com", cfg.region);
    let password = cmd_output(
        "aws",
        &["ecr", "get-login-password", "--region", &cfg.region],
    )
    .await?;

    let mut login = Command::new("docker")
        .args(["login", "--username", "AWS", "--password-stdin", &registry])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    login
        .stdin
        .as_mut()
        .unwrap()
        .write_all(password.as_bytes())
        .await?;
    if !login.wait().await?.success() {
        bail!("ECR login failed");
    }

    cmd_exec("docker", &["push", &tag]).await?;
    println!("  Pushed: {tag}");

    // 4. Create or update AgentCore runtime
    let artifact = format!(r#"{{"containerConfiguration":{{"containerUri":"{tag}"}}}}"#);

    let existing = cmd_output(
        "aws",
        &[
            "bedrock-agentcore-control",
            "list-agent-runtimes",
            "--region",
            &cfg.region,
            "--query",
            &format!(
                "agentRuntimeSummaries[?agentRuntimeName=='{}'].agentRuntimeArn | [0]",
                cfg.agent_name
            ),
            "--output",
            "text",
        ],
    )
    .await
    .unwrap_or_default();

    if !existing.is_empty() && existing != "None" {
        print!("  Updating AgentCore runtime... ");
        cmd_output(
            "aws",
            &[
                "bedrock-agentcore-control",
                "update-agent-runtime",
                "--agent-runtime-id",
                &existing,
                "--agent-runtime-artifact",
                &artifact,
                "--region",
                &cfg.region,
            ],
        )
        .await
        .context("Failed to update AgentCore runtime")?;
        println!("done");
        println!("  Runtime: {existing}");
    } else {
        let role_arn = ensure_role(cfg).await?;
        print!("  Creating AgentCore runtime... ");
        let result = cmd_output(
            "aws",
            &[
                "bedrock-agentcore-control",
                "create-agent-runtime",
                "--agent-runtime-name",
                &cfg.agent_name,
                "--agent-runtime-artifact",
                &artifact,
                "--network-configuration",
                r#"{"networkMode":"PUBLIC"}"#,
                "--role-arn",
                &role_arn,
                "--region",
                &cfg.region,
                "--output",
                "json",
            ],
        )
        .await
        .context("Failed to create AgentCore runtime")?;
        println!("done");
        // Try to extract ARN from JSON response
        if let Some(arn_start) = result.find("\"agentRuntimeArn\"") {
            if let Some(val) = result[arn_start..].split('"').nth(3) {
                println!("  Runtime: {val}");
            }
        }
    }

    println!("\nDeployment complete: {}", cfg.agent_name);
    Ok(())
}

/// Build and run the container locally.
pub async fn local(cfg: &Config, port: u16) -> Result<()> {
    let tag = format!("{}:local", cfg.agent_name);

    println!("Building image for local testing...");
    docker_build(cfg, &tag, None).await?;

    println!();
    println!("  Health check: curl http://localhost:{port}/ping");
    println!("  Invoke:       curl -N -X POST http://localhost:{port}/invocations \\");
    println!("                  -H 'Content-Type: application/json' \\");
    println!(r#"                  -d '{{"prompt": "Hello"}}'"#);
    println!();

    let port_mapping = format!("{port}:8080");
    let region_env = format!("AWS_REGION={}", cfg.region);
    cmd_exec(
        "docker",
        &["run", "--rm", "-p", &port_mapping, "-e", &region_env, &tag],
    )
    .await
}

/// Print current deployment status.
pub async fn status(cfg: &Config) -> Result<()> {
    println!("Agent:  {}", cfg.agent_name);
    println!("Region: {}\n", cfg.region);

    let result = cmd_output(
        "aws",
        &[
            "bedrock-agentcore-control",
            "list-agent-runtimes",
            "--region",
            &cfg.region,
            "--query",
            &format!(
                "agentRuntimeSummaries[?agentRuntimeName=='{}'] | [0]",
                cfg.agent_name
            ),
            "--output",
            "table",
        ],
    )
    .await;

    match result {
        Ok(ref table) if !table.is_empty() && table != "None" => println!("{table}"),
        _ => println!("No AgentCore runtime found."),
    }
    Ok(())
}

/// Delete AgentCore runtime, ECR repository, and IAM role.
pub async fn destroy(cfg: &Config) -> Result<()> {
    println!("Destroying {} ({})...\n", cfg.agent_name, cfg.region);

    // AgentCore runtime
    let existing = cmd_output(
        "aws",
        &[
            "bedrock-agentcore-control",
            "list-agent-runtimes",
            "--region",
            &cfg.region,
            "--query",
            &format!(
                "agentRuntimeSummaries[?agentRuntimeName=='{}'].agentRuntimeArn | [0]",
                cfg.agent_name
            ),
            "--output",
            "text",
        ],
    )
    .await
    .unwrap_or_default();

    if !existing.is_empty() && existing != "None" {
        print!("  Deleting AgentCore runtime... ");
        let _ = cmd_output(
            "aws",
            &[
                "bedrock-agentcore-control",
                "delete-agent-runtime",
                "--agent-runtime-id",
                &existing,
                "--region",
                &cfg.region,
            ],
        )
        .await;
        println!("done");
    }

    // ECR repository
    print!("  Deleting ECR repository... ");
    let _ = cmd_output(
        "aws",
        &[
            "ecr",
            "delete-repository",
            "--repository-name",
            &cfg.agent_name,
            "--region",
            &cfg.region,
            "--force",
        ],
    )
    .await;
    println!("done");

    // IAM role
    let role_name = format!("{}-agentcore-role", cfg.agent_name);
    print!("  Deleting IAM role... ");
    let _ = cmd_output(
        "aws",
        &[
            "iam",
            "detach-role-policy",
            "--role-name",
            &role_name,
            "--policy-arn",
            "arn:aws:iam::aws:policy/AmazonBedrockFullAccess",
        ],
    )
    .await;
    let _ = cmd_output("aws", &["iam", "delete-role", "--role-name", &role_name]).await;
    println!("done");

    println!("\nDestroy complete.");
    Ok(())
}

// ---------------------------------------------------------------------------
// IAM role management
// ---------------------------------------------------------------------------

/// Ensure an IAM role exists for the AgentCore runtime, creating it if needed.
async fn ensure_role(cfg: &Config) -> Result<String> {
    let role_name = format!("{}-agentcore-role", cfg.agent_name);

    // Check for existing role
    if let Ok(arn) = cmd_output(
        "aws",
        &[
            "iam",
            "get-role",
            "--role-name",
            &role_name,
            "--query",
            "Role.Arn",
            "--output",
            "text",
        ],
    )
    .await
    {
        return Ok(arn);
    }

    // Create role with AgentCore trust policy
    print!("  Creating IAM role ({role_name})... ");
    let trust_policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"bedrock-agentcore.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#;

    let arn = cmd_output(
        "aws",
        &[
            "iam",
            "create-role",
            "--role-name",
            &role_name,
            "--assume-role-policy-document",
            trust_policy,
            "--query",
            "Role.Arn",
            "--output",
            "text",
        ],
    )
    .await
    .context("Failed to create IAM role")?;

    // Attach Bedrock access
    let _ = cmd_output(
        "aws",
        &[
            "iam",
            "attach-role-policy",
            "--role-name",
            &role_name,
            "--policy-arn",
            "arn:aws:iam::aws:policy/AmazonBedrockFullAccess",
        ],
    )
    .await;

    println!("done");
    Ok(arn)
}
