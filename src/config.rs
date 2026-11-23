use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use tracing::{info, warn};

/// Default Telegram Bot API base URL.
pub const DEFAULT_TELEGRAM_API_BASE: &str = "https://api.telegram.org";

/// Default download directory inside container or bare metal.
pub const DEFAULT_DOWNLOAD_DIR: &str = "/downloads";

/// CLI startup mode selected by command-line arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliMode {
    /// Run only the initial web setup wizard on port 8080 and exit after completion.
    SetupOnly,
    /// Run setup wizard if `TELEGRAM_TOKEN` is missing, then run status dashboard and bot polling.
    Normal,
}

/// Parses command-line arguments to determine the startup mode.
#[must_use]
pub fn parse_cli_mode(args: &[String]) -> CliMode {
    if args.get(1).is_some_and(|arg| arg == "setup") {
        CliMode::SetupOnly
    } else {
        CliMode::Normal
    }
}

/// Application configuration loaded from `.env` files and process environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub telegram_token: Option<String>,
    pub allowed_user_id: Option<i64>,
    pub download_dir: PathBuf,
    pub instagram_user: Option<String>,
    pub instagram_password: Option<String>,
    pub config_dir: PathBuf,
    pub telegram_api_base: String,
    pub web_port: u16,
}

impl Config {
    /// Loads configuration using standard `.env` candidate paths and environment variables.
    #[must_use]
    pub fn load() -> Self {
        let config_dir = Self::resolve_write_dir();
        let candidates = Self::default_env_candidates(None);
        Self::load_from_paths(&candidates, config_dir, |key| std::env::var(key).ok())
    }

    /// Loads configuration exclusively from a specific directory's `.env` and current env.
    #[must_use]
    pub fn load_from_dir(dir: &Path) -> Self {
        let candidates = Self::default_env_candidates(Some(dir));
        Self::load_from_paths(&candidates, dir.to_path_buf(), |key| {
            std::env::var(key).ok()
        })
    }

    /// Loads configuration from explicit `.env` candidate paths and a custom environment lookup.
    #[must_use]
    pub fn load_from_paths<F>(candidates: &[PathBuf], config_dir: PathBuf, env_lookup: F) -> Self
    where
        F: Fn(&str) -> Option<String>,
    {
        let mut dotenv_map = HashMap::new();
        for path in candidates {
            if path.exists() {
                match fs::read_to_string(path) {
                    Ok(content) => {
                        dotenv_map = parse_dotenv_content(&content);
                        info!(path = %path.display(), "Loaded .env configuration file");
                        break;
                    }
                    Err(err) => {
                        warn!(path = %path.display(), error = %err, "Failed to read .env file");
                    }
                }
            }
        }

        let get_non_empty = |key: &str| -> Option<String> {
            dotenv_map
                .get(key)
                .filter(|v| !v.trim().is_empty())
                .cloned()
                .or_else(|| env_lookup(key).filter(|v| !v.trim().is_empty()))
                .map(|v| v.trim().to_string())
        };

        let telegram_token = get_non_empty("TELEGRAM_TOKEN");
        let allowed_user_id = get_non_empty("ALLOWED_USER_ID").and_then(|v| v.parse::<i64>().ok());

        let download_dir = env_lookup("DOWNLOAD_DIR")
            .filter(|v| !v.trim().is_empty())
            .or_else(|| {
                dotenv_map
                    .get("DOWNLOAD_DIR")
                    .filter(|v| !v.trim().is_empty())
                    .cloned()
            })
            .or_else(|| get_non_empty("DOWNLOAD_PATH"))
            .map_or_else(|| PathBuf::from(DEFAULT_DOWNLOAD_DIR), PathBuf::from);

        let instagram_user = get_non_empty("INSTAGRAM_USER");
        let instagram_password = get_non_empty("INSTAGRAM_PASSWORD");

        let telegram_api_base = get_non_empty("TELEGRAM_API_BASE")
            .unwrap_or_else(|| DEFAULT_TELEGRAM_API_BASE.to_string());

        let web_port = get_non_empty("WEB_PORT")
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(8080);

        Self {
            telegram_token,
            allowed_user_id,
            download_dir,
            instagram_user,
            instagram_password,
            config_dir,
            telegram_api_base,
            web_port,
        }
    }

    /// Returns `true` when `TELEGRAM_TOKEN` is present and non-empty.
    #[must_use]
    pub fn has_token(&self) -> bool {
        self.telegram_token
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty())
    }

    /// Checks whether a Telegram user ID is allowed to interact with the bot.
    ///
    /// When `ALLOWED_USER_ID` is unset (or unparseable), every user is allowed,
    /// matching the original Python bot behavior.
    #[must_use]
    pub fn is_user_allowed(&self, user_id: i64) -> bool {
        match self.allowed_user_id {
            Some(allowed) => user_id == allowed,
            None => true,
        }
    }

    /// Returns the path where `.env` should be written or deleted during reconfiguration.
    #[must_use]
    pub fn env_file_path(&self) -> PathBuf {
        self.config_dir.join(".env")
    }

    /// Returns candidate paths where Instagram `cookies.txt` may exist.
    #[must_use]
    pub fn cookie_candidates(&self) -> Vec<PathBuf> {
        let raw = [
            PathBuf::from("/app/cookies.txt"),
            PathBuf::from("/downloads/www.instagram.com_cookies.txt"),
            PathBuf::from("/downloads/cookies.txt"),
            self.download_dir.join("www.instagram.com_cookies.txt"),
            self.download_dir.join("cookies.txt"),
            self.config_dir.join("cookies.txt"),
            PathBuf::from("./cookies.txt"),
        ];
        let mut deduped = Vec::with_capacity(raw.len());
        for path in raw {
            if !deduped.contains(&path) {
                deduped.push(path);
            }
        }
        deduped
    }

    /// Returns `.env` candidate paths in priority order.
    #[must_use]
    pub fn default_env_candidates(base_dir: Option<&Path>) -> Vec<PathBuf> {
        if let Some(dir) = base_dir {
            return vec![dir.join(".env")];
        }

        let mut candidates = Vec::with_capacity(3);
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join(".env"));
        }
        let app_env = PathBuf::from("/app/.env");
        if !candidates.contains(&app_env) {
            candidates.push(app_env);
        }
        let rel_env = PathBuf::from(".env");
        if !candidates.contains(&rel_env) {
            candidates.push(rel_env);
        }
        candidates
    }

    /// Resolves the directory where `.env` and `cookies.txt` should be saved.
    #[must_use]
    pub fn resolve_write_dir() -> PathBuf {
        if let Ok(cwd) = std::env::current_dir()
            && cwd != Path::new("/")
        {
            return cwd;
        }
        let app_dir = Path::new("/app");
        if app_dir.is_dir() {
            return app_dir.to_path_buf();
        }
        PathBuf::from(".")
    }
}

/// Parses `.env` file contents into key-value pairs without mutating global process state.
#[must_use]
pub fn parse_dotenv_content(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").map_or(line, str::trim_start);
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim();
        if key.is_empty() {
            continue;
        }
        let val = strip_matching_quotes(v.trim());
        map.insert(key.to_string(), val.to_string());
    }
    map
}

fn strip_matching_quotes(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        let first = bytes[0];
        let last = bytes[value.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &value[1..value.len() - 1];
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_cli_mode_recognizes_setup_and_default() {
        assert_eq!(parse_cli_mode(&["bot".into()]), CliMode::Normal);
        assert_eq!(
            parse_cli_mode(&["bot".into(), "setup".into()]),
            CliMode::SetupOnly
        );
        assert_eq!(
            parse_cli_mode(&["bot".into(), "other".into()]),
            CliMode::Normal
        );
    }

    #[test]
    fn parse_dotenv_content_handles_comments_quotes_and_equals() {
        let raw = r#"
# Comment line
TELEGRAM_TOKEN=123456:ABC=DEF
ALLOWED_USER_ID="987654321"
export INSTAGRAM_USER='my_user'
EMPTY_VAL=
MALFORMED_LINE
"#;
        let parsed = parse_dotenv_content(raw);
        assert_eq!(
            parsed.get("TELEGRAM_TOKEN").map(String::as_str),
            Some("123456:ABC=DEF")
        );
        assert_eq!(
            parsed.get("ALLOWED_USER_ID").map(String::as_str),
            Some("987654321")
        );
        assert_eq!(
            parsed.get("INSTAGRAM_USER").map(String::as_str),
            Some("my_user")
        );
        assert_eq!(parsed.get("EMPTY_VAL").map(String::as_str), Some(""));
        assert!(!parsed.contains_key("MALFORMED_LINE"));
    }

    #[test]
    fn load_from_paths_loads_dotenv_and_honors_download_dir_fallback() {
        let tmp = TempDir::new().expect("tempdir");
        let env_path = tmp.path().join(".env");
        fs::write(
            &env_path,
            "TELEGRAM_TOKEN=tok123\nALLOWED_USER_ID=42\nDOWNLOAD_PATH=./custom_dl\n",
        )
        .expect("write .env");

        let cfg = Config::load_from_paths(&[env_path], tmp.path().to_path_buf(), |_| None);
        assert!(cfg.has_token());
        assert_eq!(cfg.telegram_token.as_deref(), Some("tok123"));
        assert_eq!(cfg.allowed_user_id, Some(42));
        assert_eq!(cfg.download_dir, PathBuf::from("./custom_dl"));
        assert!(cfg.is_user_allowed(42));
        assert!(!cfg.is_user_allowed(99));
    }

    #[test]
    fn authorization_allows_all_when_unset_or_invalid() {
        let tmp = TempDir::new().expect("tempdir");
        let env_path = tmp.path().join(".env");
        fs::write(&env_path, "TELEGRAM_TOKEN=tok123\nALLOWED_USER_ID=abc\n").expect("write .env");

        let cfg = Config::load_from_paths(&[env_path], tmp.path().to_path_buf(), |_| None);
        assert_eq!(cfg.allowed_user_id, None);
        assert!(cfg.is_user_allowed(1));
        assert!(cfg.is_user_allowed(999_999));
    }
}
