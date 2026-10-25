use serde_json::{Value, json};
use std::io::{self, BufRead, Read, Write};
use yy_project::{Change, Store};
const MAX_MESSAGE: usize = 2 * 1024 * 1024;
fn schema(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn tools() -> Value {
    let id = json!({"type":"integer","minimum":1});
    let date = json!({"type":"string","format":"date"});
    let mut tools = vec![];
    for (name, description, input, read_only) in [
        (
            "project_list",
            "List projects and their current revisions",
            schema(json!({}), &[]),
            true,
        ),
        (
            "project_get",
            "Read a complete project: tasks/issues, masters, dependencies and recorded progress. Re-read after a revision conflict.",
            schema(json!({"project_id":id}), &["project_id"]),
            true,
        ),
        (
            "project_create",
            "Create a project. Dates are ISO YYYY-MM-DD. The database path is fixed by the server command line.",
            schema(
                json!({"name":{"type":"string"},"start":date,"finish":date,"actor":{"type":"string"}}),
                &["name", "start", "finish", "actor"],
            ),
            false,
        ),
        (
            "project_change",
            "Atomically change a project using expected_revision from project_get. A conflict never overwrites GUI or other AI edits. Task effort is hours; duration_days is working days. Master kinds: category, assignee, holiday. New task/master id is 0. Task status: todo, doing, blocked, done, cancelled; kind: task or issue. Omitted task fields use defaults; read existing task before replacing it.",
            schema(
                json!({"project_id":id,"expected_revision":id,"actor":{"type":"string"},"change":{"oneOf":[
                    schema(json!({"operation":{"const":"save_task"},"task":{"type":"object","properties":{"id":{"type":"integer","minimum":0},"title":{"type":"string"},"description":{"type":"string"},"kind":{"enum":["task","issue"]},"status":{"enum":["todo","doing","blocked","done","cancelled"]},"assignee":{"type":["integer","null"]},"category":{"type":["integer","null"]},"start":{"type":["string","null"],"format":"date"},"due":{"type":["string","null"],"format":"date"},"duration_days":{"type":"integer","minimum":0,"maximum":3660},"estimate":{"type":"number","minimum":0},"progress":{"type":"integer","minimum":0,"maximum":100}},"required":["title"],"additionalProperties":false}}),&["operation","task"]),
                    schema(json!({"operation":{"const":"remove_task"},"id":id}),&["operation","id"]),
                    schema(json!({"operation":{"const":"save_master"},"master":{"type":"object","properties":{"id":{"type":"integer","minimum":0},"name":{"type":"string"},"kind":{"enum":["category","assignee","holiday"]},"date":{"type":["string","null"],"format":"date"}},"required":["id","name","kind"],"additionalProperties":false}}),&["operation","master"]),
                    schema(json!({"operation":{"const":"remove_master"},"id":id}),&["operation","id"]),
                    schema(json!({"operation":{"const":"add_dependency"},"dependency":{"type":"object","properties":{"predecessor":id,"successor":id,"lag_days":{"type":"integer","minimum":0,"maximum":3660}},"required":["predecessor","successor"],"additionalProperties":false}}),&["operation","dependency"]),
                    schema(json!({"operation":{"const":"remove_dependency"},"predecessor":id,"successor":id}),&["operation","predecessor","successor"]),
                    schema(json!({"operation":{"const":"rename"},"name":{"type":"string"},"start":date,"finish":date}),&["operation","name","start","finish"]),
                    schema(json!({"operation":{"const":"schedule"}}),&["operation"]),
                    schema(json!({"operation":{"const":"capture"},"day":date}),&["operation","day"])
                ]}}),
                &["project_id", "expected_revision", "actor", "change"],
            ),
            false,
        ),
        (
            "task_list",
            "Page tasks/issues. Optional status and kind filters. Page size is capped at 200.",
            schema(
                json!({"project_id":id,"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":200},"status":{"enum":["todo","doing","blocked","done","cancelled"]},"kind":{"enum":["task","issue"]}}),
                &["project_id"],
            ),
            true,
        ),
        (
            "audit_list",
            "Read up to 100 committed changes, ordered by audit ID. Use the last id as after for the next page.",
            schema(
                json!({"project_id":id,"after":{"type":"integer","minimum":0}}),
                &["project_id"],
            ),
            true,
        ),
    ] {
        tools.push(json!({"name":name,"description":description,"inputSchema":input,"annotations":{"readOnlyHint":read_only,"destructiveHint":!read_only,"openWorldHint":false}}));
    }
    json!({"tools":tools})
}
fn call(store: &mut Store, name: &str, args: Value) -> Result<Value, Box<dyn std::error::Error>> {
    if !args.is_object() {
        return Err("Tool arguments must be an object".into());
    }
    let allowed: &[&str] = match name {
        "project_list" => &[],
        "project_get" => &["project_id"],
        "task_list" => &["project_id", "offset", "limit", "status", "kind"],
        "audit_list" => &["project_id", "after"],
        "project_create" => &["name", "start", "finish", "actor"],
        "project_change" => &["project_id", "expected_revision", "actor", "change"],
        _ => return Err("Unknown tool".into()),
    };
    if args
        .as_object()
        .unwrap()
        .keys()
        .any(|k| !allowed.contains(&k.as_str()))
    {
        return Err("Unknown tool argument".into());
    }
    for key in ["offset", "after"] {
        if args.get(key).is_some_and(|n| n.as_u64().is_none()) {
            return Err("Paging values must be nonnegative integers".into());
        }
    }
    if args
        .get("limit")
        .is_some_and(|n| n.as_u64().is_none_or(|n| !(1..=200).contains(&n)))
    {
        return Err("Page limit must be 1..200".into());
    }
    if let Some(s) = args.get("status") {
        serde_json::from_value::<yy_project::Status>(s.clone())?;
    }
    if let Some(s) = args.get("kind") {
        serde_json::from_value::<yy_project::Kind>(s.clone())?;
    }
    let id = || {
        args["project_id"]
            .as_i64()
            .filter(|n| *n > 0)
            .ok_or("project_id must be positive")
    };
    Ok(match name {
        "project_list" => {
            json!({"projects":store.list()?.into_iter().map(|(id,name,revision)|json!({"id":id,"name":name,"revision":revision})).collect::<Vec<_>>()})
        }
        "project_get" => serde_json::to_value(store.get(id()?)?)?,
        "project_create" => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Create {
                name: String,
                start: chrono::NaiveDate,
                finish: chrono::NaiveDate,
                actor: String,
            }
            let a: Create = serde_json::from_value(args)?;
            serde_json::to_value(store.create(a.name, a.start, a.finish, &a.actor)?)?
        }
        "project_change" => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Apply {
                project_id: i64,
                expected_revision: i64,
                actor: String,
                change: Change,
            }
            let a: Apply = serde_json::from_value(args)?;
            serde_json::to_value(store.apply(
                a.project_id,
                a.expected_revision,
                &a.change,
                &a.actor,
            )?)?
        }
        "task_list" => {
            let p = store.get(id()?)?;
            let offset = args["offset"].as_u64().unwrap_or(0).min(usize::MAX as u64) as usize;
            let limit = args["limit"].as_u64().unwrap_or(100).clamp(1, 200) as usize;
            let rows = p
                .tasks
                .iter()
                .filter(|t| {
                    args.get("status")
                        .is_none_or(|v| serde_json::to_value(t.status).is_ok_and(|s| s == *v))
                        && args
                            .get("kind")
                            .is_none_or(|v| serde_json::to_value(t.kind).is_ok_and(|s| s == *v))
                })
                .collect::<Vec<_>>();
            json!({"revision":p.revision,"total":rows.len(),"tasks":rows.into_iter().skip(offset).take(limit).collect::<Vec<_>>()})
        }
        "audit_list" => json!({"changes":store.audit(id()?,args["after"].as_i64().unwrap_or(0))?}),
        _ => return Err("Unknown tool".into()),
    })
}
pub fn serve(store: &mut Store, mut input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    let mut initialized = false;
    let mut ready = false;
    loop {
        let mut line = vec![];
        let count = input
            .by_ref()
            .take((MAX_MESSAGE + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        let response = if count > MAX_MESSAGE {
            while !line.ends_with(b"\n") {
                line.clear();
                if input
                    .by_ref()
                    .take(MAX_MESSAGE as u64)
                    .read_until(b'\n', &mut line)?
                    == 0
                {
                    break;
                }
            }
            Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Message exceeds 2 MiB"}}),
            )
        } else {
            match serde_json::from_slice::<Value>(&line) {
                Err(_) => Some(
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Invalid JSON"}}),
                ),
                Ok(request)
                    if !request.is_object()
                        || request["jsonrpc"] != "2.0"
                        || !request["method"].is_string()
                        || request.get("id").is_some_and(|id| {
                            !(id.is_null() || id.is_string() || id.is_i64() || id.is_u64())
                        }) =>
                {
                    Some(
                        json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid JSON-RPC request"}}),
                    )
                }
                Ok(request) => {
                    let id = request.get("id").cloned();
                    let method = request["method"].as_str().unwrap_or("");
                    if id.is_none() {
                        if method == "notifications/initialized" && initialized {
                            ready = true;
                        }
                        None
                    } else {
                        let result = match method {
                            "initialize"
                                if !request["params"]["protocolVersion"].is_string()
                                    || !request["params"]["capabilities"].is_object()
                                    || !request["params"]["clientInfo"]["name"].is_string()
                                    || !request["params"]["clientInfo"]["version"].is_string() =>
                            {
                                Err((-32602, "Invalid initialization parameters"))
                            }
                            "initialize" if !initialized => {
                                initialized = true;
                                let requested =
                                    request["params"]["protocolVersion"].as_str().unwrap_or("");
                                let version = if ["2025-11-25", "2025-06-18", "2024-11-05"]
                                    .contains(&requested)
                                {
                                    requested
                                } else {
                                    "2025-11-25"
                                };
                                Ok(
                                    json!({"protocolVersion":version,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"yyproject","version":env!("CARGO_PKG_VERSION")},"instructions":"Use project_list/get before changing a project. Changes require its expected_revision. Database path and read-only mode are fixed by the person launching this server."}),
                                )
                            }
                            "ping" => Ok(json!({})),
                            _ if !ready => Err((
                                -32000,
                                "Complete initialize and notifications/initialized first",
                            )),
                            "tools/list" => Ok(tools()),
                            "tools/call" => {
                                let params = &request["params"];
                                match call(
                                    store,
                                    params["name"].as_str().unwrap_or(""),
                                    params.get("arguments").cloned().unwrap_or(json!({})),
                                ) {
                                    Ok(data) => Ok(
                                        json!({"content":[{"type":"text","text":data.to_string()}],"structuredContent":data,"isError":false}),
                                    ),
                                    Err(error) => Ok(
                                        json!({"content":[{"type":"text","text":error.to_string()}],"isError":true}),
                                    ),
                                }
                            }
                            _ => Err((-32601, "Method not found")),
                        };
                        Some(match result {
                            Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
                            Err((code, message)) => {
                                json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
                            }
                        })
                    }
                }
            }
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut output, &response)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session(store: &mut Store, requests: Vec<Value>) -> Vec<Value> {
        let bytes = requests
            .into_iter()
            .map(|v| v.to_string() + "\n")
            .collect::<String>();
        let mut out = vec![];
        serve(store, std::io::Cursor::new(bytes), &mut out).unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }
    fn init() -> Vec<Value> {
        vec![
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        ]
    }
    #[test]
    fn lifecycle_schema_writes_errors_and_revisions_share_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("test"), false).unwrap();
        let mut requests = vec![json!({"jsonrpc":"2.0","id":0,"method":"tools/list"})];
        requests.extend(init());
        requests.extend([
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"project_create","arguments":{"name":"MCP project","start":"2026-01-01","finish":"2026-12-31","actor":"AI"}}}),
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"project_change","arguments":{"project_id":1,"expected_revision":1,"actor":"AI","change":{"operation":"save_task","task":{"title":"Unicode 日本語","estimate":8}}}}}),
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"project_change","arguments":{"project_id":1,"expected_revision":1,"actor":"stale","change":{"operation":"remove_task","id":1}}}}),
            json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"task_list","arguments":{"project_id":1,"limit":1}}}),
            json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"task_list","arguments":{"project_id":1,"limit":-1}}}),
        ]);
        let out = session(&mut store, requests);
        assert_eq!(out.len(), 8);
        assert_eq!(out[0]["error"]["code"], -32000);
        assert_eq!(out[1]["result"]["protocolVersion"], "2025-11-25");
        assert_eq!(out[2]["result"]["tools"].as_array().unwrap().len(), 6);
        assert_eq!(out[4]["result"]["structuredContent"]["revision"], 2);
        assert_eq!(out[5]["result"]["isError"], true);
        assert_eq!(
            out[6]["result"]["structuredContent"]["tasks"][0]["title"],
            "Unicode 日本語"
        );
        assert_eq!(out[7]["result"]["isError"], true);
        assert_eq!(store.get(1).unwrap().tasks.len(), 1);
    }
    #[test]
    fn malformed_and_oversized_input_resynchronizes_without_stdout_logs() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("test"), false).unwrap();
        let mut input = b"not-json\n{\"jsonrpc\":\"1.0\",\"id\":2,\"method\":\"ping\"}\n".to_vec();
        input.extend(vec![b' '; MAX_MESSAGE + 32]);
        input.extend(b"\n{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ping\"}\n");
        let mut out = vec![];
        serve(&mut store, std::io::Cursor::new(input), &mut out).unwrap();
        let rows = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str::<Value>(s).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0]["error"]["code"], -32700);
        assert_eq!(rows[1]["error"]["code"], -32600);
        assert_eq!(rows[2]["error"]["code"], -32600);
        assert_eq!(rows[3]["result"], json!({}));
    }
}
