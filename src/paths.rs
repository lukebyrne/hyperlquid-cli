use std::path::PathBuf;

use anyhow::{Result, anyhow};
use directories::UserDirs;

pub fn hl_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("HYPERLIQUID_CLI_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let home = UserDirs::new()
        .ok_or_else(|| anyhow!("Unable to determine home directory"))?
        .home_dir()
        .to_path_buf();
    Ok(home.join(".hl"))
}

pub fn db_path() -> Result<PathBuf> {
    Ok(hl_dir()?.join("hl.db"))
}

pub fn order_config_path() -> Result<PathBuf> {
    Ok(hl_dir()?.join("order-config.json"))
}

pub fn server_socket_path() -> Result<PathBuf> {
    Ok(hl_dir()?.join("server.sock"))
}

pub fn server_pid_path() -> Result<PathBuf> {
    Ok(hl_dir()?.join("server.pid"))
}

pub fn server_log_path() -> Result<PathBuf> {
    Ok(hl_dir()?.join("server.log"))
}

pub fn server_config_path() -> Result<PathBuf> {
    Ok(hl_dir()?.join("server.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_js_paths_defaults() {
        crate::test_support::without_hl_dir_env(|| {
            let home = UserDirs::new().unwrap().home_dir().to_path_buf();
            let expected = home.join(".hl");
            assert_eq!(hl_dir().unwrap(), expected);
            assert_eq!(db_path().unwrap(), expected.join("hl.db"));
            assert_eq!(
                order_config_path().unwrap(),
                expected.join("order-config.json")
            );
            assert_eq!(server_socket_path().unwrap(), expected.join("server.sock"));
            assert_eq!(server_pid_path().unwrap(), expected.join("server.pid"));
            assert_eq!(server_log_path().unwrap(), expected.join("server.log"));
            assert_eq!(server_config_path().unwrap(), expected.join("server.json"));
        });
    }
}
