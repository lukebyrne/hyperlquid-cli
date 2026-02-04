use std::{
    env,
    ffi::{OsStr, OsString},
    path::Path,
    sync::{Mutex, MutexGuard},
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

pub struct EnvLockGuard {
    _guard: MutexGuard<'static, ()>,
}

impl EnvLockGuard {
    pub fn lock() -> EnvLockGuard {
        EnvLockGuard {
            _guard: ENV_LOCK.lock().expect("env lock poisoned"),
        }
    }
}

struct EnvVarGuard {
    key: &'static str,
    prev: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: Option<&OsStr>) -> EnvVarGuard {
        let prev = env::var_os(key);
        match value {
            Some(v) => unsafe { env::set_var(key, v) },
            None => unsafe { env::remove_var(key) },
        }
        EnvVarGuard { key, prev }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => unsafe { env::set_var(self.key, v) },
            None => unsafe { env::remove_var(self.key) },
        }
    }
}

pub fn without_hl_dir_env<T>(f: impl FnOnce() -> T) -> T {
    let _lock = EnvLockGuard::lock();
    let _guard = EnvVarGuard::set("HYPERLIQUID_CLI_DIR", None);
    f()
}

pub fn with_temp_hl_dir<T>(f: impl FnOnce(&Path) -> T) -> T {
    let _lock = EnvLockGuard::lock();
    let tmp = tempfile::tempdir().expect("create tempdir");
    let _guard = EnvVarGuard::set("HYPERLIQUID_CLI_DIR", Some(tmp.path().as_os_str()));
    f(tmp.path())
}

pub fn with_env_vars<T>(vars: &[(&'static str, Option<&OsStr>)], f: impl FnOnce() -> T) -> T {
    let _lock = EnvLockGuard::lock();
    let mut guards = Vec::with_capacity(vars.len());
    for (key, value) in vars {
        guards.push(EnvVarGuard::set(key, *value));
    }
    let result = f();
    drop(guards);
    result
}
