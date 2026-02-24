mod cache;
mod retry;

use std::{collections::HashSet, net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use cache::Cache;
use reqwest::Client;
use serde_json::{Value, json};
use tokio::signal;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

// curl -H 'Content-Type: application/json' -d '{"url":""}' http://localhost:8080/url

// tar -cjf evm-cache.tar.gz .evm-proxy-cache
// scp evm-cache.tar.gz nuc:/home/work/Code/solenoid/rpc-proxy/evm-cache.tar.gz
// ssh nuc
// cd /home/work/Code/solenoid/rpc-proxy
// tar -cjf evm-cache-backup.tar.gz .evm-proxy-cache
// rm -rf .evm-proxy-cache
// tar -xf evm-cache.tar.gz

struct AppState {
    upstream_url: RwLock<String>,
    http_client: Client,
    cache: Cache,
    offline: bool,
    empty: bool,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();

    let args: HashSet<String> = std::env::args().collect();
    let offline = args.contains("--offline");
    let empty = args.contains("--empty");

    if offline && empty {
        error!("Cannot run in offline and empty mode at the same time");
        std::process::exit(1);
    }

    let bind_addr: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
        .parse()
        .expect("invalid BIND_ADDR");

    let upstream_url =
        std::env::var("URL").unwrap_or_else(|_| "https://ethereum-rpc.publicnode.com".to_string());

    let cache_dir: PathBuf = std::env::var("CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".evm-proxy-cache"));

    let cache = Cache::open(&cache_dir).expect("failed to open cache directory");

    let http_client = Client::builder()
        .pool_max_idle_per_host(8)
        .build()
        .expect("failed building reqwest client");

    let state = Arc::new(AppState {
        upstream_url: RwLock::new(upstream_url),
        http_client,
        cache,
        offline,
        empty,
    });

    let app = build_router(state.clone());

    info!(%bind_addr, "Starting EVM JSON-RPC caching proxy");
    if offline {
        info!("Running in offline mode");
    }

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .expect("failed to bind address");
    let shutdown = shutdown_signal();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .expect("server error");
}

fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/rpc", post(handle_jsonrpc))
        .route("/url", post(handle_update_url))
        .route("/ready", get(handle_ready))
        .with_state(state)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        sigterm.recv().await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    info!("Shutting down");
}

async fn handle_ready(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_blockNumber",
        "params": []
    });
    match forward(&state, body).await {
        Ok(resp) => {
            if let Some(result) = resp.get("result").and_then(|v| v.as_str()) {
                (StatusCode::OK, Json(json!({ "block": result })))
            } else if resp.get("error").is_some() {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({ "error": "upstream error" })),
                )
            } else {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({ "error": "invalid response" })),
                )
            }
        }
        Err(err) => {
            error!(error = %err, "Ready check failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": err.to_string() })),
            )
        }
    }
}

async fn handle_update_url(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if let Some(url) = body.get("url").and_then(|url| url.as_str()) {
        *state.upstream_url.write().await = url.to_string();
    } else {
        return (StatusCode::BAD_REQUEST, "ignored").into_response();
    }
    (StatusCode::OK, "URL updated").into_response()
}

async fn handle_jsonrpc(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if body.is_array() {
        match forward(&state, body).await {
            Ok(resp) => return (StatusCode::OK, Json(resp)).into_response(),
            Err(err) => return error_response(err).into_response(),
        }
    }

    let (method, params, id) = {
        let Some(obj) = body.as_object() else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "jsonrpc": "2.0",
                    "error": {"code": -32600, "message": "Invalid Request"},
                    "id": null
                })),
            )
                .into_response();
        };
        let method = obj
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let params = obj.get("params").cloned().unwrap_or_else(|| json!([]));
        let id = obj.get("id").cloned().unwrap_or(Value::Null);
        (method, params, id)
    };
    let method = method.as_str();

    debug!(method, "Request");

    let is_latest = params
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .map(|s| s == "latest" || s == "pending" || s == "earliest")
        .unwrap_or(false);

    let cacheable = !state.empty && !is_latest;

    if !cacheable {
        return forward_and_respond(&state, body).await;
    }

    let arr = params.as_array();
    let p = |i: usize| arr.and_then(|a| a.get(i)).and_then(|v| v.as_str());

    match method {
        "eth_getStorageAt" => {
            let (Some(address), Some(slot), Some(block)) = (p(0), p(1), p(2)) else {
                return forward_and_respond(&state, body).await;
            };
            if let Ok(Some(val)) = state.cache.get_storage_at(address, slot, block) {
                info!(method, address, slot, block, "Cache hit");
                return jsonrpc_ok(&id, json!(val));
            }
            cache_miss_and_forward(&state, method, body, &id, |result| {
                let val = result.as_str().unwrap_or(
                    "0x0000000000000000000000000000000000000000000000000000000000000000",
                );
                if let Err(e) = state.cache.put_storage_at(address, slot, block, val) {
                    error!(method, address, slot, block, error=%e, "Cache put failed");
                }
            })
            .await
        }
        "eth_getBalance" => {
            let (Some(address), Some(block)) = (p(0), p(1)) else {
                return forward_and_respond(&state, body).await;
            };
            if let Ok(Some(val)) = state.cache.get_balance(address, block) {
                info!(method, address, block, "Cache hit");
                return jsonrpc_ok(&id, json!(val));
            }
            cache_miss_and_forward(&state, method, body, &id, |result| {
                if let Some(val) = result.as_str() {
                    let _ = state.cache.put_balance(address, block, val);
                }
            })
            .await
        }
        "eth_getTransactionCount" => {
            let (Some(address), Some(block)) = (p(0), p(1)) else {
                return forward_and_respond(&state, body).await;
            };
            if let Ok(Some(val)) = state.cache.get_tx_count(address, block) {
                info!(method, address, block, "Cache hit");
                return jsonrpc_ok(&id, json!(val));
            }
            cache_miss_and_forward(&state, method, body, &id, |result| {
                if let Some(val) = result.as_str() {
                    let _ = state.cache.put_tx_count(address, block, val);
                }
            })
            .await
        }
        "eth_getCode" => {
            let (Some(address), Some(_block)) = (p(0), p(1)) else {
                return forward_and_respond(&state, body).await;
            };
            if let Ok(Some(code)) = state.cache.get_code(address) {
                info!(method, address, "Cache hit");
                return jsonrpc_ok(&id, json!(code));
            }
            cache_miss_and_forward(&state, method, body, &id, |result| {
                if let Some(code) = result.as_str() {
                    let _ = state.cache.put_code(address, code);
                }
            })
            .await
        }
        "eth_getBlockByHash" => {
            let Some(hash) = p(0) else {
                return forward_and_respond(&state, body).await;
            };
            if let Ok(Some(json_str)) = state.cache.get_block_by_hash(hash) {
                info!(method, hash, "Cache hit");
                if let Ok(val) = serde_json::from_str::<Value>(&json_str) {
                    return jsonrpc_ok(&id, val);
                }
            }
            cache_miss_and_forward(&state, method, body, &id, |result| {
                if let Some(obj) = result.as_object() {
                    let block_hash = obj.get("hash").and_then(|v| v.as_str());
                    let block_number = obj.get("number").and_then(|v| v.as_str());
                    if let (Some(h), Some(n)) = (block_hash, block_number) {
                        let json_str = serde_json::to_string(result).unwrap_or_default();
                        let _ = state.cache.put_block(h, n, &json_str);
                    }
                }
            })
            .await
        }
        "eth_getBlockByNumber" => {
            let Some(number) = p(0) else {
                return forward_and_respond(&state, body).await;
            };
            if let Ok(Some(json_str)) = state.cache.get_block_by_number(number) {
                info!(method, number, "Cache hit");
                if let Ok(val) = serde_json::from_str::<Value>(&json_str) {
                    return jsonrpc_ok(&id, val);
                }
            }
            cache_miss_and_forward(&state, method, body, &id, |result| {
                if let Some(obj) = result.as_object() {
                    let block_hash = obj.get("hash").and_then(|v| v.as_str());
                    let block_number = obj.get("number").and_then(|v| v.as_str());
                    if let (Some(h), Some(n)) = (block_hash, block_number) {
                        let json_str = serde_json::to_string(result).unwrap_or_default();
                        let _ = state.cache.put_block(h, n, &json_str);
                    }
                }
            })
            .await
        }
        "eth_getTransactionByHash" => {
            let Some(hash) = p(0) else {
                return forward_and_respond(&state, body).await;
            };
            if let Ok(Some(json_str)) = state.cache.get_tx(hash) {
                info!(method, hash, "Cache hit");
                if let Ok(val) = serde_json::from_str::<Value>(&json_str) {
                    return jsonrpc_ok(&id, val);
                }
            }
            cache_miss_and_forward(&state, method, body, &id, |result| {
                if !result.is_null() {
                    let json_str = serde_json::to_string(result).unwrap_or_default();
                    let _ = state.cache.put_tx(hash, &json_str);
                }
            })
            .await
        }
        "eth_getTransactionReceipt" => {
            let Some(hash) = p(0) else {
                return forward_and_respond(&state, body).await;
            };
            if let Ok(Some(json_str)) = state.cache.get_receipt(hash) {
                info!(method, hash, "Cache hit");
                if let Ok(val) = serde_json::from_str::<Value>(&json_str) {
                    return jsonrpc_ok(&id, val);
                }
            }
            cache_miss_and_forward(&state, method, body, &id, |result| {
                if !result.is_null() {
                    let json_str = serde_json::to_string(result).unwrap_or_default();
                    let _ = state.cache.put_receipt(hash, &json_str);
                }
            })
            .await
        }
        _ => forward_and_respond(&state, body).await,
    }
}

async fn cache_miss_and_forward(
    state: &AppState,
    method: &str,
    body: Value,
    id: &Value,
    on_result: impl FnOnce(&Value),
) -> axum::response::Response {
    if state.offline {
        warn!(method, "Cache miss (offline mode)");
        return jsonrpc_err(id, -32001, "Server running in offline mode");
    }
    match forward(state, body).await {
        Ok(resp) => {
            warn!(request=%method, result=?resp.get("result"), "Cache miss");
            if resp.get("error").is_none()
                && let Some(result) = resp.get("result")
            {
                on_result(result);
            }
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(err) => error_response(err).into_response(),
    }
}

async fn forward_and_respond(state: &AppState, body: Value) -> axum::response::Response {
    match forward(state, body).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(err) => error_response(err).into_response(),
    }
}

fn jsonrpc_ok(id: &Value, result: Value) -> axum::response::Response {
    (
        StatusCode::OK,
        Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        })),
    )
        .into_response()
}

fn jsonrpc_err(id: &Value, code: i64, message: &str) -> axum::response::Response {
    (
        StatusCode::OK,
        Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": code, "message": message},
        })),
    )
        .into_response()
}

#[derive(Debug)]
enum ForwardError {
    Reqwest(reqwest::Error),
    Http {
        status: reqwest::StatusCode,
        body: String,
    },
}

impl ForwardError {
    fn is_retryable(&self) -> bool {
        match self {
            Self::Reqwest(e) => e.is_connect() || e.is_timeout() || e.is_request(),
            Self::Http { status, .. } => status.is_server_error() || status.as_u16() == 429,
        }
    }
}

impl std::fmt::Display for ForwardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Reqwest(e) => write!(f, "Request failed: {}", e),
            Self::Http { status, body } => write!(f, "upstream error: {} - {}", status, body),
        }
    }
}

impl std::error::Error for ForwardError {}

async fn forward(state: &AppState, body: Value) -> anyhow::Result<Value> {
    let url = state.upstream_url.read().await.clone();
    retry::retry(
        || async {
            let response = state
                .http_client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(ForwardError::Reqwest)?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(ForwardError::Http { status, body });
            }

            response.json().await.map_err(ForwardError::Reqwest)
        },
        ForwardError::is_retryable,
        retry::RetryConfig {
            max_attempts: 30,
            initial_delay_ms: 1000,
            max_delay_ms: 20_000,
        },
    )
    .await
    .map_err(Into::into)
}

fn error_response(err: anyhow::Error) -> impl IntoResponse {
    error!(error = %err, "Request failed");
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({
            "jsonrpc": "2.0",
            "error": {"code": -32000, "message": err.to_string()},
            "id": null
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Mock {
        hits: Arc<AtomicUsize>,
        url: String,
    }

    impl Mock {
        async fn start() -> Self {
            let hits = Arc::new(AtomicUsize::new(0));
            let hits2 = hits.clone();

            let app = Router::new().route(
                "/",
                post(move |Json(body): Json<Value>| {
                    let hits = hits2.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        let method = body.get("method").and_then(|m| m.as_str()).unwrap_or("");
                        let id = body.get("id").cloned().unwrap_or(Value::Null);
                        let result = match method {
                            "eth_getBalance" => json!("0xde0b6b3a7640000"),
                            "eth_getTransactionCount" => json!("0x5"),
                            "eth_getStorageAt" => json!(
                                "0x00000000000000000000000000000000000000000000000000000000deadbeef"
                            ),
                            "eth_getCode" => json!("0x6080604052"),
                            "eth_getBlockByHash" | "eth_getBlockByNumber" => json!({
                                "hash": "0xabc123",
                                "number": "0x100",
                                "timestamp": "0x60000000"
                            }),
                            "eth_getTransactionByHash" => json!({
                                "hash": "0xdeadbeef",
                                "from": "0x1111111111111111111111111111111111111111"
                            }),
                            "eth_getTransactionReceipt" => json!({
                                "transactionHash": "0xdeadbeef",
                                "status": "0x1"
                            }),
                            _ => Value::Null,
                        };
                        Json(json!({"jsonrpc": "2.0", "id": id, "result": result}))
                    }
                }),
            );

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(axum::serve(listener, app).into_future());

            Mock {
                hits,
                url: format!("http://{addr}/"),
            }
        }

        fn hits(&self) -> usize {
            self.hits.load(Ordering::SeqCst)
        }
    }

    async fn start_proxy(upstream_url: &str, cache_dir: &Path) -> String {
        let cache = Cache::open(cache_dir).unwrap();
        let state = Arc::new(AppState {
            upstream_url: RwLock::new(upstream_url.to_string()),
            http_client: Client::new(),
            cache,
            offline: false,
            empty: false,
        });
        let app = build_router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());
        format!("http://{addr}/rpc")
    }

    use std::path::Path;

    async fn rpc(client: &Client, url: &str, method: &str, params: Value) -> Value {
        client
            .post(url)
            .json(&json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}))
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()
    }

    async fn assert_cached(
        client: &Client,
        proxy_url: &str,
        mock: &Mock,
        method: &str,
        params: Value,
    ) {
        let before = mock.hits();
        let r1 = rpc(client, proxy_url, method, params.clone()).await;
        assert_eq!(
            mock.hits(),
            before + 1,
            "{method}: first call should hit upstream"
        );
        assert!(r1.get("result").is_some(), "{method}: should have result");

        let r2 = rpc(client, proxy_url, method, params.clone()).await;
        assert_eq!(
            mock.hits(),
            before + 1,
            "{method}: second call should be cached"
        );
        assert_eq!(
            r1.get("result"),
            r2.get("result"),
            "{method}: cached result should match"
        );
    }

    #[tokio::test]
    async fn test_caching() {
        let mock = Mock::start().await;
        let cache_dir = PathBuf::from("target/test-cache");
        let _ = std::fs::remove_dir_all(&cache_dir);
        let proxy_url = start_proxy(&mock.url, &cache_dir).await;
        let client = Client::new();

        assert_cached(
            &client,
            &proxy_url,
            &mock,
            "eth_getBalance",
            json!(["0x1234", "0x100"]),
        )
        .await;
        assert_cached(
            &client,
            &proxy_url,
            &mock,
            "eth_getTransactionCount",
            json!(["0x1234", "0x100"]),
        )
        .await;
        assert_cached(
            &client,
            &proxy_url,
            &mock,
            "eth_getStorageAt",
            json!(["0x1234", "0x0", "0x100"]),
        )
        .await;
        assert_cached(
            &client,
            &proxy_url,
            &mock,
            "eth_getCode",
            json!(["0x1234", "0x100"]),
        )
        .await;
        assert_cached(
            &client,
            &proxy_url,
            &mock,
            "eth_getTransactionByHash",
            json!(["0xdeadbeef"]),
        )
        .await;
        assert_cached(
            &client,
            &proxy_url,
            &mock,
            "eth_getTransactionReceipt",
            json!(["0xdeadbeef"]),
        )
        .await;

        assert_cached(
            &client,
            &proxy_url,
            &mock,
            "eth_getBlockByNumber",
            json!(["0x100", false]),
        )
        .await;

        let before = mock.hits();
        let r = rpc(
            &client,
            &proxy_url,
            "eth_getBlockByHash",
            json!(["0xabc123", false]),
        )
        .await;
        assert_eq!(
            mock.hits(),
            before,
            "eth_getBlockByHash: should be cached from block-by-number"
        );
        assert!(r.get("result").is_some());
    }

    #[tokio::test]
    async fn test_storage() {
        let mock = Mock::start().await;
        let cache_dir = PathBuf::from("target/test-cache-slots");
        let _ = std::fs::remove_dir_all(&cache_dir);
        let proxy_url = start_proxy(&mock.url, &cache_dir).await;
        let client = Client::new();

        let cases = [
            ("0x1f98431c8ad98523631ae4a59f267346ea31f984", "0x3"),
            ("0xf38521f130fccf29db1961597bc5d2b60f995f85", "0x1"),
            ("0x5c69bee701ef814a2b6a3edd4b1652cb9cc5aa6f", "0x0"),
        ];

        for (addr, slot) in &cases {
            assert_cached(
                &client,
                &proxy_url,
                &mock,
                "eth_getStorageAt",
                json!([addr, slot, "0x17599f9"]),
            )
            .await;
        }
    }
}
