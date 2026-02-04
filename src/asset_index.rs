use anyhow::{Result, anyhow};

use crate::hl_api::{PerpMeta, SpotMeta};

/// Resolve Hyperliquid asset index for a given `coin`.
///
/// Logic matches the upstream JS CLI:
/// - Main perps (dexIndex 0): `marketIndex`
/// - Builder perps (dexIndex 1+): `100000 + dexIndex * 10000 + marketIndex`
/// - Spot: `10000 + spotIndex`
pub fn resolve_asset_index(
    all_perp_metas: &[PerpMeta],
    spot_meta: &SpotMeta,
    coin: &str,
) -> Result<u32> {
    for (dex_index, dex) in all_perp_metas.iter().enumerate() {
        if let Some(market_index) = dex.universe.iter().position(|a| a.name == coin) {
            let market_index_u32: u32 = market_index
                .try_into()
                .map_err(|_| anyhow!("Market index overflow"))?;
            let dex_index_u32: u32 = dex_index
                .try_into()
                .map_err(|_| anyhow!("Dex index overflow"))?;
            return Ok(if dex_index_u32 == 0 {
                market_index_u32
            } else {
                100_000 + dex_index_u32 * 10_000 + market_index_u32
            });
        }
    }

    if let Some(spot_index) = spot_meta.universe.iter().position(|a| a.name == coin) {
        let spot_index_u32: u32 = spot_index
            .try_into()
            .map_err(|_| anyhow!("Spot index overflow"))?;
        return Ok(10_000 + spot_index_u32);
    }

    Err(anyhow!("Unknown coin: {coin}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hl_api::{PerpAssetMeta, SpotToken, SpotUniverse};

    fn perp_meta(coins: &[&str]) -> PerpMeta {
        PerpMeta {
            universe: coins
                .iter()
                .map(|c| PerpAssetMeta {
                    sz_decimals: 2,
                    name: (*c).to_string(),
                    max_leverage: 50,
                    only_isolated: false,
                    is_delisted: false,
                })
                .collect(),
            collateral_token: 0,
            margin_tables: serde_json::Value::Null,
        }
    }

    fn spot_meta(pairs: &[&str]) -> SpotMeta {
        SpotMeta {
            tokens: vec![
                SpotToken {
                    name: "USDC".to_string(),
                    sz_decimals: 2,
                    wei_decimals: 6,
                    index: 0,
                    token_id: "0".to_string(),
                    is_canonical: true,
                },
                SpotToken {
                    name: "PURR".to_string(),
                    sz_decimals: 2,
                    wei_decimals: 6,
                    index: 1,
                    token_id: "1".to_string(),
                    is_canonical: true,
                },
            ],
            universe: pairs
                .iter()
                .enumerate()
                .map(|(i, p)| SpotUniverse {
                    tokens: vec![1, 0],
                    name: (*p).to_string(),
                    index: i as u32,
                    is_canonical: true,
                })
                .collect(),
        }
    }

    #[test]
    fn resolves_main_perp_index() {
        let perps = vec![perp_meta(&["BTC", "ETH"])];
        let spot = spot_meta(&[]);
        let idx = resolve_asset_index(&perps, &spot, "ETH").unwrap();
        assert_eq!(idx, 1);
    }

    #[test]
    fn resolves_builder_perp_index() {
        let perps = vec![
            perp_meta(&["BTC"]),
            perp_meta(&["DEX:AAA", "DEX:BBB", "DEX:CCC"]),
        ];
        let spot = spot_meta(&[]);
        let idx = resolve_asset_index(&perps, &spot, "DEX:CCC").unwrap();
        assert_eq!(idx, 110_002);
    }

    #[test]
    fn resolves_spot_index() {
        let perps = vec![perp_meta(&[])];
        let spot = spot_meta(&["PURR/USDC", "ABC/USDC"]);
        let idx = resolve_asset_index(&perps, &spot, "ABC/USDC").unwrap();
        assert_eq!(idx, 10_001);
    }

    #[test]
    fn unknown_coin_errors() {
        let perps = vec![perp_meta(&["BTC"])];
        let spot = spot_meta(&["PURR/USDC"]);
        let err = resolve_asset_index(&perps, &spot, "NOPE").unwrap_err();
        assert!(err.to_string().contains("Unknown coin"));
    }
}
