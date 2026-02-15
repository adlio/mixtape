use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

/// Resolved deployment configuration, merged from Cargo.toml metadata
/// and CLI overrides.
#[derive(Debug)]
pub struct Config {
    pub agent_name: String,
    pub region: String,
    pub binary: String,
    pub project_root: PathBuf,
}

// Cargo.toml structure (only the fields we need)

#[derive(Deserialize)]
struct CargoToml {
    package: Option<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: Option<String>,
    metadata: Option<Metadata>,
}

#[derive(Deserialize)]
struct Metadata {
    mixtape: Option<MixtapeMetadata>,
}

/// Maps to `[package.metadata.mixtape]` in Cargo.toml:
///
/// ```toml
/// [package.metadata.mixtape]
/// agent-name = "my-agent"
/// region = "us-west-2"
/// binary = "my_binary"
/// ```
#[derive(Deserialize)]
struct MixtapeMetadata {
    #[serde(rename = "agent-name")]
    agent_name: Option<String>,
    region: Option<String>,
    binary: Option<String>,
}

fn find_cargo_toml() -> Result<PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists() {
            return Ok(candidate);
        }
        if !dir.pop() {
            bail!("Could not find Cargo.toml in current or parent directories");
        }
    }
}

/// Load configuration by merging (in order of precedence):
/// 1. CLI arguments
/// 2. `[package.metadata.mixtape]` in Cargo.toml
/// 3. Environment variables / defaults
pub fn load(
    name_override: Option<String>,
    region_override: Option<String>,
    binary_override: Option<String>,
) -> Result<Config> {
    let cargo_toml_path = find_cargo_toml()?;
    let project_root = cargo_toml_path.parent().unwrap().to_path_buf();
    let content = std::fs::read_to_string(&cargo_toml_path).context("Failed to read Cargo.toml")?;
    let parsed: CargoToml = toml::from_str(&content).context("Failed to parse Cargo.toml")?;

    let meta = parsed
        .package
        .as_ref()
        .and_then(|p| p.metadata.as_ref())
        .and_then(|m| m.mixtape.as_ref());

    let package_name = parsed
        .package
        .as_ref()
        .and_then(|p| p.name.clone())
        .unwrap_or_else(|| "mixtape-agent".to_string());

    let agent_name = name_override
        .or_else(|| meta.and_then(|m| m.agent_name.clone()))
        .unwrap_or_else(|| package_name.clone());

    let region = region_override
        .or_else(|| meta.and_then(|m| m.region.clone()))
        .or_else(|| std::env::var("AWS_REGION").ok())
        .or_else(|| std::env::var("AWS_DEFAULT_REGION").ok())
        .unwrap_or_else(|| "us-west-2".to_string());

    let binary = binary_override
        .or_else(|| meta.and_then(|m| m.binary.clone()))
        .unwrap_or(package_name);

    Ok(Config {
        agent_name,
        region,
        binary,
        project_root,
    })
}
