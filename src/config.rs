use std::str::FromStr;

use anyhow::{Result, anyhow};
use ethers::{
    signers::{LocalWallet, Signer},
    types::Address,
};

use crate::{db, validation};

#[derive(Clone, Debug)]
pub struct AccountSummary {
    pub alias: String,
    pub account_type: AccountType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountType {
    Readonly,
    ApiWallet,
}

#[derive(Clone, Debug)]
pub struct LoadedConfig {
    pub private_key: Option<String>,
    pub wallet_address: Option<Address>,
    pub testnet: bool,
    pub account: Option<AccountSummary>,
}

pub fn load_config(testnet: bool) -> Result<LoadedConfig> {
    // Try to load from default account in SQLite
    let default_account = db::get_default_account().ok().flatten();

    if let Some(acc) = default_account {
        let wallet_address = acc
            .user_address
            .parse::<Address>()
            .map_err(|_| anyhow!("Invalid address in db for account \"{}\"", acc.alias))?;

        let account_type = match acc.account_type.as_str() {
            "readonly" => AccountType::Readonly,
            "api_wallet" => AccountType::ApiWallet,
            other => {
                return Err(anyhow!(
                    "Invalid account type in db for account \"{}\": {}",
                    acc.alias,
                    other
                ));
            }
        };

        return Ok(LoadedConfig {
            private_key: acc.api_wallet_private_key,
            wallet_address: Some(wallet_address),
            testnet,
            account: Some(AccountSummary {
                alias: acc.alias,
                account_type,
            }),
        });
    }

    // Fall back to environment variables
    let private_key = std::env::var("HYPERLIQUID_PRIVATE_KEY").ok();
    let wallet_address_env = std::env::var("HYPERLIQUID_WALLET_ADDRESS").ok();

    let (private_key, wallet_address) = match (private_key, wallet_address_env) {
        (None, None) => (None, None),
        (None, Some(addr)) => (None, Some(validation::validate_address(&addr)?)),
        (Some(pk), Some(addr)) => (
            Some(validation::validate_private_key(&pk)?),
            Some(validation::validate_address(&addr)?),
        ),
        (Some(pk), None) => {
            let pk = validation::validate_private_key(&pk)?;
            let wallet = LocalWallet::from_str(&pk)
                .map_err(|_| anyhow!("Invalid HYPERLIQUID_PRIVATE_KEY format"))?;
            (Some(pk), Some(wallet.address()))
        }
    };

    Ok(LoadedConfig {
        private_key,
        wallet_address,
        testnet,
        account: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_js_load_config_testnet_flag() {
        let tmp = tempfile::tempdir().unwrap();
        crate::test_support::with_env_vars(
            &[
                ("HYPERLIQUID_CLI_DIR", Some(tmp.path().as_os_str())),
                ("HYPERLIQUID_PRIVATE_KEY", None),
                ("HYPERLIQUID_WALLET_ADDRESS", None),
            ],
            || {
                let cfg = load_config(false).unwrap();
                assert!(!cfg.testnet);

                let cfg = load_config(true).unwrap();
                assert!(cfg.testnet);
            },
        );
    }
}
