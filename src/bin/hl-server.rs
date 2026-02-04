use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use clap::Parser;
use daemonize::Daemonize;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{RwLock, watch},
};

use hyperliquid_cli::{
    hl_api::{AllDexsAssetCtxsEvent, HlApi, PerpMeta, SpotAssetCtx, SpotMeta},
    paths,
    server::types::{CacheStatus, ServerStatus},
};

#[derive(Parser, Debug)]
#[command(
    name = "rhl-server",
    about = "Background server for hyperliquid-cli",
    version
)]
struct Args {
    /// Use testnet instead of mainnet
    #[arg(long, default_value_t = false)]
    testnet: bool,

    /// Detach and run in the background
    #[arg(long, default_value_t = false)]
    daemonize: bool,
}

#[derive(Clone)]
struct Logger {
    path: PathBuf,
}

impl Logger {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn log(&self, message: impl AsRef<str>) {
        let ts = chrono::Utc::now().to_rfc3339();
        let line = format!("[{ts}] {}\n", message.as_ref());
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis() as i64
}

#[derive(Clone)]
struct Clock {
    now_ms: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl Clock {
    fn system() -> Clock {
        Clock {
            now_ms: Arc::new(now_ms),
        }
    }

    fn now_ms(&self) -> i64 {
        (self.now_ms)()
    }
}

#[derive(Clone)]
struct CacheEntry<T> {
    data: T,
    updated_at: i64,
}

struct ServerCache {
    clock: Clock,
    mids: Option<CacheEntry<HashMap<String, String>>>,
    asset_ctxs: Option<CacheEntry<AllDexsAssetCtxsEvent>>,
    perp_metas: Option<CacheEntry<Vec<PerpMeta>>>,
    spot_meta: Option<CacheEntry<SpotMeta>>,
    spot_ctxs: Option<CacheEntry<Vec<SpotAssetCtx>>>,
}

impl ServerCache {
    fn new(clock: Clock) -> ServerCache {
        ServerCache {
            clock,
            mids: None,
            asset_ctxs: None,
            perp_metas: None,
            spot_meta: None,
            spot_ctxs: None,
        }
    }

    fn set_mids(&mut self, data: HashMap<String, String>) {
        self.mids = Some(CacheEntry {
            data,
            updated_at: self.clock.now_ms(),
        });
    }

    fn set_asset_ctxs(&mut self, data: AllDexsAssetCtxsEvent) {
        self.asset_ctxs = Some(CacheEntry {
            data,
            updated_at: self.clock.now_ms(),
        });
    }

    fn set_perp_metas(&mut self, data: Vec<PerpMeta>) {
        self.perp_metas = Some(CacheEntry {
            data,
            updated_at: self.clock.now_ms(),
        });
    }

    fn set_spot_meta(&mut self, data: SpotMeta) {
        self.spot_meta = Some(CacheEntry {
            data,
            updated_at: self.clock.now_ms(),
        });
    }

    fn set_spot_ctxs(&mut self, data: Vec<SpotAssetCtx>) {
        self.spot_ctxs = Some(CacheEntry {
            data,
            updated_at: self.clock.now_ms(),
        });
    }

    fn status(&self) -> CacheStatus {
        let now = self.clock.now_ms();
        CacheStatus {
            has_mids: self.mids.is_some(),
            has_asset_ctxs: self.asset_ctxs.is_some(),
            has_perp_metas: self.perp_metas.is_some(),
            has_spot_meta: self.spot_meta.is_some(),
            has_spot_asset_ctxs: self.spot_ctxs.is_some(),
            mids_age: self.mids.as_ref().map(|e| now - e.updated_at),
            asset_ctxs_age: self.asset_ctxs.as_ref().map(|e| now - e.updated_at),
            perp_metas_age: self.perp_metas.as_ref().map(|e| now - e.updated_at),
            spot_meta_age: self.spot_meta.as_ref().map(|e| now - e.updated_at),
            spot_asset_ctxs_age: self.spot_ctxs.as_ref().map(|e| now - e.updated_at),
        }
    }

    fn is_connected(&self) -> bool {
        self.mids
            .as_ref()
            .map(|e| self.clock.now_ms() - e.updated_at < 5_000)
            .unwrap_or(false)
    }
}

impl Default for ServerCache {
    fn default() -> ServerCache {
        ServerCache::new(Clock::system())
    }
}

#[derive(serde::Deserialize)]
struct RpcRequest {
    id: String,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(serde::Serialize)]
struct RpcResponse {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cached_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn response_ok(id: String, result: serde_json::Value, cached_at: Option<i64>) -> RpcResponse {
    RpcResponse {
        id,
        result: Some(result),
        cached_at,
        error: None,
    }
}

fn response_err(id: String, error: impl Into<String>) -> RpcResponse {
    RpcResponse {
        id,
        result: None,
        cached_at: None,
        error: Some(error.into()),
    }
}

fn ensure_hl_dir() -> Result<PathBuf> {
    let dir = paths::hl_dir()?;
    fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    Ok(dir)
}

fn cleanup_files(logger: &Logger) {
    for path in [
        paths::server_pid_path(),
        paths::server_socket_path(),
        paths::server_config_path(),
    ]
    .into_iter()
    .flatten()
    {
        if path.exists()
            && let Err(e) = fs::remove_file(&path)
        {
            logger.log(format!("Failed to remove {}: {e}", path.display()));
        }
    }
}

fn daemonize_if_needed(args: &Args, logger: &Logger) -> Result<()> {
    if !args.daemonize {
        return Ok(());
    }

    let hl_dir = ensure_hl_dir()?;
    let log_path = paths::server_log_path()?;
    let stdout = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("Open log {}", log_path.display()))?;
    let stderr = stdout
        .try_clone()
        .with_context(|| format!("Clone log {}", log_path.display()))?;

    logger.log("Daemonizing...");
    Daemonize::new()
        .working_directory(hl_dir)
        .stdout(stdout)
        .stderr(stderr)
        .start()
        .context("Failed to daemonize")?;
    Ok(())
}

fn write_pid_and_config(testnet: bool, started_at: i64, logger: &Logger) -> Result<()> {
    let pid_path = paths::server_pid_path()?;
    fs::write(&pid_path, std::process::id().to_string())
        .with_context(|| format!("Write {}", pid_path.display()))?;

    let config_path = paths::server_config_path()?;
    let config = serde_json::json!({ "testnet": testnet, "startedAt": started_at });
    fs::write(&config_path, serde_json::to_string_pretty(&config)?)
        .with_context(|| format!("Write {}", config_path.display()))?;

    logger.log(format!(
        "Wrote pid={} and config (testnet: {testnet})",
        std::process::id()
    ));
    Ok(())
}

fn remove_existing_socket(logger: &Logger) -> Result<()> {
    let socket_path = paths::server_socket_path()?;
    if socket_path.exists() {
        fs::remove_file(&socket_path)
            .with_context(|| format!("Remove stale socket {}", socket_path.display()))?;
        logger.log(format!("Removed stale socket {}", socket_path.display()));
    }
    Ok(())
}

fn main() {
    if let Err(err) = real_main() {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let args = Args::parse();
    ensure_hl_dir()?;

    let logger = Logger::new(paths::server_log_path()?);
    logger.log(format!(
        "Starting rhl-server (testnet: {}, pid: {})",
        args.testnet,
        std::process::id()
    ));

    remove_existing_socket(&logger)?;
    daemonize_if_needed(&args, &logger)?;

    let started_at = now_ms();
    write_pid_and_config(args.testnet, started_at, &logger)?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("Build tokio runtime")?;

    let result = rt.block_on(async move { server_main(args.testnet, started_at, logger).await });
    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
    Ok(())
}

async fn server_main(testnet: bool, started_at: i64, logger: Logger) -> Result<()> {
    let api = HlApi::new(testnet)?;

    let cache = Arc::new(RwLock::new(ServerCache::default()));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    spawn_pollers(
        api.clone(),
        cache.clone(),
        shutdown_rx.clone(),
        logger.clone(),
    );

    let socket_path = paths::server_socket_path()?;
    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("Bind {}", socket_path.display()))?;
    logger.log(format!("IPC listening on {}", socket_path.display()));

    let mut shutdown_rx_accept = shutdown_rx.clone();

    #[cfg(unix)]
    let mut sigterm = {
        use tokio::signal::unix::{SignalKind, signal};
        signal(SignalKind::terminate()).context("Register SIGTERM handler")?
    };

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    #[cfg(unix)]
    loop {
        tokio::select! {
            _ = shutdown_rx_accept.changed() => {
                if *shutdown_rx_accept.borrow() {
                    break;
                }
            }
            _ = &mut ctrl_c => {
                logger.log("Shutdown: SIGINT");
                let _ = shutdown_tx.send(true);
                break;
            }
            _ = sigterm.recv() => {
                logger.log("Shutdown: SIGTERM");
                let _ = shutdown_tx.send(true);
                break;
            }
            accept_res = listener.accept() => {
                let (stream, _) = accept_res?;
                let cache = cache.clone();
                let shutdown_tx = shutdown_tx.clone();
                let logger = logger.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, cache, shutdown_tx, testnet, started_at, logger).await {
                        // Most errors here are client disconnects; keep log light.
                        let _ = e;
                    }
                });
            }
        }
    }

    #[cfg(not(unix))]
    loop {
        tokio::select! {
            _ = shutdown_rx_accept.changed() => {
                if *shutdown_rx_accept.borrow() {
                    break;
                }
            }
            _ = &mut ctrl_c => {
                logger.log("Shutdown: SIGINT");
                let _ = shutdown_tx.send(true);
                break;
            }
            accept_res = listener.accept() => {
                let (stream, _) = accept_res?;
                let cache = cache.clone();
                let shutdown_tx = shutdown_tx.clone();
                let logger = logger.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, cache, shutdown_tx, testnet, started_at, logger).await {
                        let _ = e;
                    }
                });
            }
        }
    }

    logger.log("Server stopping...");
    cleanup_files(&logger);
    Ok(())
}

fn spawn_pollers(
    api: HlApi,
    cache: Arc<RwLock<ServerCache>>,
    shutdown_rx: watch::Receiver<bool>,
    logger: Logger,
) {
    // allMids (fast)
    {
        let api = api.clone();
        let cache = cache.clone();
        let mut shutdown = shutdown_rx.clone();
        let logger = logger.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    _ = interval.tick() => {
                        match api.all_mids().await {
                            Ok(mids) => cache.write().await.set_mids(mids),
                            Err(e) => logger.log(format!("Error fetching allMids: {e}")),
                        }
                    }
                }
            }
        });
    }

    // perp ctxs (metaAndAssetCtxs)
    {
        let api = api.clone();
        let cache = cache.clone();
        let mut shutdown = shutdown_rx.clone();
        let logger = logger.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(1500));
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    _ = interval.tick() => {
                        match api.meta_and_asset_ctxs().await {
                            Ok((_meta, default_ctxs)) => {
                                let dexes: Vec<String> = {
                                    let r = cache.read().await;
                                    let mut set: HashSet<String> = HashSet::new();
                                    if let Some(entry) = &r.perp_metas {
                                        for (meta_index, perp_meta) in entry.data.iter().enumerate()
                                        {
                                            if meta_index == 0 {
                                                continue;
                                            }
                                            let Some(dex) = perp_meta.universe.first().and_then(|m| {
                                                m.name
                                                    .split_once(':')
                                                    .map(|(p, _)| p.to_string())
                                            }) else {
                                                continue;
                                            };
                                            set.insert(dex);
                                        }
                                    }
                                    let mut dexes: Vec<String> = set.into_iter().collect();
                                    dexes.sort();
                                    dexes
                                };

                                let mut ctxs: Vec<(String, Vec<_>)> = Vec::with_capacity(1 + dexes.len());
                                ctxs.push((String::new(), default_ctxs));
                                for dex in dexes {
                                    match api.meta_and_asset_ctxs_for_dex(Some(&dex)).await {
                                        Ok((_meta, dex_ctxs)) => ctxs.push((dex, dex_ctxs)),
                                        Err(e) => logger.log(format!("Error fetching metaAndAssetCtxs (dex: {dex}): {e}")),
                                    }
                                }
                                cache
                                    .write()
                                    .await
                                    .set_asset_ctxs(AllDexsAssetCtxsEvent { ctxs });
                            }
                            Err(e) => logger.log(format!("Error fetching metaAndAssetCtxs: {e}")),
                        }
                    }
                }
            }
        });
    }

    // allPerpMetas (slow)
    {
        let api = api.clone();
        let cache = cache.clone();
        let mut shutdown = shutdown_rx.clone();
        let logger = logger.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            // Fetch once immediately
            match api.all_perp_metas().await {
                Ok(metas) => cache.write().await.set_perp_metas(metas),
                Err(e) => logger.log(format!("Error fetching allPerpMetas: {e}")),
            }
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    _ = interval.tick() => {
                        match api.all_perp_metas().await {
                            Ok(metas) => cache.write().await.set_perp_metas(metas),
                            Err(e) => logger.log(format!("Error fetching allPerpMetas: {e}")),
                        }
                    }
                }
            }
        });
    }

    // spot meta + ctxs
    {
        let api = api.clone();
        let cache = cache.clone();
        let mut shutdown = shutdown_rx.clone();
        let logger = logger.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(1500));
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    _ = interval.tick() => {
                        match api.spot_meta_and_asset_ctxs().await {
                            Ok((meta, ctxs)) => {
                                let mut w = cache.write().await;
                                w.set_spot_meta(meta);
                                w.set_spot_ctxs(ctxs);
                            }
                            Err(e) => logger.log(format!("Error fetching spotMetaAndAssetCtxs: {e}")),
                        }
                    }
                }
            }
        });
    }
}

async fn handle_connection(
    stream: UnixStream,
    cache: Arc<RwLock<ServerCache>>,
    shutdown_tx: watch::Sender<bool>,
    testnet: bool,
    started_at: i64,
    logger: Logger,
) -> Result<()> {
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut buf = String::new();

    loop {
        buf.clear();
        let n = reader.read_line(&mut buf).await?;
        if n == 0 {
            break;
        }
        let line = buf.trim();
        if line.is_empty() {
            continue;
        }

        let req: RpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(_) => {
                let resp = response_err("0".to_string(), "Invalid JSON");
                let s = serde_json::to_string(&resp)?;
                let _ = write.write_all(s.as_bytes()).await;
                let _ = write.write_all(b"\n").await;
                let _ = write.flush().await;
                continue;
            }
        };

        let (resp, should_shutdown) = handle_request(req, cache.clone(), testnet, started_at).await;
        let s = serde_json::to_string(&resp)?;
        write.write_all(s.as_bytes()).await?;
        write.write_all(b"\n").await?;
        write.flush().await?;

        if should_shutdown {
            logger.log("Shutdown requested via IPC");
            let _ = shutdown_tx.send(true);
            break;
        }
    }

    Ok(())
}

async fn handle_request(
    req: RpcRequest,
    cache: Arc<RwLock<ServerCache>>,
    testnet: bool,
    started_at: i64,
) -> (RpcResponse, bool) {
    let id = req.id.clone();
    match req.method.as_str() {
        "getPrices" => {
            let coin = req
                .params
                .as_ref()
                .and_then(|p| p.get("coin"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let r = cache.read().await;
            let Some(entry) = &r.mids else {
                return (response_err(id, "No data available"), false);
            };
            if let Some(coin) = coin {
                let coin_upper = coin.to_ascii_uppercase();
                let Some(price) = entry.data.get(&coin_upper) else {
                    return (response_err(id, format!("Coin not found: {coin}")), false);
                };
                let mut one = HashMap::new();
                one.insert(coin_upper, price.clone());
                let result = serde_json::to_value(one).unwrap_or(serde_json::Value::Null);
                return (response_ok(id, result, Some(entry.updated_at)), false);
            }
            let result = serde_json::to_value(&entry.data).unwrap_or(serde_json::Value::Null);
            (response_ok(id, result, Some(entry.updated_at)), false)
        }
        "getAssetCtxs" => {
            let r = cache.read().await;
            let Some(entry) = &r.asset_ctxs else {
                return (response_err(id, "No data available"), false);
            };
            let result = serde_json::to_value(&entry.data).unwrap_or(serde_json::Value::Null);
            (response_ok(id, result, Some(entry.updated_at)), false)
        }
        "getPerpMeta" => {
            let r = cache.read().await;
            let Some(entry) = &r.perp_metas else {
                return (response_err(id, "No data available"), false);
            };
            let result = serde_json::to_value(&entry.data).unwrap_or(serde_json::Value::Null);
            (response_ok(id, result, Some(entry.updated_at)), false)
        }
        "getSpotMeta" => {
            let r = cache.read().await;
            let Some(entry) = &r.spot_meta else {
                return (response_err(id, "No data available"), false);
            };
            let result = serde_json::to_value(&entry.data).unwrap_or(serde_json::Value::Null);
            (response_ok(id, result, Some(entry.updated_at)), false)
        }
        "getSpotAssetCtxs" => {
            let r = cache.read().await;
            let Some(entry) = &r.spot_ctxs else {
                return (response_err(id, "No data available"), false);
            };
            let result = serde_json::to_value(&entry.data).unwrap_or(serde_json::Value::Null);
            (response_ok(id, result, Some(entry.updated_at)), false)
        }
        "getStatus" => {
            let r = cache.read().await;
            let cache_status = r.status();
            let status = ServerStatus {
                running: true,
                testnet,
                connected: r.is_connected(),
                started_at,
                uptime: now_ms() - started_at,
                cache: cache_status,
            };
            let result = serde_json::to_value(status).unwrap_or(serde_json::Value::Null);
            (response_ok(id, result, None), false)
        }
        "shutdown" => (
            response_ok(id, serde_json::json!({ "ok": true }), None),
            true,
        ),
        other => (response_err(id, format!("Unknown method: {other}")), false),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI64, Ordering};

    use super::*;

    fn test_clock(now: Arc<AtomicI64>) -> Clock {
        Clock {
            now_ms: Arc::new(move || now.load(Ordering::Relaxed)),
        }
    }

    #[test]
    fn server_cache_matches_js_cache_tests() {
        let now = Arc::new(AtomicI64::new(0));
        let clock = test_clock(now.clone());

        let mut cache = ServerCache::new(clock);

        // Empty cache
        let status = cache.status();
        assert!(!status.has_mids);
        assert!(!status.has_asset_ctxs);
        assert!(!status.has_perp_metas);
        assert!(status.mids_age.is_none());
        assert!(status.asset_ctxs_age.is_none());
        assert!(status.perp_metas_age.is_none());

        // setAllMids
        now.store(0, Ordering::Relaxed);
        let mut mids = HashMap::new();
        mids.insert("BTC".to_string(), "50000.5".to_string());
        mids.insert("ETH".to_string(), "3000.25".to_string());
        cache.set_mids(mids.clone());
        assert_eq!(cache.mids.as_ref().unwrap().data, mids);
        assert_eq!(cache.mids.as_ref().unwrap().updated_at, 0);

        // Timestamp updates
        let first_time = cache.mids.as_ref().unwrap().updated_at;
        now.store(1000, Ordering::Relaxed);
        let mut mids2 = HashMap::new();
        mids2.insert("BTC".to_string(), "51000".to_string());
        cache.set_mids(mids2);
        let second_time = cache.mids.as_ref().unwrap().updated_at;
        assert_eq!(second_time - first_time, 1000);

        // Age calculations (matching JS fake-timer test)
        let mut cache = ServerCache::new(test_clock(now.clone()));
        now.store(0, Ordering::Relaxed);
        cache.set_mids(HashMap::from([("BTC".to_string(), "50000".to_string())]));

        now.store(1000, Ordering::Relaxed);
        cache.set_asset_ctxs(AllDexsAssetCtxsEvent { ctxs: vec![] });

        // setAllPerpMetas (including onlyIsolated flag)
        now.store(3000, Ordering::Relaxed);
        cache.set_perp_metas(vec![PerpMeta {
            universe: vec![hyperliquid_cli::hl_api::PerpAssetMeta {
                sz_decimals: 0,
                name: "MEME".to_string(),
                max_leverage: 10,
                only_isolated: true,
                is_delisted: false,
            }],
            collateral_token: 0,
            margin_tables: serde_json::json!([]),
        }]);
        assert!(cache.perp_metas.as_ref().unwrap().data[0].universe[0].only_isolated);

        now.store(3500, Ordering::Relaxed);
        let status = cache.status();
        assert_eq!(status.mids_age, Some(3500));
        assert_eq!(status.asset_ctxs_age, Some(2500));
        assert_eq!(status.perp_metas_age, Some(500));

        // Connection heuristic
        now.store(0, Ordering::Relaxed);
        let mut cache = ServerCache::new(test_clock(now.clone()));
        let mut mids = HashMap::new();
        mids.insert("BTC".to_string(), "50000".to_string());
        cache.set_mids(mids);
        now.store(4_999, Ordering::Relaxed);
        assert!(cache.is_connected());
        now.store(5_000, Ordering::Relaxed);
        assert!(!cache.is_connected());
    }

    #[tokio::test]
    async fn ipc_handle_request_matches_js_ipc_tests() {
        let now = Arc::new(AtomicI64::new(0));
        let clock = test_clock(now.clone());
        let cache = ServerCache::new(clock);

        // getPrices empty
        let cache = Arc::new(RwLock::new(cache));
        let req = RpcRequest {
            id: "1".to_string(),
            method: "getPrices".to_string(),
            params: None,
        };
        let (resp, shutdown) = handle_request(req, cache.clone(), false, 0).await;
        assert_eq!(resp.id, "1");
        assert_eq!(resp.error.as_deref(), Some("No data available"));
        assert!(!shutdown);

        // Populate mids
        now.store(123, Ordering::Relaxed);
        {
            let mut w = cache.write().await;
            let mut mids = HashMap::new();
            mids.insert("BTC".to_string(), "50000".to_string());
            mids.insert("ETH".to_string(), "3000".to_string());
            w.set_mids(mids);
        }

        // getPrices all
        let req = RpcRequest {
            id: "2".to_string(),
            method: "getPrices".to_string(),
            params: None,
        };
        let (resp, shutdown) = handle_request(req, cache.clone(), false, 0).await;
        assert!(!shutdown);
        let data: HashMap<String, String> = serde_json::from_value(resp.result.unwrap()).unwrap();
        assert_eq!(data.get("BTC").unwrap(), "50000");
        assert_eq!(data.get("ETH").unwrap(), "3000");
        assert_eq!(resp.cached_at, Some(123));

        // getPrices coin param is case-insensitive and returns uppercase key
        let req = RpcRequest {
            id: "3".to_string(),
            method: "getPrices".to_string(),
            params: Some(serde_json::json!({ "coin": "btc" })),
        };
        let (resp, shutdown) = handle_request(req, cache.clone(), false, 0).await;
        assert!(!shutdown);
        let data: HashMap<String, String> = serde_json::from_value(resp.result.unwrap()).unwrap();
        assert_eq!(
            data,
            HashMap::from([("BTC".to_string(), "50000".to_string())])
        );

        // getPrices unknown coin
        let req = RpcRequest {
            id: "4".to_string(),
            method: "getPrices".to_string(),
            params: Some(serde_json::json!({ "coin": "UNKNOWN" })),
        };
        let (resp, shutdown) = handle_request(req, cache.clone(), false, 0).await;
        assert!(!shutdown);
        assert_eq!(resp.error.as_deref(), Some("Coin not found: UNKNOWN"));

        // getAssetCtxs empty -> error
        let req = RpcRequest {
            id: "5".to_string(),
            method: "getAssetCtxs".to_string(),
            params: None,
        };
        let (resp, _) = handle_request(req, cache.clone(), false, 0).await;
        assert_eq!(resp.error.as_deref(), Some("No data available"));

        // getPerpMeta empty -> error
        let req = RpcRequest {
            id: "6".to_string(),
            method: "getPerpMeta".to_string(),
            params: None,
        };
        let (resp, _) = handle_request(req, cache.clone(), false, 0).await;
        assert_eq!(resp.error.as_deref(), Some("No data available"));

        // getStatus reflects testnet + connected
        now.store(1_000, Ordering::Relaxed);
        {
            let mut w = cache.write().await;
            w.set_mids(HashMap::from([("BTC".to_string(), "50000".to_string())]));
        }
        now.store(2_000, Ordering::Relaxed);
        let req = RpcRequest {
            id: "7".to_string(),
            method: "getStatus".to_string(),
            params: None,
        };
        let (resp, _) = handle_request(req, cache.clone(), true, 1_000).await;
        assert!(resp.error.is_none());
        let status: ServerStatus = serde_json::from_value(resp.result.unwrap()).unwrap();
        assert!(status.running);
        assert!(status.testnet);
        assert!(status.connected);
        assert_eq!(status.started_at, 1_000);

        // shutdown
        let req = RpcRequest {
            id: "8".to_string(),
            method: "shutdown".to_string(),
            params: None,
        };
        let (resp, shutdown) = handle_request(req, cache.clone(), false, 0).await;
        assert!(shutdown);
        assert!(resp.error.is_none());

        // unknown method
        let req = RpcRequest {
            id: "9".to_string(),
            method: "unknownMethod".to_string(),
            params: None,
        };
        let (resp, shutdown) = handle_request(req, cache.clone(), false, 0).await;
        assert!(!shutdown);
        assert_eq!(resp.error.as_deref(), Some("Unknown method: unknownMethod"));
        assert_eq!(resp.id, "9");
    }
}
