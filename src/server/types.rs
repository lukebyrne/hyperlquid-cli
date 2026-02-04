use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStatus {
    #[serde(rename = "hasMids")]
    pub has_mids: bool,
    #[serde(rename = "hasAssetCtxs")]
    pub has_asset_ctxs: bool,
    #[serde(rename = "hasPerpMetas")]
    pub has_perp_metas: bool,
    #[serde(rename = "hasSpotMeta")]
    pub has_spot_meta: bool,
    #[serde(rename = "hasSpotAssetCtxs")]
    pub has_spot_asset_ctxs: bool,
    #[serde(rename = "midsAge")]
    pub mids_age: Option<i64>,
    #[serde(rename = "assetCtxsAge")]
    pub asset_ctxs_age: Option<i64>,
    #[serde(rename = "perpMetasAge")]
    pub perp_metas_age: Option<i64>,
    #[serde(rename = "spotMetaAge")]
    pub spot_meta_age: Option<i64>,
    #[serde(rename = "spotAssetCtxsAge")]
    pub spot_asset_ctxs_age: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerStatus {
    pub running: bool,
    pub testnet: bool,
    pub connected: bool,
    #[serde(rename = "startedAt")]
    pub started_at: i64,
    pub uptime: i64,
    pub cache: CacheStatus,
}
