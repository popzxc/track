use std::ffi::OsString;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

// Tests that redirect `TRACK_DATA_DIR` share a single process-global lock so
// unrelated cases do not race on the same environment variable when Rust runs
// unit tests in parallel.
pub(crate) fn track_data_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) struct EnvVarGuard {
    key: &'static str,
    previous_value: Option<OsString>,
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.previous_value.take() {
            Some(previous_value) => std::env::set_var(self.key, previous_value),
            None => std::env::remove_var(self.key),
        }
    }
}

pub(crate) fn set_env_var(key: &'static str, value: &Path) -> EnvVarGuard {
    let previous_value = std::env::var_os(key);
    std::env::set_var(key, value);

    EnvVarGuard {
        key,
        previous_value,
    }
}
