//! Environment variable parsing with safe defaults and warn-on-invalid semantics.

use std::str::FromStr;

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

/// Parse a strictly positive numeric env value, or return `default` on missing/invalid input.
///
/// Rejects values that fail to parse and values `<= T::default()` (e.g. `0` for unsigned types),
/// logging a warning before falling back.
fn read_env_positive<T>(name: &str, default: T) -> T
where
    T: Copy + Default + PartialOrd + std::fmt::Display,
    T: FromStr,
{
    match var_with_legacy(name) {
        Some(s) => match s.parse::<T>() {
            Ok(v) if v > T::default() => v,
            _ => {
                log::warn!("Invalid {name}={s:?}; using default {default}");
                default
            }
        },
        None => default,
    }
}

/// Read a positive `u64` from the environment, or return `default` on missing/invalid values.
pub fn read_env_u64(name: &str, default: u64) -> u64 {
    read_env_positive(name, default)
}

/// Read a positive `usize` from the environment, or return `default` on missing/invalid values.
pub fn read_env_usize(name: &str, default: usize) -> usize {
    read_env_positive(name, default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn read_env_u64_rejects_zero_and_negative_strings() {
        let _guard = env_lock();
        let key = "STATIX_TEST_U64_POSITIVE";
        unsafe { std::env::set_var(key, "0") };
        assert_eq!(read_env_u64(key, 42), 42);
        unsafe { std::env::set_var(key, "-1") };
        assert_eq!(read_env_u64(key, 42), 42);
        unsafe { std::env::set_var(key, "10") };
        assert_eq!(read_env_u64(key, 42), 10);
        unsafe { std::env::remove_var(key) };
    }

    #[test]
    fn read_env_usize_rejects_zero() {
        let _guard = env_lock();
        let key = "STATIX_TEST_USIZE_POSITIVE";
        unsafe { std::env::set_var(key, "0") };
        assert_eq!(read_env_usize(key, 99), 99);
        unsafe { std::env::set_var(key, "5") };
        assert_eq!(read_env_usize(key, 99), 5);
        unsafe { std::env::remove_var(key) };
    }
}
