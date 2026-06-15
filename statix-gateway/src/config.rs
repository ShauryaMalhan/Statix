//! Centralized gateway configuration from environment variables.

const DEFAULT_API_PORT: u16 = 3000;
const DEFAULT_CLICKHOUSE_URL: &str = "http://localhost:8123";
const DEFAULT_CLICKHOUSE_USER: &str = "default";

#[derive(Debug, Clone)]
pub struct Config {
    pub api_port: u16,
    pub api_token: Option<String>,
    pub expected_bearer: Option<String>,
    pub clickhouse_url: String,
    pub clickhouse_user: String,
    pub clickhouse_password: String,
}

impl Config {
    pub fn from_env() -> Self {
        let api_token = statix_infra::env::var("STATIX_API_TOKEN").filter(|s| !s.is_empty());
        let expected_bearer = api_token
            .as_ref()
            .map(|t| format!("Bearer {t}"));

        Self {
            api_port: env_api_port(),
            api_token,
            expected_bearer,
            clickhouse_url: env_string("CLICKHOUSE_URL", DEFAULT_CLICKHOUSE_URL),
            clickhouse_user: env_string("CLICKHOUSE_USER", DEFAULT_CLICKHOUSE_USER),
            clickhouse_password: std::env::var("CLICKHOUSE_PASSWORD").unwrap_or_default(),
        }
    }

    pub fn expected_bearer(&self) -> Option<&str> {
        self.expected_bearer.as_deref()
    }

    pub fn clickhouse_client(&self) -> clickhouse::Client {
        clickhouse::Client::default()
            .with_url(self.clickhouse_url.clone())
            .with_user(self.clickhouse_user.clone())
            .with_password(self.clickhouse_password.clone())
    }
}

fn env_string(name: &str, default: &str) -> String {
    match std::env::var(name) {
        Ok(s) if !s.is_empty() => s,
        Ok(_) => {
            eprintln!("WARN: {name} is empty; using default {default:?}");
            default.to_string()
        }
        Err(_) => default.to_string(),
    }
}

fn env_api_port() -> u16 {
    match statix_infra::env::var("STATIX_API_PORT") {
        None => DEFAULT_API_PORT,
        Some(s) => match s.parse::<u16>() {
            Ok(0) => {
                eprintln!("ERROR: STATIX_API_PORT must be 1..=65535, got 0");
                std::process::exit(1);
            }
            Ok(port) => port,
            Err(_) => {
                eprintln!("ERROR: invalid STATIX_API_PORT={s:?}: must be a valid u16");
                std::process::exit(1);
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(api_token: Option<String>) -> Config {
        Config {
            api_port: 3000,
            api_token: api_token.clone(),
            expected_bearer: api_token
                .as_ref()
                .map(|t| format!("Bearer {t}")),
            clickhouse_url: String::new(),
            clickhouse_user: String::new(),
            clickhouse_password: String::new(),
        }
    }

    #[test]
    fn expected_bearer_formats_token() {
        let cfg = test_config(Some("secret".into()));
        assert_eq!(cfg.expected_bearer(), Some("Bearer secret"));
        assert_eq!(cfg.expected_bearer.as_deref(), Some("Bearer secret"));
    }

    #[test]
    fn expected_bearer_none_without_token() {
        let cfg = test_config(None);
        assert!(cfg.expected_bearer().is_none());
        assert!(cfg.expected_bearer.is_none());
    }
}
