mod config;
mod deploy;

use clap::{Parser, Subcommand};

/// Cargo subcommand for deploying mixtape agents to AWS Bedrock AgentCore.
///
/// Install:
///   cargo install cargo-mixtape
///
/// Configure in your Cargo.toml:
///   [package.metadata.mixtape]
///   agent-name = "my-agent"
///   region = "us-west-2"
///
/// Deploy:
///   cargo mixtape deploy
#[derive(Parser)]
#[command(name = "cargo", bin_name = "cargo")]
enum Cli {
    Mixtape(MixtapeArgs),
}

#[derive(clap::Args)]
#[command(version, about = "Deploy mixtape agents to AWS Bedrock AgentCore")]
struct MixtapeArgs {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Deploy the agent to AgentCore (build, push, register).
    Deploy {
        /// Agent name (overrides Cargo.toml metadata).
        #[arg(long)]
        name: Option<String>,
        /// AWS region.
        #[arg(long)]
        region: Option<String>,
        /// Binary name to build (defaults to package name).
        #[arg(long)]
        binary: Option<String>,
    },
    /// Build and run the agent container locally.
    Local {
        /// Binary name to build.
        #[arg(long)]
        binary: Option<String>,
        /// Host port to map to the container's 8080.
        #[arg(long, default_value = "8080")]
        port: u16,
    },
    /// Show current AgentCore runtime status.
    Status {
        /// Agent name.
        #[arg(long)]
        name: Option<String>,
        /// AWS region.
        #[arg(long)]
        region: Option<String>,
    },
    /// Tear down the AgentCore runtime, ECR repository, and IAM role.
    Destroy {
        /// Agent name.
        #[arg(long)]
        name: Option<String>,
        /// AWS region.
        #[arg(long)]
        region: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let Cli::Mixtape(args) = Cli::parse();

    match args.command {
        Command::Deploy {
            name,
            region,
            binary,
        } => {
            let cfg = config::load(name, region, binary)?;
            deploy::deploy(&cfg).await
        }
        Command::Local { binary, port } => {
            let cfg = config::load(None, None, binary)?;
            deploy::local(&cfg, port).await
        }
        Command::Status { name, region } => {
            let cfg = config::load(name, region, None)?;
            deploy::status(&cfg).await
        }
        Command::Destroy { name, region } => {
            let cfg = config::load(name, region, None)?;
            deploy::destroy(&cfg).await
        }
    }
}
