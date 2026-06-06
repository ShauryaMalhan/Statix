//! Environment variable parsing with safe defaults and warn-on-invalid semantics.

/// Read a positive `u64` from the environment, or return `default` on missing/invalid values.
pub fn read_env_u64(name: &str, default: u64) -> u64 {
    match std::env::var(name) {
        Ok(s) => match s.parse::<u64>() {
            Ok(v) if v > 0 => v,
            _ => {
                log::warn!("Invalid {name}={s:?}; using default {default}");
                default
            }
        },
        Err(_) => default,
    }
}

/// Read a positive `usize` from the environment, or return `default` on missing/invalid values.
pub fn read_env_usize(name: &str, default: usize) -> usize {
    match std::env::var(name) {
        Ok(s) => match s.parse::<usize>() {
            Ok(v) if v > 0 => v,
            _ => {
                log::warn!("Invalid {name}={s:?}; using default {default}");
                default
            }
        },
        Err(_) => default,
    }
}
