use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderConfig {
    pub slippage: f64,
}

impl Default for OrderConfig {
    fn default() -> Self {
        Self { slippage: 1.0 }
    }
}

fn config_path() -> Result<PathBuf> {
    paths::order_config_path()
}

pub fn load_order_config() -> OrderConfig {
    let path = match config_path() {
        Ok(p) => p,
        Err(_) => return OrderConfig::default(),
    };

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return OrderConfig::default(),
    };

    let parsed: OrderConfig = match serde_json::from_str(&content) {
        Ok(p) => p,
        Err(_) => return OrderConfig::default(),
    };

    OrderConfig {
        slippage: parsed.slippage,
    }
}

pub fn update_order_config(slippage: Option<f64>) -> Result<OrderConfig> {
    let path = config_path()?;
    let dir = path
        .parent()
        .map(ToOwned::to_owned)
        .context("Invalid order config path")?;

    fs::create_dir_all(&dir).with_context(|| format!("Failed to create dir: {}", dir.display()))?;

    let mut current = load_order_config();
    if let Some(s) = slippage {
        current.slippage = s;
    }

    fs::write(&path, serde_json::to_string_pretty(&current)?)
        .with_context(|| format!("Failed to write {}", path.display()))?;

    Ok(current)
}
