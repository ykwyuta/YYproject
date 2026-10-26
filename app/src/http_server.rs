//! Streamable HTTP with JSON responses (no optional SSE channel).
use crate::{mcp, pat};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::any,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use yy_project::Store;
const VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];
struct Client {
    session: mcp::Session,
    version: String,
    owner: String,
    fingerprint: String,
    seen: Instant,
}
struct Database {
    store: Store,
    clients: BTreeMap<String, Client>,
}
#[derive(Clone)]
struct Service {
    db: Arc<Mutex<Database>>,
    config: PathBuf,
    hosts: BTreeSet<String>,
    origins: BTreeSet<String>,
    capacity: Arc<tokio::sync::Semaphore>,
}
fn error(status: StatusCode, message: &str) -> Response {
    let mut response = (
        status,
        Json(json!({"jsonrpc":"2.0","error":{"code":-32000,"message":message}})),
    )
        .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    if status == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            header::HeaderValue::from_static("Bearer"),
        );
    }
    if status == StatusCode::METHOD_NOT_ALLOWED {
        response.headers_mut().insert(
            header::ALLOW,
            header::HeaderValue::from_static("POST, DELETE"),
        );
    }
    response
}
fn one<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, &'static str> {
    if headers.get_all(name).iter().count() > 1 {
        return Err("Duplicate transport header");
    }
    headers
        .get(name)
        .map(|v| v.to_str().map_err(|_| "Invalid transport header"))
        .transpose()
}
fn process(service: Service, headers: HeaderMap, method: Method, body: Vec<u8>) -> Response {
    let config = match pat::Config::load(&service.config) {
        Ok(c) => c,
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "PAT configuration unavailable",
            );
        }
    };
    let token = match one(&headers, "authorization") {
        Ok(Some(v)) => v.strip_prefix("Bearer ").unwrap_or(""),
        _ => "",
    };
    let Some(user) = config.authenticate(token) else {
        return error(StatusCode::UNAUTHORIZED, "Valid PAT required");
    };
    let version = match one(&headers, "mcp-protocol-version") {
        Ok(Some(v)) if VERSIONS.contains(&v) => Some(v),
        Ok(None) => None,
        _ => return error(StatusCode::BAD_REQUEST, "Unsupported MCP protocol version"),
    };
    if method == Method::GET {
        if let Ok(Some(id)) = one(&headers, "mcp-session-id") {
            let Ok(mut db) = service.db.lock() else {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database worker unavailable",
                );
            };
            db.clients
                .retain(|_, c| c.seen.elapsed() < Duration::from_secs(1800));
            let Some(client) = db.clients.get(id) else {
                return error(StatusCode::NOT_FOUND, "Session expired or unknown");
            };
            if client.owner != user.name || client.fingerprint != user.pat_sha256 {
                return error(StatusCode::FORBIDDEN, "Session belongs to another PAT");
            }
        }
        let mut r = error(
            StatusCode::METHOD_NOT_ALLOWED,
            "This endpoint uses JSON responses, not SSE",
        );
        r.headers_mut().insert(
            header::ALLOW,
            header::HeaderValue::from_static("POST, DELETE"),
        );
        return r;
    }
    if method != Method::POST && method != Method::DELETE {
        return error(StatusCode::METHOD_NOT_ALLOWED, "Use POST or DELETE");
    }
    let session_id = match one(&headers, "mcp-session-id") {
        Ok(v) => v,
        Err(e) => return error(StatusCode::BAD_REQUEST, e),
    };
    let mut request = if method == Method::POST {
        match serde_json::from_slice::<Value>(&body) {
            Ok(v) if valid_message(&v) => v,
            _ => return error(StatusCode::BAD_REQUEST, "Expected one JSON-RPC message"),
        }
    } else {
        Value::Null
    };
    let initialize = request["method"] == "initialize";
    let mut db = match service.db.lock() {
        Ok(db) => db,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database worker unavailable",
            );
        }
    };
    db.clients
        .retain(|_, c| c.seen.elapsed() < Duration::from_secs(1800));
    if initialize && session_id.is_none() {
        if request.get("id").is_none() {
            return error(StatusCode::BAD_REQUEST, "Initialize requires a request ID");
        }
        if db.clients.len() >= 256 {
            return error(StatusCode::SERVICE_UNAVAILABLE, "Session limit reached");
        }
        if request["params"]["protocolVersion"]
            .as_str()
            .is_some_and(|v| !VERSIONS.contains(&v))
        {
            request["params"]["protocolVersion"] = json!(VERSIONS[0]);
        }
        let mut session = mcp::Session::default();
        let Some(reply) = session.handle(&mut db.store, request) else {
            return error(StatusCode::BAD_REQUEST, "Invalid initialization");
        };
        if reply.get("error").is_some() {
            return (StatusCode::BAD_REQUEST, Json(reply)).into_response();
        }
        let id = match pat::random_id() {
            Ok(id) => id,
            Err(_) => {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Session ID generation failed",
                );
            }
        };
        let negotiated = reply["result"]["protocolVersion"]
            .as_str()
            .unwrap_or(VERSIONS[0])
            .to_string();
        db.clients.insert(
            id.clone(),
            Client {
                session,
                version: negotiated,
                owner: user.name,
                fingerprint: user.pat_sha256,
                seen: Instant::now(),
            },
        );
        let mut response = Json(reply).into_response();
        response
            .headers_mut()
            .insert("mcp-session-id", id.parse().unwrap());
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
        return response;
    }
    let Some(id) = session_id else {
        return error(StatusCode::BAD_REQUEST, "MCP-Session-Id required");
    };
    let Database { store, clients } = &mut *db;
    let Some(client) = clients.get_mut(id) else {
        return error(StatusCode::NOT_FOUND, "Session expired or unknown");
    };
    if client.owner != user.name || client.fingerprint != user.pat_sha256 {
        return error(StatusCode::FORBIDDEN, "Session belongs to another PAT");
    }
    if version.is_some_and(|v| v != client.version) {
        return error(
            StatusCode::BAD_REQUEST,
            "Protocol version does not match session",
        );
    }
    if method == Method::DELETE {
        clients.remove(id);
        return StatusCode::NO_CONTENT.into_response();
    }
    if initialize {
        return error(StatusCode::BAD_REQUEST, "Session already initialized");
    }
    client.seen = Instant::now();
    if request.get("method").is_none() {
        return StatusCode::ACCEPTED.into_response();
    }
    if request["method"] == "tools/call"
        && matches!(
            request["params"]["name"].as_str(),
            Some("project_create" | "project_change")
        )
    {
        if user.read_only {
            return if let Some(id) = request.get("id") {
                Json(json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":"PAT user is read-only"}],"isError":true}})).into_response()
            } else {
                StatusCode::ACCEPTED.into_response()
            };
        }
        if request["params"]["arguments"].is_object() {
            request["params"]["arguments"]["actor"] = json!(user.name);
        }
    }
    match client.session.handle(store, request) {
        Some(reply) => Json(reply).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}
fn valid_message(v: &Value) -> bool {
    v.is_object()
        && v["jsonrpc"] == "2.0"
        && v.get("id")
            .is_none_or(|id| id.is_string() || id.is_i64() || id.is_u64())
        && if v.get("method").is_some() {
            v["method"].is_string()
        } else {
            v.get("id").is_some() && (v.get("result").is_some() ^ v.get("error").is_some())
        }
}
async fn endpoint(State(service): State<Service>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let host = match one(&parts.headers, "host") {
        Ok(Some(h)) => h,
        _ => return error(StatusCode::FORBIDDEN, "Host required"),
    };
    if !service.hosts.contains(host) {
        return error(StatusCode::FORBIDDEN, "Host not allowed");
    }
    match one(&parts.headers, "origin") {
        Ok(Some(o)) if service.origins.contains(o) => {}
        Ok(None) => {}
        _ => return error(StatusCode::FORBIDDEN, "Origin not allowed"),
    };
    if parts.method == Method::POST {
        let content = parts
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if content.split(';').next().unwrap_or("").trim() != "application/json" {
            return error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Content-Type must be application/json",
            );
        }
        let accept = parts
            .headers
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let types = accept
            .split(',')
            .map(|s| s.split(';').next().unwrap_or("").trim())
            .collect::<Vec<_>>();
        if !types.contains(&"application/json") || !types.contains(&"text/event-stream") {
            return error(
                StatusCode::NOT_ACCEPTABLE,
                "Accept must include application/json and text/event-stream",
            );
        }
    }
    let Ok(permit) = service.capacity.clone().try_acquire_owned() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Request limit reached");
    };
    let bytes = match tokio::time::timeout(Duration::from_secs(15), to_bytes(body, 2 * 1024 * 1024))
        .await
    {
        Ok(Ok(b)) => b.to_vec(),
        Ok(Err(_)) => return error(StatusCode::PAYLOAD_TOO_LARGE, "Message exceeds 2 MiB"),
        Err(_) => return error(StatusCode::REQUEST_TIMEOUT, "Body read timed out"),
    };
    match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        process(service, parts.headers, parts.method, bytes)
    })
    .await
    {
        Ok(mut r) => {
            r.headers_mut().insert(
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("no-store"),
            );
            r
        }
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "Request worker failed"),
    }
}
pub fn serve(
    store: Store,
    listen: SocketAddr,
    config: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let auth = pat::Config::load(&config).map_err(|e| e.to_string())?;
    let socket = std::net::TcpListener::bind(listen)?;
    socket.set_nonblocking(true)?;
    let address = socket.local_addr()?;
    let mut hosts = auth
        .server
        .allowed_hosts
        .into_iter()
        .collect::<BTreeSet<_>>();
    if address.ip().is_loopback() {
        hosts.extend([
            format!("127.0.0.1:{}", address.port()),
            format!("localhost:{}", address.port()),
            format!("[::1]:{}", address.port()),
        ]);
    } else if hosts.is_empty() {
        return Err("Non-loopback listen requires server.allowed_hosts in PAT config".into());
    }
    let origins = hosts
        .iter()
        .map(|h| format!("http://{h}"))
        .chain(auth.server.allowed_origins)
        .collect();
    let service = Service {
        db: Arc::new(Mutex::new(Database {
            store,
            clients: BTreeMap::new(),
        })),
        config,
        hosts,
        origins,
        capacity: Arc::new(tokio::sync::Semaphore::new(64)),
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let _keep_alive = service.clone();
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::from_std(socket)?;
        eprintln!("YYproject MCP Streamable HTTP: http://{address}/mcp");
        axum::serve(
            listener,
            Router::new()
                .route("/mcp", any(endpoint))
                .with_state(service),
        )
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
    })?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transport_rejects_batches_invalid_ids_and_ambiguous_response_messages() {
        assert!(!valid_message(&json!([])));
        assert!(!valid_message(
            &json!({"jsonrpc":"2.0","id":null,"method":"ping"})
        ));
        assert!(!valid_message(
            &json!({"jsonrpc":"2.0","id":1,"result":{},"error":{}})
        ));
        assert!(valid_message(
            &json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        ));
        assert!(valid_message(&json!({"jsonrpc":"2.0","id":1,"result":{}})));
        let mut headers = HeaderMap::new();
        headers.append("mcp-session-id", "one".parse().unwrap());
        headers.append("mcp-session-id", "two".parse().unwrap());
        assert!(one(&headers, "mcp-session-id").is_err());
    }
}
