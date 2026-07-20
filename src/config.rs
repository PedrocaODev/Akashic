use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Config {
    pub(crate) log_level: String,
    pub(crate) shutdown_timeout_seconds: u64,
    pub(crate) jsonl_max_line_bytes: usize,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Overrides {
    pub(crate) log_level: Option<String>,
    pub(crate) shutdown_timeout_seconds: Option<String>,
    pub(crate) jsonl_max_line_bytes: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConfigError {
    pub(crate) code: &'static str,
    pub(crate) message: &'static str,
}

#[derive(Default)]
struct Partial {
    log_level: Option<String>,
    shutdown_timeout_seconds: Option<u64>,
    jsonl_max_line_bytes: Option<usize>,
}

impl Config {
    pub(crate) fn summary(&self) -> String {
        format!(
            "log_level={}; shutdown_timeout_seconds={}; jsonl_max_line_bytes={}",
            self.log_level, self.shutdown_timeout_seconds, self.jsonl_max_line_bytes
        )
    }
}

pub(crate) fn resolve(overrides: &Overrides) -> Result<Config, ConfigError> {
    let mut config = Config {
        log_level: "info".to_string(),
        shutdown_timeout_seconds: 10,
        jsonl_max_line_bytes: 1_048_576,
    };
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let config_home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let user_path = config_home.join("akashic/config.toml");
    let project_path = project_config_path(&env::current_dir().map_err(|_| invalid())?);

    if let Some(partial) = read_config(&user_path)? {
        apply_partial(&mut config, partial);
    }
    if let Some(partial) = read_config(&project_path)? {
        apply_partial(&mut config, partial);
    }

    if let Some(value) = env::var_os("AKASHIC_LOG_LEVEL") {
        config.log_level = parse_log_level(&value.to_string_lossy())?;
    }
    if let Some(value) = env::var_os("AKASHIC_SHUTDOWN_TIMEOUT_SECONDS") {
        config.shutdown_timeout_seconds = parse_timeout(&value.to_string_lossy())?;
    }
    if let Some(value) = env::var_os("AKASHIC_JSONL_MAX_LINE_BYTES") {
        config.jsonl_max_line_bytes = parse_line_bytes(&value.to_string_lossy())?;
    }

    if let Some(value) = &overrides.log_level {
        config.log_level = parse_log_level(value)?;
    }
    if let Some(value) = &overrides.shutdown_timeout_seconds {
        config.shutdown_timeout_seconds = parse_timeout(value)?;
    }
    if let Some(value) = &overrides.jsonl_max_line_bytes {
        config.jsonl_max_line_bytes = parse_line_bytes(value)?;
    }
    Ok(config)
}

pub(crate) fn validate_overrides(overrides: &Overrides) -> Result<(), ConfigError> {
    if let Some(value) = &overrides.log_level {
        parse_log_level(value)?;
    }
    if let Some(value) = &overrides.shutdown_timeout_seconds {
        parse_timeout(value)?;
    }
    if let Some(value) = &overrides.jsonl_max_line_bytes {
        parse_line_bytes(value)?;
    }
    Ok(())
}

fn project_config_path(cwd: &Path) -> PathBuf {
    let mut current = Some(cwd);
    while let Some(path) = current {
        if path.join(".git").exists() {
            return path.join(".akashic/config.toml");
        }
        current = path.parent();
    }
    cwd.join(".akashic/config.toml")
}

fn read_config(path: &Path) -> Result<Option<Partial>, ConfigError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(invalid()),
    };
    let mut partial = Partial::default();
    let mut version_seen = false;
    let mut seen_keys = HashSet::new();
    for raw_line in contents.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(invalid());
        };
        let key = key.trim();
        let value = value.trim();
        if secret_like(key) {
            return Err(secret_forbidden());
        }
        if !seen_keys.insert(key) {
            return Err(invalid());
        }
        match key {
            "config_version" => {
                if version_seen {
                    return Err(invalid());
                }
                version_seen = true;
                match parse_integer(value) {
                    Some(1) => {}
                    Some(_) => return Err(unsupported_version()),
                    None => return Err(invalid()),
                }
            }
            "log_level" => partial.log_level = Some(parse_quoted(value).ok_or_else(invalid)?),
            "shutdown_timeout_seconds" => {
                partial.shutdown_timeout_seconds = Some(parse_integer(value).ok_or_else(invalid)?)
            }
            "jsonl_max_line_bytes" => {
                partial.jsonl_max_line_bytes = Some(
                    usize::try_from(parse_integer(value).ok_or_else(invalid)?)
                        .map_err(|_| invalid())?,
                )
            }
            _ => return Err(invalid()),
        }
    }
    if !version_seen {
        return Err(invalid());
    }
    validate_partial(&partial)?;
    Ok(Some(partial))
}

fn apply_partial(config: &mut Config, partial: Partial) {
    if let Some(value) = partial.log_level {
        config.log_level = value;
    }
    if let Some(value) = partial.shutdown_timeout_seconds {
        config.shutdown_timeout_seconds = value;
    }
    if let Some(value) = partial.jsonl_max_line_bytes {
        config.jsonl_max_line_bytes = value;
    }
}

fn validate_partial(partial: &Partial) -> Result<(), ConfigError> {
    if let Some(value) = &partial.log_level {
        validate_log_level(value)?;
    }
    if let Some(value) = partial.shutdown_timeout_seconds {
        validate_timeout(value)?;
    }
    if let Some(value) = partial.jsonl_max_line_bytes {
        validate_line_bytes(value)?;
    }
    Ok(())
}

fn parse_log_level(value: &str) -> Result<String, ConfigError> {
    validate_log_level(value)?;
    Ok(value.to_string())
}

fn validate_log_level(value: &str) -> Result<(), ConfigError> {
    if matches!(value, "error" | "warn" | "info" | "debug" | "trace") {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn parse_timeout(value: &str) -> Result<u64, ConfigError> {
    let value = parse_integer(value).ok_or_else(invalid)?;
    validate_timeout(value)?;
    Ok(value)
}

fn validate_timeout(value: u64) -> Result<(), ConfigError> {
    if (1..=600).contains(&value) {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn parse_line_bytes(value: &str) -> Result<usize, ConfigError> {
    let value =
        usize::try_from(parse_integer(value).ok_or_else(invalid)?).map_err(|_| invalid())?;
    validate_line_bytes(value)?;
    Ok(value)
}

fn validate_line_bytes(value: usize) -> Result<(), ConfigError> {
    if (1024..=1_048_576).contains(&value) {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn parse_quoted(value: &str) -> Option<String> {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .map(str::to_string)
}

fn parse_integer(value: &str) -> Option<u64> {
    (!value.is_empty() && value.chars().all(|character| character.is_ascii_digit()))
        .then(|| value.parse().ok())
        .flatten()
}

fn secret_like(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "token",
        "secret",
        "password",
        "api_key",
        "authorization",
        "cookie",
        "credential",
    ]
    .iter()
    .any(|part| key.contains(part))
}

fn invalid() -> ConfigError {
    ConfigError {
        code: "config.invalid",
        message: "invalid configuration",
    }
}

fn unsupported_version() -> ConfigError {
    ConfigError {
        code: "config.unsupported_version",
        message: "invalid configuration",
    }
}

fn secret_forbidden() -> ConfigError {
    ConfigError {
        code: "config.secret_forbidden",
        message: "invalid configuration",
    }
}
