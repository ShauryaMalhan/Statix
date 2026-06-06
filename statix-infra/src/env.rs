//! Environment variable parsing with safe defaults and warn-on-invalid semantics.

fn var_with_legacy(name: &str) -> Option<String> {
    std::env::var(name).ok().or_else(|| {
        name.strip_prefix("STATIX_")
            .and_then(|legacy| std::env::var(format!("FINOPS_{legacy}")).ok())
    })
}

/// Read an environment variable, accepting legacy `FINOPS_*` names when `name` is `STATIX_*`.
pub fn var(name: &str) -> Option<String> {
    var_with_legacy(name)
}

/// Read a positive `u64` from the environment, or return `default` on missing/invalid values.
pub fn read_env_u64(name: &str, default: u64) -> u64 {
    match var_with_legacy(name) {
        Some(s) => match s.parse::<u64>() {
            Ok(v) if v > 0 => v,
            _ => {
                log::warn!("Invalid {name}={s:?}; using default {default}");
                default
            }
        },
        None => default,
    }
}

/// Read a positive `usize` from the environment, or return `default` on missing/invalid values.
pub fn read_env_usize(name: &str, default: usize) -> usize {
    match var_with_legacy(name) {
        Some(s) => match s.parse::<usize>() {
            Ok(v) if v > 0 => v,
            _ => {
                log::warn!("Invalid {name}={s:?}; using default {default}");
                default
            }
        },
        None => default,
    }
}
