//! Embedded human UI, cookie sessions and Store-backed operations.
use crate::{http_server::Service, mcp, pat};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::any,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use yy_project::{Change, Kind, Master, MasterKind, Task, TaskFilter, table};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
struct Session {
    name: String,
    fingerprint: String,
    csrf: String,
    seen: Instant,
    created: Instant,
}
#[derive(Default)]
pub(super) struct Sessions {
    clients: BTreeMap<String, Session>,
    failed: Vec<Instant>,
}
pub(super) fn routes(router: Router<Service>) -> Router<Service> {
    router
        .route("/", any(endpoint))
        .route("/favicon.ico", any(endpoint))
        .route("/web/app.js", any(endpoint))
        .route("/web/app.css", any(endpoint))
        .route("/web/login", any(endpoint))
        .route("/web/logout", any(endpoint))
        .route("/web/api", any(endpoint))
        .route("/web/session", any(endpoint))
        .route("/web/backup", any(endpoint))
}
fn reply(status: StatusCode, value: Value) -> Response {
    (status, Json(value)).into_response()
}
fn failure(status: StatusCode, message: &str) -> Response {
    reply(status, json!({"error":message}))
}
fn secure(mut r: Response) -> Response {
    for (name, value) in [
        ("cache-control", "no-store"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "same-origin"),
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
        ),
    ] {
        r.headers_mut().insert(
            header::HeaderName::from_static(name),
            header::HeaderValue::from_static(value),
        );
    }
    r
}
async fn endpoint(State(s): State<Service>, r: Request) -> Response {
    let (parts, body) = r.into_parts();
    let host = parts
        .headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if !s.hosts.contains(host) {
        return secure(failure(StatusCode::FORBIDDEN, "Host not allowed"));
    }
    if parts.headers.get_all(header::HOST).iter().count() != 1 {
        return secure(failure(StatusCode::FORBIDDEN, "Invalid Host"));
    }
    let path = parts.uri.path().to_owned();
    if parts.method == Method::GET && path == "/favicon.ico" {
        return secure(
            (
                [(header::CONTENT_TYPE, "image/x-icon")],
                include_bytes!("../res/yyproject.ico").as_slice(),
            )
                .into_response(),
        );
    }
    if parts.method == Method::GET && ["/", "/web/app.js", "/web/app.css"].contains(&path.as_str())
    {
        let (content, mime) = match path.as_str() {
            "/web/app.js" => (include_str!("web/app.js"), "text/javascript; charset=utf-8"),
            "/web/app.css" => (include_str!("web/app.css"), "text/css; charset=utf-8"),
            _ => (include_str!("web/index.html"), "text/html; charset=utf-8"),
        };
        return secure(([(header::CONTENT_TYPE, mime)], content).into_response());
    }
    if !(parts.method == Method::POST || (path == "/web/session" && parts.method == Method::GET)) {
        return secure(failure(StatusCode::METHOD_NOT_ALLOWED, "Use POST"));
    }
    if path != "/web/session" {
        let origin = parts
            .headers
            .get(header::ORIGIN)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        if !s.origins.contains(origin) || parts.headers.get_all(header::ORIGIN).iter().count() != 1
        {
            return secure(failure(
                StatusCode::FORBIDDEN,
                "Same-origin request required",
            ));
        }
        if parts
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            != "application/json"
        {
            return secure(failure(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Use application/json",
            ));
        }
    }
    let Ok(permit) = s.capacity.clone().try_acquire_owned() else {
        return secure(failure(StatusCode::SERVICE_UNAVAILABLE, "Busy"));
    };
    let bytes = match tokio::time::timeout(Duration::from_secs(15), to_bytes(body, 2 * 1024 * 1024))
        .await
    {
        Ok(Ok(b)) => b.to_vec(),
        _ => {
            return secure(failure(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Invalid or oversized body",
            ));
        }
    };
    match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        process(&s, &path, &parts.headers, &bytes)
    })
    .await
    {
        Ok(r) => secure(r),
        Err(_) => secure(failure(StatusCode::INTERNAL_SERVER_ERROR, "Request failed")),
    }
}
fn cookie(s: &Service, id: &str) -> String {
    format!(
        "yy_session={id}; Path=/web; HttpOnly; SameSite=Strict; Max-Age={}{}",
        if id.is_empty() { 0 } else { 43200 },
        if s.secure_cookies { "; Secure" } else { "" }
    )
}
fn process(s: &Service, path: &str, headers: &HeaderMap, bytes: &[u8]) -> Response {
    let config = match pat::Config::load(&s.config) {
        Ok(c) => c,
        Err(_) => {
            return failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "Authentication configuration unavailable",
            );
        }
    };
    let value = if path == "/web/session" {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(bytes) {
            Ok(v) if v.is_object() => v,
            _ => return failure(StatusCode::BAD_REQUEST, "Expected JSON object"),
        }
    };
    let Ok(mut sessions) = s.web.lock() else {
        return failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Session worker unavailable",
        );
    };
    sessions.clients.retain(|_, c| {
        c.seen.elapsed() < Duration::from_secs(1800)
            && c.created.elapsed() < Duration::from_secs(43200)
    });
    if path == "/web/login" {
        sessions
            .failed
            .retain(|at| at.elapsed() < Duration::from_secs(60));
        if sessions.failed.len() >= 30 {
            return failure(
                StatusCode::TOO_MANY_REQUESTS,
                "しばらく待って再試行してください",
            );
        }
        let Some(user) = config.authenticate_password(value["password"].as_str().unwrap_or(""))
        else {
            sessions.failed.push(Instant::now());
            return failure(StatusCode::UNAUTHORIZED, "パスワードを確認してください");
        };
        if sessions.clients.len() >= 256 {
            return failure(StatusCode::SERVICE_UNAVAILABLE, "Session limit reached");
        }
        let (Ok(id), Ok(csrf)) = (pat::random_id(), pat::random_id()) else {
            return failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Random generator unavailable",
            );
        };
        let mut r = reply(
            StatusCode::OK,
            json!({"user":user.name,"read_only":user.read_only||s.read_only,"csrf":csrf}),
        );
        sessions.clients.insert(
            id.clone(),
            Session {
                name: user.name,
                fingerprint: user.password_sha256,
                csrf,
                seen: Instant::now(),
                created: Instant::now(),
            },
        );
        r.headers_mut()
            .insert(header::SET_COOKIE, cookie(s, &id).parse().unwrap());
        return r;
    }
    let ids = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .filter_map(|h| h.trim().strip_prefix("yy_session="))
        .collect::<Vec<_>>();
    if ids.len() != 1 {
        return failure(StatusCode::UNAUTHORIZED, "ログインしてください");
    }
    let id = ids[0];
    let Some(session) = sessions.clients.get_mut(id) else {
        return failure(StatusCode::UNAUTHORIZED, "セッションが終了しました");
    };
    let Some(user) = config
        .web_users
        .iter()
        .find(|u| u.name == session.name && u.password_sha256 == session.fingerprint)
    else {
        sessions.clients.remove(id);
        return failure(StatusCode::UNAUTHORIZED, "ログインし直してください");
    };
    if path == "/web/session" {
        return reply(
            StatusCode::OK,
            json!({"user":user.name,"read_only":user.read_only||s.read_only,"csrf":session.csrf}),
        );
    }
    if headers.get_all("x-yy-csrf").iter().count() != 1
        || headers.get("x-yy-csrf").and_then(|v| v.to_str().ok()) != Some(session.csrf.as_str())
    {
        return failure(StatusCode::FORBIDDEN, "Invalid CSRF token");
    }
    session.seen = Instant::now();
    if path == "/web/logout" {
        sessions.clients.remove(id);
        let mut r = reply(StatusCode::OK, json!({}));
        r.headers_mut()
            .insert(header::SET_COOKIE, cookie(s, "").parse().unwrap());
        return r;
    }
    drop(sessions);
    let Ok(mut db) = s.db.lock() else {
        return failure(StatusCode::INTERNAL_SERVER_ERROR, "Database unavailable");
    };
    let mut user = user.clone();
    user.read_only |= s.read_only;
    if path == "/web/backup" {
        let backup = (|| -> Result<Vec<u8>> {
            let dir = tempfile::tempdir()?;
            let path = dir.path().join("backup.sqlite3");
            db.store.export_sqlite(&path)?;
            Ok(std::fs::read(path)?)
        })();
        return match backup {
            Ok(bytes) => (
                [
                    (header::CONTENT_TYPE, "application/vnd.sqlite3"),
                    (
                        header::CONTENT_DISPOSITION,
                        "attachment; filename=yyproject-backup.sqlite3",
                    ),
                ],
                bytes,
            )
                .into_response(),
            Err(e) => failure(StatusCode::BAD_REQUEST, &e.to_string()),
        };
    }
    match dispatch(&mut db.store, &value, &user) {
        Ok(v) => reply(StatusCode::OK, v),
        Err(e) => {
            let status = if matches!(
                e.downcast_ref::<yy_project::Error>(),
                Some(yy_project::Error::Conflict)
            ) {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            };
            failure(status, &e.to_string())
        }
    }
}
fn columns(kind: Kind) -> &'static [table::Column] {
    match kind {
        Kind::Inquiry => table::INQUIRY_COLUMNS,
        Kind::Workaround => table::WORKAROUND_COLUMNS,
        Kind::Bug | Kind::Issue => table::CASE_COLUMNS,
        _ => table::COLUMNS,
    }
}
fn record(p: &yy_project::Project, t: &Task) -> Value {
    json!({"id":t.id,"values":columns(t.kind).iter().map(|c|(c.key,table::value(p,t,c.key))).collect::<BTreeMap<_,_>>()})
}
fn metadata() -> Value {
    json!({"columns":{"task":table::COLUMNS,"bug":table::CASE_COLUMNS,"issue":table::CASE_COLUMNS,"inquiry":table::INQUIRY_COLUMNS,"workaround":table::WORKAROUND_COLUMNS},"masters":{"category":table::master_columns(MasterKind::Category),"assignee":table::master_columns(MasterKind::Assignee),"holiday":table::master_columns(MasterKind::Holiday)},"bulk":table::BULK_TASK_KEYS,
    "enums":{"status":yy_project::Status::ALL.iter().map(|s|json!([s,s.label()])).collect::<Vec<_>>(),"priority":yy_project::Priority::ALL.iter().map(|s|json!([s,s.label()])).collect::<Vec<_>>(),"severity":yy_project::Severity::ALL.iter().map(|s|json!([s,s.label()])).collect::<Vec<_>>(),"review":yy_project::ReviewState::ALL.iter().map(|s|json!([s,s.label()])).collect::<Vec<_>>(),"difficulty":yy_project::cases::Difficulty::ALL.iter().map(|s|json!([s,s.label()])).collect::<Vec<_>>()},"jira_columns":table::JIRA_COLUMNS})
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GridEdit {
    project_id: i64,
    expected_revision: i64,
    ids: Vec<i64>,
    keys: Vec<String>,
    text: String,
    #[serde(default)]
    master: Option<MasterKind>,
    #[serde(default)]
    fill_down: bool,
}
fn grid_change(p: &yy_project::Project, g: &GridEdit) -> Result<Change> {
    let rows = table::decode(&g.text, '\t')?;
    if g.ids.is_empty() || g.ids.len() > 2000 || g.keys.is_empty() || rows.is_empty() {
        return Err("Empty or oversized selection".into());
    }
    if rows.iter().any(|r| r.len() != rows[0].len())
        || g.keys
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != g.keys.len()
    {
        return Err("Clipboard must be rectangular and columns unique".into());
    }
    if rows.len() != g.ids.len() && !(rows.len() == 1 && (g.fill_down || rows[0].len() == 1)) {
        return Err("Clipboard rows exceed selection".into());
    }
    if (g.fill_down || g.ids.len() > 1) && g.master.is_none() {
        table::validate_bulk_keys(g.keys.iter().map(String::as_str))?;
    }
    if let Some(kind) = g.master {
        let mut masters = vec![];
        for (r, id) in g.ids.iter().enumerate() {
            let mut m = p
                .masters
                .iter()
                .find(|m| m.id == *id && m.kind == kind)
                .ok_or("Master not found")?
                .clone();
            let values = &rows[r.min(rows.len() - 1)];
            if values.len() != g.keys.len() && values.len() != 1 {
                return Err("Clipboard columns exceed selection".into());
            }
            for (c, key) in g.keys.iter().enumerate() {
                if !table::master_columns(kind)
                    .iter()
                    .any(|c| c.key == key && c.editable)
                {
                    return Err("Column is read-only".into());
                }
                table::set_master_value(&mut m, key, &values[c.min(values.len() - 1)])?;
            }
            masters.push(m);
        }
        Ok(Change::SaveMasters { masters })
    } else {
        let mut tasks = vec![];
        for (r, id) in g.ids.iter().enumerate() {
            let mut t = p
                .tasks
                .iter()
                .find(|t| t.id == *id && !t.archived)
                .ok_or("Task not found")?
                .clone();
            let values = &rows[r.min(rows.len() - 1)];
            if values.len() != g.keys.len() && values.len() != 1 {
                return Err("Clipboard columns exceed selection".into());
            }
            for (c, key) in g.keys.iter().enumerate() {
                if !columns(t.kind).iter().any(|c| c.key == key && c.editable) {
                    return Err("Column is read-only".into());
                }
                table::set_value(p, &mut t, key, &values[c.min(values.len() - 1)])?;
            }
            tasks.push(t);
        }
        Ok(Change::SaveTasks { tasks })
    }
}
fn dispatch(store: &mut yy_project::Store, v: &Value, user: &pat::WebUser) -> Result<Value> {
    let action = v["action"].as_str().ok_or("Action required")?;
    let id = || v["project_id"].as_i64().ok_or("Project ID required");
    let revision = || v["expected_revision"].as_i64().ok_or("Revision required");
    if user.read_only && ["grid", "edit", "edit_master"].contains(&action) {
        return Err("読み取り専用ユーザーです".into());
    }
    match action {
        "metadata" => Ok(metadata()),
        "call" => {
            let name = v["name"].as_str().ok_or("Tool name required")?;
            let read = [
                "project_list",
                "project_get",
                "task_list",
                "audit_list",
                "schedule_preview",
                "removal_preview",
                "case_statistics",
                "management_statistics",
                "csv_export",
            ]
            .contains(&name);
            if !read && (!["project_create", "project_change"].contains(&name) || user.read_only) {
                return Err("Operation not allowed".into());
            }
            let mut args = v["args"].clone();
            if !args.is_object() {
                return Err("Arguments must be an object".into());
            }
            if !read {
                args["actor"] = json!(user.name);
            }
            mcp::call(store, name, args)
        }
        "project" => {
            let p = store.get(id()?)?;
            let filter = serde_json::from_value::<TaskFilter>(
                v.get("filter").cloned().unwrap_or_else(|| json!({})),
            )?;
            filter.validate()?;
            let rows = p
                .ordered_tasks()
                .into_iter()
                .filter(|(t, _)| p.matches_filter(&filter, t))
                .map(|(t, d)| {
                    let mut r = record(&p, t);
                    r["depth"] = json!(d);
                    r
                })
                .collect::<Vec<_>>();
            Ok(
                json!({"rows":rows,"network":p.network()?,"project":p,"master_rows":p.masters.iter().map(|m|json!({"id":m.id,"values":table::master_columns(m.kind).iter().map(|c|(c.key,table::master_value(m,c.key))).collect::<BTreeMap<_,_>>()})).collect::<Vec<_>>()}),
            )
        }
        "draft" => {
            let p = store.get(id()?)?;
            let kind = serde_json::from_value::<Kind>(v["kind"].clone())?;
            Ok(record(
                &p,
                &Task {
                    kind,
                    ..Default::default()
                },
            ))
        }
        "grid" => {
            let g: GridEdit = serde_json::from_value(v["edit"].clone())?;
            let p = store.get(g.project_id)?;
            let change = grid_change(&p, &g)?;
            Ok(serde_json::to_value(store.apply(
                p.id,
                g.expected_revision,
                &change,
                &user.name,
            )?)?)
        }
        "edit" => {
            let p = store.get(id()?)?;
            let item = v["id"].as_i64().unwrap_or(0);
            let kind = serde_json::from_value::<Kind>(v["kind"].clone())?;
            let mut t = if item == 0 {
                Task {
                    kind,
                    parent: serde_json::from_value(
                        v.get("parent").cloned().unwrap_or(Value::Null),
                    )?,
                    include_in_effort: kind == Kind::Task,
                    ..Default::default()
                }
            } else {
                p.tasks
                    .iter()
                    .find(|t| t.id == item && !t.archived)
                    .ok_or("Task not found")?
                    .clone()
            };
            for (key, value) in v["values"].as_object().ok_or("Values required")? {
                if !columns(t.kind).iter().any(|c| c.key == key && c.editable) {
                    return Err("Column is read-only".into());
                }
                table::set_value(
                    &p,
                    &mut t,
                    key,
                    value.as_str().ok_or("Cell value must be text")?,
                )?;
            }
            Ok(serde_json::to_value(store.apply(
                p.id,
                revision()?,
                &Change::SaveTask { task: t },
                &user.name,
            )?)?)
        }
        "edit_master" => {
            let p = store.get(id()?)?;
            let item = v["id"].as_i64().unwrap_or(0);
            let kind = serde_json::from_value::<MasterKind>(v["kind"].clone())?;
            let mut m = if item == 0 {
                Master {
                    id: 0,
                    kind,
                    name: String::new(),
                    parent: None,
                    date: None,
                }
            } else {
                p.masters
                    .iter()
                    .find(|m| m.id == item)
                    .ok_or("Master not found")?
                    .clone()
            };
            for (key, value) in v["values"].as_object().ok_or("Values required")? {
                if !table::master_columns(kind)
                    .iter()
                    .any(|c| c.key == key && c.editable)
                {
                    return Err("Column is read-only".into());
                }
                table::set_master_value(
                    &mut m,
                    key,
                    value.as_str().ok_or("Cell value must be text")?,
                )?;
            }
            Ok(serde_json::to_value(store.apply(
                p.id,
                revision()?,
                &Change::SaveMaster { master: m },
                &user.name,
            )?)?)
        }
        "jira" => {
            let p = store.get(id()?)?;
            Ok(
                json!({"content":table::export_jira_csv(&p,&serde_json::from_value(v["filter"].clone())?,&serde_json::from_value::<Vec<String>>(v["keys"].clone())?,&serde_json::from_value(v["options"].clone())?)?}),
            )
        }
        _ => Err("Unknown Web operation".into()),
    }
}
