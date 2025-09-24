use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use reqwest::Client;
use serde_json::{Value, json};
use tokio::signal;
use tokio::{fs, sync::RwLock};
use tracing::{error, info, warn};

struct AppState {
    upstream_url: String,
    http_client: Client,
    cache_file: PathBuf,
    persistent: RwLock<HashMap<String, Value>>,
}

// TODO: Add proper logging

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();
    dotenv::dotenv().ok();

    let bind_addr: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
        .parse()
        .expect("invalid BIND_ADDR");

    let upstream_url = std::env::var("URL1").unwrap_or_else(|_| {
        // Default to public Cloudflare endpoint if not provided
        "https://cloudflare-eth.com".to_string()
    });

    let cache_file: PathBuf = std::env::var("CACHE_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".evm-proxy-cache.json"));
    let initial_persistent = load_persistent(&cache_file).await.unwrap_or_default();

    let http_client = Client::builder()
        .pool_max_idle_per_host(8)
        .build()
        .expect("failed building reqwest client");

    let state = Arc::new(AppState {
        upstream_url,
        http_client,
        cache_file,
        persistent: RwLock::new(initial_persistent),
    });

    let app = Router::new()
        .route("/", post(handle_jsonrpc))
        .with_state(state.clone());

    info!(%bind_addr, "Starting EVM JSON-RPC caching proxy");
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .expect("failed to bind address");
    let shutdown = shutdown_signal_with_persist(state.clone());
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .expect("server error");
}

async fn shutdown_signal_with_persist(state: Arc<AppState>) {
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
    if let Err(e) = save_persistent(&state).await {
        error!(error = %e, "Failed to save persistent cache on shutdown");
    } else {
        info!("Persistent cache saved on shutdown");
    }
}

async fn handle_jsonrpc(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    // If batch (array), bypass cache and forward as-is
    if body.is_array() {
        match forward(&state, body).await {
            Ok(resp) => return (StatusCode::OK, Json(resp)).into_response(),
            Err(err) => return error_response(err).into_response(),
        }
    }

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

    let method = obj.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let cacheable = matches!(
        method,
        "eth_getCode"
            | "eth_getBalance"
            | "eth_getTransactionCount"
            | "eth_getStorageAt"
            | "eth_getBlockByHash"
            | "eth_getBlockByNumber"
    );

    if cacheable {
        let params = obj.get("params").cloned().unwrap_or_else(|| json!([]));
        let key = cache_key(method, &params);

        // Check persistent map
        if let Some(v) = {
            let store = state.persistent.read().await;
            store.get(&key).cloned()
        } {
            info!(%key, "Cache hit");
            return (StatusCode::OK, Json(v)).into_response();
        }

        warn!(%key, "Cache miss");
        match forward(&state, body.clone()).await {
            Ok(resp) => {
                {
                    let mut store = state.persistent.write().await;
                    store.insert(key, resp.clone());
                }
                (StatusCode::OK, Json(resp)).into_response()
            }
            Err(err) => error_response(err).into_response(),
        }
    } else {
        match forward(&state, body).await {
            Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
            Err(err) => error_response(err).into_response(),
        }
    }
}

fn cache_key(method: &str, params: &Value) -> String {
    let params_str = serde_json::to_string(params).unwrap_or_else(|_| "null".to_string());
    format!("{}:{}", method, params_str)
}

async fn forward(state: &AppState, body: Value) -> anyhow::Result<Value> {
    let response = state
        .http_client
        .post(&state.upstream_url)
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        error!(?status, body = %text, "Upstream returned error");
        anyhow::bail!("upstream error: {}", status);
    }

    let v = response.json::<Value>().await?;
    Ok(v)
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

async fn load_persistent(path: &PathBuf) -> anyhow::Result<HashMap<String, Value>> {
    let data = match fs::read(path).await {
        Ok(d) => d,
        Err(_) => return Ok(HashMap::new()),
    };
    let v: Value = serde_json::from_slice(&data)?;
    if let Some(map) = v.get("entries").and_then(|e| e.as_object()) {
        let mut out = HashMap::new();
        for (k, val) in map.iter() {
            out.insert(k.clone(), val.clone());
        }
        return Ok(out);
    }
    // Fallback: plain map
    if let Some(map) = v.as_object() {
        let mut out = HashMap::new();
        for (k, val) in map.iter() {
            out.insert(k.clone(), val.clone());
        }
        return Ok(out);
    }
    Ok(HashMap::new())
}

async fn save_persistent(state: &AppState) -> anyhow::Result<()> {
    let saved_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let entries = {
        let store = state.persistent.read().await;
        serde_json::to_value(&*store)?
    };
    let wrapper = json!({
        "saved_at": saved_at,
        "entries": entries,
    });
    let pretty = serde_json::to_string_pretty(&wrapper)?;
    let path = &state.cache_file;
    let tmp_path = path.with_extension("json.tmp");
    fs::write(&tmp_path, pretty).await?;
    if let Err(_e) = fs::rename(&tmp_path, path).await {
        let _ = fs::remove_file(path).await;
        let _ = fs::rename(&tmp_path, path).await;
    }
    Ok(())
}
