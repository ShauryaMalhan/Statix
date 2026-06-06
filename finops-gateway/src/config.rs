//! Centralized gateway configuration from environment variables.

const DEFAULT_KAFKA_BROKERS: &str = "localhost:9092";
const DEFAULT_API_PORT: u16 = 3000;
const DEFAULT_CLICKHOUSE_URL: &str = "http://localhost:8123";
const DEFAULT_CLICKHOUSE_USER: &str = "default";

/// Strongly typed `finops-gateway` configuration loaded once at startup.
#[derive(Debug, Clone)]
pub struct Config {
    pub kafka_brokers: String,
    pub api_port: u16,
    pub api_token: Option<String>,
    pub clickhouse_url: String,
    pub clickhouse_user: String,
    pub clickhouse_password: String,
}

impl Config {
    /// Load configuration from the process environment (defaults when unset).
    pub fn from_env() -> Self {
        Self {
            kafka_brokers: env_string("KAFKA_BROKERS", DEFAULT_KAFKA_BROKERS),
            api_port: env_api_port(),
            api_token: std::env::var("FINOPS_API_TOKEN")
                .ok()
                .filter(|s| !s.is_empty()),
            clickhouse_url: env_string("CLICKHOUSE_URL", DEFAULT_CLICKHOUSE_URL),
            clickhouse_user: env_string("CLICKHOUSE_USER", DEFAULT_CLICKHOUSE_USER),
            clickhouse_password: std::env::var("CLICKHOUSE_PASSWORD").unwrap_or_default(),
        }
    }

    /// Full `Authorization` header value for `POST /ingest` when `api_token` is set.
    pub fn expected_bearer(&self) -> Option<String> {
        self.api_token.as_ref().map(|t| format!("Bearer {t}"))
    }

    /// ClickHouse HTTP client for the read path.
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
    match std::env::var("FINOPS_API_PORT") {
        Err(_) => DEFAULT_API_PORT,
        Ok(s) => match s.parse::<u16>() {
            Ok(0) => {
                eprintln!("ERROR: FINOPS_API_PORT must be 1..=65535, got 0");
                std::process::exit(1);
            }
            Ok(port) => port,
            Err(_) => {
                eprintln!("ERROR: invalid FINOPS_API_PORT={s:?}: must be a valid u16");
                std::process::exit(1);
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_bearer_formats_token() {
        let cfg = Config {
            kafka_brokers: String::new(),
            api_port: 3000,
            api_token: Some("secret".into()),
            clickhouse_url: String::new(),
            clickhouse_user: String::new(),
            clickhouse_password: String::new(),
        };
        assert_eq!(cfg.expected_bearer().as_deref(), Some("Bearer secret"));
    }

    #[test]
    fn expected_bearer_none_without_token() {
        let cfg = Config {
            kafka_brokers: String::new(),
            api_port: 3000,
            api_token: None,
            clickhouse_url: String::new(),
            clickhouse_user: String::new(),
            clickhouse_password: String::new(),
        };
        assert!(cfg.expected_bearer().is_none());
    }
}
