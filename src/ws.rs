use anyhow::{Context, Result, anyhow};
use ethers::types::Address;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn ws_endpoint(testnet: bool) -> &'static str {
    if testnet {
        "wss://api.hyperliquid-testnet.xyz/ws"
    } else {
        "wss://api.hyperliquid.xyz/ws"
    }
}

fn addr(user: Address) -> String {
    format!("{user:#x}")
}

pub fn sub_all_mids_all_dexs() -> serde_json::Value {
    serde_json::json!({ "type": "allMids", "dex": "ALL_DEXS" })
}

pub fn sub_l2_book(coin: &str) -> serde_json::Value {
    serde_json::json!({ "type": "l2Book", "coin": coin })
}

pub fn sub_order_updates(user: Address) -> serde_json::Value {
    serde_json::json!({ "type": "orderUpdates", "user": addr(user) })
}

pub fn sub_active_asset_data(user: Address, coin: &str) -> serde_json::Value {
    serde_json::json!({ "type": "activeAssetData", "user": addr(user), "coin": coin })
}

pub fn sub_all_dexs_clearinghouse_state(user: Address) -> serde_json::Value {
    serde_json::json!({ "type": "allDexsClearinghouseState", "user": addr(user) })
}

pub struct WsClient {
    stream: WsStream,
}

impl WsClient {
    pub async fn connect(testnet: bool) -> Result<Self> {
        let url = ws_endpoint(testnet);
        let (stream, _) = connect_async(url)
            .await
            .with_context(|| format!("WebSocket connect failed: {url}"))?;
        Ok(Self { stream })
    }

    pub async fn subscribe(&mut self, subscription: serde_json::Value) -> Result<()> {
        let req = serde_json::json!({
            "method": "subscribe",
            "subscription": subscription,
        });
        let text = serde_json::to_string(&req).context("Serialize WebSocket subscribe request")?;
        self.stream
            .send(Message::Text(text))
            .await
            .context("WebSocket send subscribe request")?;
        Ok(())
    }

    pub async fn next_json(&mut self) -> Result<Option<serde_json::Value>> {
        loop {
            let Some(msg) = self
                .stream
                .next()
                .await
                .transpose()
                .context("WebSocket read failed")?
            else {
                return Ok(None);
            };

            match msg {
                Message::Text(text) => {
                    let v = serde_json::from_str::<serde_json::Value>(&text)
                        .context("Invalid WebSocket JSON message")?;
                    return Ok(Some(v));
                }
                Message::Binary(_) => continue,
                Message::Ping(payload) => {
                    self.stream
                        .send(Message::Pong(payload))
                        .await
                        .context("WebSocket pong failed")?;
                }
                Message::Pong(_) => {}
                Message::Close(_) => return Ok(None),
                _ => {}
            }
        }
    }

    pub fn channel<'a>(msg: &'a serde_json::Value) -> Result<&'a str> {
        msg.get("channel")
            .and_then(|c| c.as_str())
            .ok_or_else(|| anyhow!("Missing channel in WebSocket message"))
    }

    pub fn data<'a>(msg: &'a serde_json::Value) -> Result<&'a serde_json::Value> {
        msg.get("data")
            .ok_or_else(|| anyhow!("Missing data in WebSocket message"))
    }
}
