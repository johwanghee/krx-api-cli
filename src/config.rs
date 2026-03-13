use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use aes_gcm_siv::{
    aead::{Aead, KeyInit},
    Aes256GcmSiv, Nonce,
};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use directories::ProjectDirs;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cli::Environment;

pub const DEFAULT_BASE_URL: &str = "https://data-dbg.krx.co.kr";
pub const DEFAULT_USER_AGENT: &str = concat!("krx-api-cli/", env!("CARGO_PKG_VERSION"));

const CONFIG_KEY_BYTES: usize = 32;
const CONFIG_NONCE_BYTES: usize = 12;
const ENCRYPTED_VALUE_PREFIX: &str = "enc:krx:v1:";
const KEY_FILE_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_path: PathBuf,
    pub key_path: PathBuf,
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

#[derive(Debug, Clone)]
pub struct SetAuthKeyResult {
    pub profile: Environment,
    pub config_path: PathBuf,
    pub key_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SealConfigResult {
    pub encrypted_fields: usize,
    pub profiles_touched: usize,
    pub config_path: PathBuf,
    pub key_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct KeyStatusResult {
    pub key_path: PathBuf,
    pub key_exists: bool,
    pub key_format: Option<&'static str>,
    pub previous_key_count: usize,
    pub encrypted_field_count: usize,
    pub plaintext_field_count: usize,
    pub plaintext_fields: Vec<String>,
    pub seal_required: bool,
    pub suggested_commands: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PlaintextSecretError {
    pub config_path: PathBuf,
    pub plaintext_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeyFile {
    version: u32,
    active_key: String,
    #[serde(default)]
    previous_keys: Vec<String>,
}

#[derive(Debug, Clone)]
struct KeyMaterial {
    active: [u8; CONFIG_KEY_BYTES],
    previous: Vec<[u8; CONFIG_KEY_BYTES]>,
    format: &'static str,
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

impl std::fmt::Display for PlaintextSecretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "plaintext sensitive config values detected in {}: {}",
            self.config_path.display(),
            self.plaintext_fields.join(", ")
        )
    }
}

impl std::error::Error for PlaintextSecretError {}

pub fn app_paths(config_override: Option<&Path>) -> Result<AppPaths> {
    let dirs = ProjectDirs::from("com", "johwanghee", "krx-api-cli")
        .ok_or_else(|| anyhow!("failed to resolve OS-specific app directories"))?;

    let config_path = match config_override {
        Some(path) => path.to_path_buf(),
        None => dirs.config_dir().join("config.toml"),
    };
    let key_path = match config_override {
        Some(path) => path.with_extension("key"),
        None => dirs.data_local_dir().join("config.key"),
    };

    Ok(AppPaths {
        config_path,
        key_path,
    })
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
) -> Result<SetAuthKeyResult> {
    let trimmed = auth_key.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("auth key cannot be empty"));
    }

    let paths = app_paths(config_override)?;
    ensure_config_exists(&paths.config_path)?;

    let mut config = read_config(&paths.config_path)?;
    let encrypted = encrypt_secret(&paths, trimmed)?;
    config.profile_for_mut(environment).auth_key = Some(encrypted);
    write_config(&paths.config_path, &config)?;

    Ok(SetAuthKeyResult {
        profile: environment,
        config_path: paths.config_path,
        key_path: paths.key_path,
    })
}

pub fn seal_config(
    config_override: Option<&Path>,
    environment: Option<Environment>,
) -> Result<SealConfigResult> {
    let paths = app_paths(config_override)?;

    if !paths.config_path.exists() {
        bail!(
            "config file does not exist at {}. Run `krx-api-cli config init` first.",
            paths.config_path.display()
        );
    }

    let mut config = read_config(&paths.config_path)?;
    let mut encrypted_fields = 0;
    let mut profiles_touched = 0;

    for current in selected_profiles(environment) {
        let Some(profile) = profile_option_mut(&mut config.profiles, current) else {
            continue;
        };

        let Some(value) = profile.auth_key.as_ref() else {
            continue;
        };

        if value.trim().is_empty() || is_encrypted(value) {
            continue;
        }

        profile.auth_key = Some(encrypt_secret(&paths, value)?);
        encrypted_fields += 1;
        profiles_touched += 1;
    }

    if encrypted_fields > 0 {
        write_config(&paths.config_path, &config)?;
    }

    Ok(SealConfigResult {
        encrypted_fields,
        profiles_touched,
        config_path: paths.config_path,
        key_path: paths.key_path,
    })
}

pub fn key_status(config_override: Option<&Path>) -> Result<KeyStatusResult> {
    let paths = app_paths(config_override)?;
    let config = load_config_or_default(&paths.config_path)?;
    let encrypted_field_count = count_encrypted_secret_fields(&config);
    let plaintext_fields = collect_plaintext_secret_fields(&config);
    let plaintext_field_count = plaintext_fields.len();
    let seal_required = plaintext_field_count > 0;
    let suggested_commands = if seal_required {
        vec![
            "krx-api-cli config key status --compact".to_string(),
            "krx-api-cli config seal".to_string(),
        ]
    } else {
        Vec::new()
    };

    if !paths.key_path.exists() {
        return Ok(KeyStatusResult {
            key_path: paths.key_path,
            key_exists: false,
            key_format: None,
            previous_key_count: 0,
            encrypted_field_count,
            plaintext_field_count,
            plaintext_fields,
            seal_required,
            suggested_commands,
        });
    }

    let key_material = load_key_material_from_path(&paths.key_path)?;
    Ok(KeyStatusResult {
        key_path: paths.key_path,
        key_exists: true,
        key_format: Some(key_material.format),
        previous_key_count: key_material.previous.len(),
        encrypted_field_count,
        plaintext_field_count,
        plaintext_fields,
        seal_required,
        suggested_commands,
    })
}

pub fn resolve_profile(
    config_override: Option<&Path>,
    environment: Environment,
) -> Result<ResolvedProfile> {
    let paths = app_paths(config_override)?;
    let config = load_config_or_default(&paths.config_path)?;
    ensure_no_plaintext_secret_fields(&config, &paths.config_path)?;
    let profile = config.profile_for(environment);

    let user_agent = env::var("KRX_USER_AGENT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or(config.user_agent)
        .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());

    let base_url = env_override(environment, "BASE_URL")
        .or(profile.base_url)
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

    let auth_key = if let Some(value) = env_override(environment, "AUTH_KEY") {
        value
    } else {
        resolve_secret_value(profile.auth_key, &paths, "auth_key")?
            .ok_or_else(|| missing_auth_key_error(environment, &paths.config_path))?
    };

    Ok(ResolvedProfile {
        auth_key,
        base_url,
        user_agent,
    })
}

pub fn redacted_config_value(config_override: Option<&Path>) -> Result<Value> {
    let paths = app_paths(config_override)?;
    let config = load_config_or_default(&paths.config_path)?;
    let plaintext_fields = collect_plaintext_secret_fields(&config);

    Ok(json!({
        "config_path": paths.config_path,
        "exists": paths.config_path.exists(),
        "key_path": paths.key_path,
        "key_exists": paths.key_path.exists(),
        "encrypted_field_count": count_encrypted_secret_fields(&config),
        "plaintext_field_count": plaintext_fields.len(),
        "plaintext_fields": plaintext_fields,
        "seal_required": !collect_plaintext_secret_fields(&config).is_empty(),
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
            "auth_key": secret_summary(profile.auth_key.as_deref()),
            "base_url": profile.base_url,
        }),
        None => Value::Null,
    }
}

fn secret_summary(value: Option<&str>) -> Value {
    match value {
        Some(value) if !value.trim().is_empty() => json!({
            "present": true,
            "storage": secret_storage_state(value),
            "preview": redact_secret_preview(value),
        }),
        _ => json!({
            "present": false,
            "storage": "absent",
            "preview": Value::Null,
        }),
    }
}

fn secret_storage_state(value: &str) -> &'static str {
    if is_encrypted(value) {
        "encrypted"
    } else {
        "plaintext"
    }
}

fn redact_secret_preview(value: &str) -> String {
    if is_encrypted(value) {
        "<encrypted>".to_string()
    } else {
        "<redacted>".to_string()
    }
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

fn load_config_or_default(path: &Path) -> Result<AppConfig> {
    if path.exists() {
        read_config(path)
    } else {
        Ok(AppConfig::default())
    }
}

fn read_config(path: &Path) -> Result<AppConfig> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("failed to parse config file {}", path.display()))
}

fn write_config(path: &Path, config: &AppConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    let rendered = toml::to_string_pretty(config).context("failed to serialize config")?;
    fs::write(path, format!("{rendered}\n"))
        .with_context(|| format!("failed to write config file {}", path.display()))
}

fn ensure_config_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        write_config_template(path, false)?;
    }

    Ok(())
}

fn resolve_secret_value(
    value: Option<String>,
    paths: &AppPaths,
    field: &str,
) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };

    if value.trim().is_empty() {
        return Ok(None);
    }

    if !is_encrypted(&value) {
        return Ok(Some(value));
    }

    decrypt_secret(paths, &value)
        .with_context(|| format!("failed to decrypt config field `{field}`"))
        .map(Some)
}

fn encrypt_secret(paths: &AppPaths, plaintext: &str) -> Result<String> {
    let key_material = load_or_create_key_material(paths)?;
    encrypt_secret_with_key(&key_material.active, plaintext)
}

fn encrypt_secret_with_key(key: &[u8; CONFIG_KEY_BYTES], plaintext: &str) -> Result<String> {
    let cipher =
        Aes256GcmSiv::new_from_slice(key).map_err(|_| anyhow!("invalid config encryption key"))?;
    let mut nonce_bytes = [0u8; CONFIG_NONCE_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|_| anyhow!("failed to encrypt config secret"))?;

    let mut payload = nonce_bytes.to_vec();
    payload.extend_from_slice(&ciphertext);
    Ok(format!(
        "{ENCRYPTED_VALUE_PREFIX}{}",
        STANDARD_NO_PAD.encode(payload)
    ))
}

fn decrypt_secret(paths: &AppPaths, value: &str) -> Result<String> {
    let key_material = load_existing_key_material(paths)?;
    decrypt_secret_with_key_material(&key_material, value)
}

fn decrypt_secret_with_key_material(key_material: &KeyMaterial, value: &str) -> Result<String> {
    let encoded = value
        .strip_prefix(ENCRYPTED_VALUE_PREFIX)
        .ok_or_else(|| anyhow!("unsupported encrypted config value format"))?;
    let payload = STANDARD_NO_PAD
        .decode(encoded)
        .context("invalid encrypted config payload")?;

    if payload.len() <= CONFIG_NONCE_BYTES {
        bail!("encrypted config payload is too short");
    }

    let (nonce_bytes, ciphertext) = payload.split_at(CONFIG_NONCE_BYTES);
    for key in std::iter::once(&key_material.active).chain(key_material.previous.iter()) {
        if let Ok(plaintext) = decrypt_secret_with_key(key, nonce_bytes, ciphertext) {
            return Ok(plaintext);
        }
    }

    bail!("failed to decrypt config secret with available key material")
}

fn decrypt_secret_with_key(
    key: &[u8; CONFIG_KEY_BYTES],
    nonce_bytes: &[u8],
    ciphertext: &[u8],
) -> Result<String> {
    let cipher =
        Aes256GcmSiv::new_from_slice(key).map_err(|_| anyhow!("invalid config encryption key"))?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|_| anyhow!("failed to decrypt config secret"))?;
    String::from_utf8(plaintext).context("config secret is not valid UTF-8")
}

fn is_encrypted(value: &str) -> bool {
    value.starts_with(ENCRYPTED_VALUE_PREFIX)
}

fn load_or_create_key_material(paths: &AppPaths) -> Result<KeyMaterial> {
    if paths.key_path.exists() {
        return load_key_material_from_path(&paths.key_path);
    }

    create_key_material(paths)
}

fn load_existing_key_material(paths: &AppPaths) -> Result<KeyMaterial> {
    if !paths.key_path.exists() {
        bail!(
            "missing config encryption key at {}. Restore the original key file or re-enter secrets.",
            paths.key_path.display()
        );
    }

    load_key_material_from_path(&paths.key_path)
}

fn load_key_material_from_path(path: &Path) -> Result<KeyMaterial> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read config key file {}", path.display()))?;
    let key_file: KeyFile = toml::from_str(&raw)
        .with_context(|| format!("failed to parse config key file {}", path.display()))?;

    if key_file.version != KEY_FILE_VERSION {
        bail!(
            "unsupported config key file version {} in {}",
            key_file.version,
            path.display()
        );
    }

    let active = decode_key_bytes(&key_file.active_key, path)?;
    let previous = key_file
        .previous_keys
        .iter()
        .map(|value| decode_key_bytes(value, path))
        .collect::<Result<Vec<_>>>()?;
    let mut key_material = KeyMaterial {
        active,
        previous,
        format: "keyring",
    };
    normalize_key_material(&mut key_material);
    Ok(key_material)
}

fn create_key_material(paths: &AppPaths) -> Result<KeyMaterial> {
    if let Some(parent) = paths.key_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create key directory {}", parent.display()))?;
    }

    let key_material = KeyMaterial {
        active: generate_random_key(),
        previous: Vec::new(),
        format: "keyring",
    };
    write_key_material_to_path(&paths.key_path, &key_material)?;
    Ok(key_material)
}

fn write_key_material_to_path(path: &Path, key_material: &KeyMaterial) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create key directory {}", parent.display()))?;
    }

    let key_file = KeyFile {
        version: KEY_FILE_VERSION,
        active_key: encode_key_bytes(&key_material.active),
        previous_keys: key_material.previous.iter().map(encode_key_bytes).collect(),
    };
    let rendered = toml::to_string_pretty(&key_file).context("failed to render config key TOML")?;
    fs::write(path, format!("{rendered}\n"))
        .with_context(|| format!("failed to write config encryption key {}", path.display()))?;
    restrict_key_permissions(path)?;
    Ok(())
}

fn generate_random_key() -> [u8; CONFIG_KEY_BYTES] {
    let mut key = [0u8; CONFIG_KEY_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut key);
    key
}

fn encode_key_bytes(key: &[u8; CONFIG_KEY_BYTES]) -> String {
    STANDARD_NO_PAD.encode(key)
}

fn decode_key_bytes(encoded: &str, path: &Path) -> Result<[u8; CONFIG_KEY_BYTES]> {
    let decoded = STANDARD_NO_PAD
        .decode(encoded)
        .with_context(|| format!("failed to decode config key file {}", path.display()))?;

    if decoded.len() != CONFIG_KEY_BYTES {
        bail!(
            "invalid config key length in {} (expected {} bytes)",
            path.display(),
            CONFIG_KEY_BYTES
        );
    }

    let mut key = [0u8; CONFIG_KEY_BYTES];
    key.copy_from_slice(&decoded);
    Ok(key)
}

fn normalize_key_material(key_material: &mut KeyMaterial) {
    let mut unique = Vec::new();
    for candidate in key_material.previous.drain(..) {
        if candidate == key_material.active || unique.iter().any(|existing| *existing == candidate)
        {
            continue;
        }
        unique.push(candidate);
    }
    key_material.previous = unique;
}

fn count_encrypted_secret_fields(config: &AppConfig) -> usize {
    [
        config.profiles.sample.as_ref(),
        config.profiles.real.as_ref(),
    ]
    .into_iter()
    .flatten()
    .filter_map(|profile| profile.auth_key.as_ref())
    .filter(|value| !value.trim().is_empty() && is_encrypted(value))
    .count()
}

fn collect_plaintext_secret_fields(config: &AppConfig) -> Vec<String> {
    let mut fields = Vec::new();

    for (environment, profile) in [
        (Environment::Sample, config.profiles.sample.as_ref()),
        (Environment::Real, config.profiles.real.as_ref()),
    ] {
        let Some(profile) = profile else {
            continue;
        };

        let Some(value) = profile.auth_key.as_ref() else {
            continue;
        };

        if !value.trim().is_empty() && !is_encrypted(value) {
            fields.push(format!("profiles.{}.auth_key", environment.as_str()));
        }
    }

    fields
}

fn ensure_no_plaintext_secret_fields(config: &AppConfig, config_path: &Path) -> Result<()> {
    let plaintext_fields = collect_plaintext_secret_fields(config);
    if plaintext_fields.is_empty() {
        return Ok(());
    }

    Err(PlaintextSecretError {
        config_path: config_path.to_path_buf(),
        plaintext_fields,
    }
    .into())
}

#[cfg(unix)]
fn restrict_key_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions).with_context(|| {
        format!(
            "failed to apply restrictive permissions to {}",
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_key_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn profile_option_mut(
    profiles: &mut Profiles,
    environment: Environment,
) -> Option<&mut KrxProfile> {
    match environment {
        Environment::Sample => profiles.sample.as_mut(),
        Environment::Real => profiles.real.as_mut(),
    }
}

fn selected_profiles(environment: Option<Environment>) -> Vec<Environment> {
    match environment {
        Some(environment) => vec![environment],
        None => vec![Environment::Sample, Environment::Real],
    }
}

fn missing_auth_key_error(environment: Environment, config_path: &Path) -> anyhow::Error {
    anyhow!(
        "missing AUTH_KEY for {} environment; run `krx-api-cli config set-auth-key --profile {} --stdin`, `krx-api-cli config init`, or set `KRX_{}_AUTH_KEY`. Expected config file: {}",
        environment.as_str(),
        environment.as_str(),
        environment.as_str().to_uppercase(),
        config_path.display()
    )
}

fn template_config() -> String {
    format!(
        concat!(
            "# krx-api-cli configuration\n",
            "#\n",
            "# Recommended secret flow:\n",
            "#   1. Fill non-secret values here.\n",
            "#   2. Store AUTH_KEY with `krx-api-cli config set-auth-key`.\n",
            "#   3. If this file already contains plaintext auth_key values, run `krx-api-cli config seal`.\n",
            "#   4. API commands refuse plaintext config auth_key values until they are sealed.\n",
            "#\n",
            "# Environment variable overrides remain supported and are used as plaintext.\n",
            "# Examples:\n",
            "#   KRX_SAMPLE_AUTH_KEY\n",
            "#   KRX_REAL_AUTH_KEY\n",
            "#   KRX_AUTH_KEY\n",
            "#\n",
            "user_agent = \"{user_agent}\"\n",
            "\n",
            "[profiles.sample]\n",
            "base_url = \"{base_url}\"\n",
            "auth_key = \"\"\n",
            "\n",
            "[profiles.real]\n",
            "base_url = \"{base_url}\"\n",
            "auth_key = \"\"\n"
        ),
        user_agent = DEFAULT_USER_AGENT,
        base_url = DEFAULT_BASE_URL,
    )
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_config_path(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("current time should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "krx-api-cli-{label}-{}-{stamp}.toml",
            std::process::id()
        ))
    }

    fn cleanup_config_artifacts(config_path: &Path) {
        let _ = fs::remove_file(config_path);
        let _ = fs::remove_file(config_path.with_extension("key"));
    }

    #[test]
    fn set_auth_key_encrypts_value_in_config() {
        let config_path = temp_config_path("encrypt");
        cleanup_config_artifacts(&config_path);

        let result = set_auth_key(Some(&config_path), Environment::Real, "SECRET12345678")
            .expect("set_auth_key should succeed");
        let raw = fs::read_to_string(&result.config_path).expect("config should be readable");

        assert!(raw.contains(ENCRYPTED_VALUE_PREFIX));
        assert!(!raw.contains("SECRET12345678"));
        assert!(result.key_path.exists());

        let resolved =
            resolve_profile(Some(&config_path), Environment::Real).expect("profile should load");
        assert_eq!(resolved.auth_key, "SECRET12345678");

        cleanup_config_artifacts(&config_path);
    }

    #[test]
    fn seal_config_encrypts_existing_plaintext_auth_keys() {
        let config_path = temp_config_path("seal");
        cleanup_config_artifacts(&config_path);

        fs::write(
            &config_path,
            concat!(
                "user_agent = \"krx-api-cli/test\"\n\n",
                "[profiles.sample]\n",
                "base_url = \"https://data-dbg.krx.co.kr\"\n",
                "auth_key = \"SAMPLE_PLAINTEXT\"\n"
            ),
        )
        .expect("plaintext config should be writable");

        let result =
            seal_config(Some(&config_path), Some(Environment::Sample)).expect("seal should work");
        assert_eq!(result.encrypted_fields, 1);

        let raw = fs::read_to_string(&config_path).expect("config should be readable");
        assert!(raw.contains(ENCRYPTED_VALUE_PREFIX));
        assert!(!raw.contains("SAMPLE_PLAINTEXT"));

        cleanup_config_artifacts(&config_path);
    }

    #[test]
    fn resolve_profile_rejects_plaintext_config_auth_key() {
        let config_path = temp_config_path("plaintext");
        cleanup_config_artifacts(&config_path);

        fs::write(
            &config_path,
            concat!(
                "[profiles.real]\n",
                "base_url = \"https://data-dbg.krx.co.kr\"\n",
                "auth_key = \"REAL_PLAINTEXT\"\n"
            ),
        )
        .expect("plaintext config should be writable");

        let error = resolve_profile(Some(&config_path), Environment::Real)
            .expect_err("plaintext config should be rejected");
        let plaintext = error
            .downcast_ref::<PlaintextSecretError>()
            .expect("error should preserve plaintext secret type");
        assert_eq!(
            plaintext.plaintext_fields,
            vec!["profiles.real.auth_key".to_string()]
        );

        cleanup_config_artifacts(&config_path);
    }
}
