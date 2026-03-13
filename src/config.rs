use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cli::Environment;

pub const DEFAULT_BASE_URL: &str = "https://data-dbg.krx.co.kr";
pub const DEFAULT_USER_AGENT: &str = concat!("krx-api-cli/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_path: PathBuf,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AppConfig {
    pub user_agent: Option<String>,
    pub profiles: Profiles,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Profiles {
    pub sample: Option<KrxProfile>,
    pub real: Option<KrxProfile>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct KrxProfile {
    pub auth_key: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedProfile {
    pub auth_key: String,
    pub base_url: String,
    pub user_agent: String,
}

impl AppConfig {
    fn profile_for(&self, environment: Environment) -> KrxProfile {
        match environment {
            Environment::Sample => self.profiles.sample.clone().unwrap_or_default(),
            Environment::Real => self.profiles.real.clone().unwrap_or_default(),
        }
    }

    fn profile_for_mut(&mut self, environment: Environment) -> &mut KrxProfile {
        match environment {
            Environment::Sample => self.profiles.sample.get_or_insert_with(KrxProfile::default),
            Environment::Real => self.profiles.real.get_or_insert_with(KrxProfile::default),
        }
    }
}

pub fn app_paths(config_override: Option<&Path>) -> Result<AppPaths> {
    let dirs = ProjectDirs::from("com", "johwanghee", "krx-api-cli")
        .ok_or_else(|| anyhow!("failed to resolve OS-specific app directories"))?;

    let config_path = match config_override {
        Some(path) => path.to_path_buf(),
        None => dirs.config_dir().join("config.toml"),
    };

    Ok(AppPaths { config_path })
}

pub fn write_config_template(path: &Path, force: bool) -> Result<()> {
    if path.exists() && !force {
        return Err(anyhow!(
            "config already exists at {} (use --force to overwrite)",
            path.display()
        ));
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    fs::write(path, template_config())
        .with_context(|| format!("failed to write config template to {}", path.display()))?;

    Ok(())
}

pub fn set_auth_key(
    config_override: Option<&Path>,
    environment: Environment,
    auth_key: &str,
) -> Result<PathBuf> {
    let trimmed = auth_key.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("auth key cannot be empty"));
    }

    let paths = app_paths(config_override)?;
    let mut config = read_config_if_exists(&paths.config_path)?;
    let profile = config.profile_for_mut(environment);
    profile.auth_key = Some(trimmed.to_string());

    if let Some(parent) = paths.config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    write_config(&paths.config_path, &config)?;
    Ok(paths.config_path)
}

pub fn resolve_profile(
    config_override: Option<&Path>,
    environment: Environment,
) -> Result<ResolvedProfile> {
    let paths = app_paths(config_override)?;
    let config = read_config_if_exists(&paths.config_path)?;
    let profile = config.profile_for(environment);

    let user_agent = env::var("KRX_USER_AGENT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or(config.user_agent)
        .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());

    let base_url = env_override(environment, "BASE_URL")
        .or(profile.base_url)
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

    let auth_key = env_override(environment, "AUTH_KEY")
        .or(profile.auth_key)
        .ok_or_else(|| {
            anyhow!(
                "missing AUTH_KEY for {} environment; run `krx-api-cli config set-auth-key --profile {} --stdin` or set `KRX_{}_AUTH_KEY`",
                environment.as_str(),
                environment.as_str(),
                environment.as_str().to_uppercase(),
            )
        })?;

    Ok(ResolvedProfile {
        auth_key,
        base_url,
        user_agent,
    })
}

pub fn redacted_config_value(config_override: Option<&Path>) -> Result<Value> {
    let paths = app_paths(config_override)?;
    let config = read_config_if_exists(&paths.config_path)?;
    Ok(json!({
        "config_path": paths.config_path,
        "exists": paths.config_path.exists(),
        "user_agent": config.user_agent,
        "profiles": {
            "sample": redact_profile(config.profiles.sample),
            "real": redact_profile(config.profiles.real),
        }
    }))
}

fn redact_profile(profile: Option<KrxProfile>) -> Value {
    match profile {
        Some(profile) => json!({
            "auth_key": redact_secret(profile.auth_key),
            "base_url": profile.base_url,
        }),
        None => Value::Null,
    }
}

fn redact_secret(value: Option<String>) -> Option<String> {
    value.map(|secret| {
        if secret.len() <= 8 {
            "********".to_string()
        } else {
            format!("{}...{}", &secret[..4], &secret[secret.len() - 4..])
        }
    })
}

fn env_override(environment: Environment, name: &str) -> Option<String> {
    let global_name = format!("KRX_{name}");
    let scoped_name = format!("KRX_{}_{}", environment.as_str().to_uppercase(), name);

    env::var(&scoped_name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var(&global_name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

fn read_config_if_exists(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("failed to parse config file {}", path.display()))
}

fn write_config(path: &Path, config: &AppConfig) -> Result<()> {
    let rendered = toml::to_string_pretty(config).context("failed to serialize config")?;
    fs::write(path, format!("{rendered}\n"))
        .with_context(|| format!("failed to write config file {}", path.display()))
}

fn template_config() -> String {
    format!(
        concat!(
            "# krx-api-cli configuration\n",
            "# Both sample and real profiles require an AUTH_KEY set through config or environment.\n",
            "# Do not commit AUTH_KEY values into the repository.\n\n",
            "user_agent = \"{user_agent}\"\n\n",
            "[profiles.sample]\n",
            "base_url = \"{base_url}\"\n",
            "# auth_key = \"YOUR_SAMPLE_AUTH_KEY\"\n\n",
            "[profiles.real]\n",
            "base_url = \"{base_url}\"\n",
            "# auth_key = \"YOUR_REAL_AUTH_KEY\"\n"
        ),
        user_agent = DEFAULT_USER_AGENT,
        base_url = DEFAULT_BASE_URL,
    )
}
