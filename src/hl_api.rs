use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use ethers::types::Address;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

fn base_url(testnet: bool) -> &'static str {
    if testnet {
        "https://api.hyperliquid-testnet.xyz"
    } else {
        "https://api.hyperliquid.xyz"
    }
}

fn addr(addr: Address) -> String {
    format!("{addr:#x}")
}

#[derive(Clone)]
pub struct HlApi {
    pub testnet: bool,
    client: reqwest::Client,
    base_url: String,
}

impl HlApi {
    pub fn new(testnet: bool) -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent("hyperliquid-cli-rs")
            .build()?;
        Ok(Self {
            testnet,
            client,
            base_url: base_url(testnet).to_string(),
        })
    }

    pub fn info_url(&self) -> String {
        format!("{}/info", self.base_url)
    }

    async fn post_info<T: DeserializeOwned>(&self, body: serde_json::Value) -> Result<T> {
        let url = self.info_url();
        let res = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(anyhow!("Hyperliquid API error ({status}): {text}"));
        }

        res.json::<T>().await.context("Invalid JSON response")
    }

    pub async fn all_mids(&self) -> Result<HashMap<String, String>> {
        self.post_info(serde_json::json!({ "type": "allMids" }))
            .await
    }

    pub async fn meta(&self) -> Result<PerpMeta> {
        self.post_info(serde_json::json!({ "type": "meta" })).await
    }

    pub async fn all_perp_metas(&self) -> Result<Vec<PerpMeta>> {
        self.post_info(serde_json::json!({ "type": "allPerpMetas" }))
            .await
    }

    pub async fn spot_meta(&self) -> Result<SpotMeta> {
        self.post_info(serde_json::json!({ "type": "spotMeta" }))
            .await
    }

    pub async fn meta_and_asset_ctxs(&self) -> Result<(PerpMeta, Vec<PerpAssetCtx>)> {
        self.meta_and_asset_ctxs_for_dex(None).await
    }

    pub async fn meta_and_asset_ctxs_for_dex(
        &self,
        dex: Option<&str>,
    ) -> Result<(PerpMeta, Vec<PerpAssetCtx>)> {
        let mut body = serde_json::json!({ "type": "metaAndAssetCtxs" });
        if let Some(dex) = dex {
            body["dex"] = serde_json::Value::String(dex.to_string());
        }

        let items: Vec<PerpMetaAndAssetCtxsItem> = self.post_info(body).await?;

        let mut meta = None;
        let mut ctxs = None;
        for item in items {
            match item {
                PerpMetaAndAssetCtxsItem::Meta(m) => meta = Some(m),
                PerpMetaAndAssetCtxsItem::Ctx(c) => ctxs = Some(c),
            }
        }
        Ok((
            meta.ok_or_else(|| anyhow!("metaAndAssetCtxs missing meta"))?,
            ctxs.ok_or_else(|| anyhow!("metaAndAssetCtxs missing ctxs"))?,
        ))
    }

    pub async fn spot_meta_and_asset_ctxs(&self) -> Result<(SpotMeta, Vec<SpotAssetCtx>)> {
        let items: Vec<SpotMetaAndAssetCtxsItem> = self
            .post_info(serde_json::json!({ "type": "spotMetaAndAssetCtxs" }))
            .await?;

        let mut meta = None;
        let mut ctxs = None;
        for item in items {
            match item {
                SpotMetaAndAssetCtxsItem::Meta(m) => meta = Some(m),
                SpotMetaAndAssetCtxsItem::Ctx(c) => ctxs = Some(c),
            }
        }
        Ok((
            meta.ok_or_else(|| anyhow!("spotMetaAndAssetCtxs missing meta"))?,
            ctxs.ok_or_else(|| anyhow!("spotMetaAndAssetCtxs missing ctxs"))?,
        ))
    }

    pub async fn open_orders(&self, user: Address) -> Result<Vec<OpenOrder>> {
        self.post_info(serde_json::json!({
            "type": "openOrders",
            "user": addr(user),
            "dex": "ALL_DEXS"
        }))
        .await
    }

    pub async fn clearinghouse_state(&self, user: Address) -> Result<ClearinghouseState> {
        self.post_info(serde_json::json!({
            "type": "clearinghouseState",
            "user": addr(user)
        }))
        .await
    }

    pub async fn spot_clearinghouse_state(&self, user: Address) -> Result<SpotClearinghouseState> {
        self.post_info(serde_json::json!({
            "type": "spotClearinghouseState",
            "user": addr(user)
        }))
        .await
    }

    pub async fn l2_book(&self, coin: &str) -> Result<L2Book> {
        self.post_info(serde_json::json!({
            "type": "l2Book",
            "coin": coin
        }))
        .await
    }

    pub async fn active_asset_data(&self, user: Address, coin: &str) -> Result<ActiveAssetData> {
        self.post_info(serde_json::json!({
            "type": "activeAssetData",
            "user": addr(user),
            "coin": coin
        }))
        .await
    }

    pub async fn user_role(&self, user: Address) -> Result<UserRoleResponse> {
        self.post_info(serde_json::json!({
            "type": "userRole",
            "user": addr(user)
        }))
        .await
    }

    pub async fn extra_agents(&self, user: Address) -> Result<Vec<ExtraAgent>> {
        self.post_info(serde_json::json!({
            "type": "extraAgents",
            "user": addr(user)
        }))
        .await
    }

    pub async fn referral(&self, user: Address) -> Result<serde_json::Value> {
        self.post_info(serde_json::json!({
            "type": "referral",
            "user": addr(user)
        }))
        .await
    }

    pub async fn check_health(&self) -> Result<()> {
        // A cheap call to validate connectivity.
        let url = self.info_url();
        let res = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "type": "allMids" }))
            .send()
            .await?;
        match res.status() {
            StatusCode::OK => Ok(()),
            other => Err(anyhow!("Hyperliquid API health check failed ({other})")),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerpMeta {
    pub universe: Vec<PerpAssetMeta>,
    #[serde(default)]
    pub collateral_token: u64,
    #[serde(default)]
    pub margin_tables: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerpAssetMeta {
    pub sz_decimals: u32,
    pub name: String,
    pub max_leverage: u32,
    #[serde(default)]
    pub only_isolated: bool,
    #[serde(default)]
    pub is_delisted: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerpAssetCtx {
    pub funding: String,
    pub open_interest: String,
    pub prev_day_px: String,
    pub day_ntl_vlm: String,
    #[serde(default)]
    pub premium: Option<String>,
    pub oracle_px: String,
    pub mark_px: String,
    #[serde(default)]
    pub mid_px: Option<String>,
    #[serde(default)]
    pub impact_pxs: Option<serde_json::Value>,
    #[serde(default)]
    pub day_base_vlm: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AllDexsAssetCtxsEvent {
    pub ctxs: Vec<(String, Vec<PerpAssetCtx>)>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum PerpMetaAndAssetCtxsItem {
    Meta(PerpMeta),
    Ctx(Vec<PerpAssetCtx>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotMeta {
    pub tokens: Vec<SpotToken>,
    pub universe: Vec<SpotUniverse>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotToken {
    pub name: String,
    pub sz_decimals: u32,
    pub wei_decimals: u32,
    pub index: u32,
    pub token_id: String,
    #[serde(default)]
    pub is_canonical: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotUniverse {
    pub tokens: Vec<u32>,
    pub name: String,
    pub index: u32,
    #[serde(default)]
    pub is_canonical: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotAssetCtx {
    pub circulating_supply: String,
    pub coin: String,
    pub day_ntl_vlm: String,
    pub mark_px: String,
    #[serde(default)]
    pub mid_px: Option<String>,
    pub prev_day_px: String,
    #[serde(default)]
    pub total_supply: Option<String>,
    #[serde(default)]
    pub day_base_vlm: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum SpotMetaAndAssetCtxsItem {
    Meta(SpotMeta),
    Ctx(Vec<SpotAssetCtx>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenOrder {
    pub coin: String,
    pub limit_px: String,
    pub oid: u64,
    pub side: String,
    pub sz: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearinghouseState {
    pub asset_positions: Vec<AssetPosition>,
    pub margin_summary: MarginSummary,
    pub cross_margin_summary: MarginSummary,
    #[serde(default)]
    pub withdrawable: String,
    #[serde(default)]
    pub time: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPosition {
    pub position: Position,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub coin: String,
    pub szi: String,
    #[serde(default)]
    pub entry_px: Option<String>,
    pub position_value: String,
    pub unrealized_pnl: String,
    pub leverage: Leverage,
    #[serde(default)]
    pub liquidation_px: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Leverage {
    #[serde(rename = "type")]
    pub leverage_type: String,
    pub value: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarginSummary {
    pub account_value: String,
    pub total_margin_used: String,
    pub total_ntl_pos: String,
    pub total_raw_usd: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SpotClearinghouseState {
    pub balances: Vec<SpotBalance>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SpotBalance {
    pub coin: String,
    pub hold: String,
    pub total: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct L2Book {
    pub coin: String,
    pub levels: Vec<Vec<BookLevel>>,
    pub time: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BookLevel {
    pub px: String,
    pub sz: String,
    pub n: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAssetData {
    pub user: String,
    pub coin: String,
    pub leverage: Leverage,
    pub max_trade_szs: [String; 2],
    pub available_to_trade: [String; 2],
    pub mark_px: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "role", rename_all = "camelCase")]
pub enum UserRoleResponse {
    #[serde(rename = "missing")]
    Missing,
    #[serde(rename = "user")]
    User,
    #[serde(rename = "vault")]
    Vault,
    #[serde(rename = "agent")]
    Agent { data: AgentRoleData },
    #[serde(rename = "subAccount")]
    SubAccount { data: SubAccountRoleData },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentRoleData {
    pub user: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubAccountRoleData {
    pub master: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtraAgent {
    pub address: String,
    pub name: String,
    pub valid_until: u64,
}
