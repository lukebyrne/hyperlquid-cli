use std::{collections::HashMap, time::Duration};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

use crate::paths;
use crate::{
    hl_api::{AllDexsAssetCtxsEvent, PerpMeta, SpotAssetCtx, SpotMeta},
    server::types::ServerStatus,
};

#[derive(Debug, Clone)]
pub struct Cached<T> {
    pub data: T,
    pub cached_at: i64,
}

#[derive(Debug, Serialize)]
struct RpcRequest {
    pub id: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    pub id: String,
    pub result: Option<serde_json::Value>,
    pub cached_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct ServerClient {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
    request_id: u64,
}

impl ServerClient {
    pub async fn connect() -> Result<ServerClient> {
        let socket_path = paths::server_socket_path()?;
        let stream = UnixStream::connect(&socket_path)
            .await
            .with_context(|| format!("Connect to {}", socket_path.display()))?;
        let (read, write) = stream.into_split();
        Ok(ServerClient {
            reader: BufReader::new(read),
            writer: write,
            request_id: 0,
        })
    }

    pub fn is_server_running() -> Result<bool> {
        let socket_path = paths::server_socket_path()?;
        Ok(socket_path.exists())
    }

    pub async fn try_connect() -> Result<Option<ServerClient>> {
        if !Self::is_server_running()? {
            return Ok(None);
        }
        match Self::connect().await {
            Ok(mut client) => {
                if client.get_status().await.is_ok() {
                    Ok(Some(client))
                } else {
                    Ok(None)
                }
            }
            Err(_) => Ok(None),
        }
    }

    async fn request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<RpcResponse> {
        self.request_id += 1;
        let id = self.request_id.to_string();

        let req = RpcRequest {
            id: id.clone(),
            method: method.to_string(),
            params,
        };
        let s = serde_json::to_string(&req)?;
        self.writer.write_all(s.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;

        let mut line = String::new();
        loop {
            line.clear();
            let read =
                tokio::time::timeout(Duration::from_secs(5), self.reader.read_line(&mut line))
                    .await
                    .context("Request timeout")??;
            if read == 0 {
                return Err(anyhow!("Connection closed"));
            }
            let resp: RpcResponse = serde_json::from_str(line.trim_end())
                .with_context(|| format!("Invalid server response: {}", line.trim_end()))?;
            if resp.id == id {
                if let Some(err) = &resp.error {
                    return Err(anyhow!("{err}"));
                }
                return Ok(resp);
            }
        }
    }

    pub async fn get_status(&mut self) -> Result<ServerStatus> {
        let resp = self.request("getStatus", None).await?;
        let value = resp.result.ok_or_else(|| anyhow!("Missing result"))?;
        serde_json::from_value(value).context("Invalid getStatus result")
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        let _ = self.request("shutdown", None).await?;
        Ok(())
    }

    pub async fn get_prices(
        &mut self,
        coin: Option<&str>,
    ) -> Result<Cached<HashMap<String, String>>> {
        let params = coin.map(|c| serde_json::json!({ "coin": c }));
        let resp = self.request("getPrices", params).await?;
        let cached_at = resp.cached_at.ok_or_else(|| anyhow!("Missing cached_at"))?;
        let value = resp.result.ok_or_else(|| anyhow!("Missing result"))?;
        let data: HashMap<String, String> =
            serde_json::from_value(value).context("Invalid getPrices result")?;
        Ok(Cached { data, cached_at })
    }

    pub async fn get_asset_ctxs(&mut self) -> Result<Cached<AllDexsAssetCtxsEvent>> {
        let resp = self.request("getAssetCtxs", None).await?;
        let cached_at = resp.cached_at.ok_or_else(|| anyhow!("Missing cached_at"))?;
        let value = resp.result.ok_or_else(|| anyhow!("Missing result"))?;
        let data: AllDexsAssetCtxsEvent =
            serde_json::from_value(value).context("Invalid getAssetCtxs result")?;
        Ok(Cached { data, cached_at })
    }

    pub async fn get_perp_meta(&mut self) -> Result<Cached<Vec<PerpMeta>>> {
        let resp = self.request("getPerpMeta", None).await?;
        let cached_at = resp.cached_at.ok_or_else(|| anyhow!("Missing cached_at"))?;
        let value = resp.result.ok_or_else(|| anyhow!("Missing result"))?;
        let data: Vec<PerpMeta> =
            serde_json::from_value(value).context("Invalid getPerpMeta result")?;
        Ok(Cached { data, cached_at })
    }

    pub async fn get_spot_meta(&mut self) -> Result<Cached<SpotMeta>> {
        let resp = self.request("getSpotMeta", None).await?;
        let cached_at = resp.cached_at.ok_or_else(|| anyhow!("Missing cached_at"))?;
        let value = resp.result.ok_or_else(|| anyhow!("Missing result"))?;
        let data: SpotMeta = serde_json::from_value(value).context("Invalid getSpotMeta result")?;
        Ok(Cached { data, cached_at })
    }

    pub async fn get_spot_asset_ctxs(&mut self) -> Result<Cached<Vec<SpotAssetCtx>>> {
        let resp = self.request("getSpotAssetCtxs", None).await?;
        let cached_at = resp.cached_at.ok_or_else(|| anyhow!("Missing cached_at"))?;
        let value = resp.result.ok_or_else(|| anyhow!("Missing result"))?;
        let data: Vec<SpotAssetCtx> =
            serde_json::from_value(value).context("Invalid getSpotAssetCtxs result")?;
        Ok(Cached { data, cached_at })
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    fn with_temp_hl_dir(f: impl FnOnce(PathBuf)) {
        let tmp = tempfile::tempdir().unwrap();
        crate::test_support::with_env_vars(
            &[("HYPERLIQUID_CLI_DIR", Some(tmp.path().as_os_str()))],
            || f(tmp.path().to_path_buf()),
        );
    }

    #[test]
    fn is_server_running_matches_socket_existence() {
        with_temp_hl_dir(|_| {
            assert!(!ServerClient::is_server_running().unwrap());

            let sock = crate::paths::server_socket_path().unwrap();
            fs::write(&sock, b"").unwrap();
            assert!(ServerClient::is_server_running().unwrap());
        });
    }

    #[test]
    fn try_connect_returns_none_when_socket_missing() {
        with_temp_hl_dir(|_| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let client = rt.block_on(ServerClient::try_connect()).unwrap();
            assert!(client.is_none());
        });
    }
}
