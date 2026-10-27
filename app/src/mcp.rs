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
            "Atomically change a project using expected_revision from project_get. A conflict never overwrites GUI or other AI edits. Task effort is hours; duration_days is working days. Master kinds: category, assignee, holiday. New task/master id is 0. Task status: todo, doing, blocked, done, cancelled; kind: task, bug, issue, inquiry or workaround. Investigation data is in case; inquiry/workaround data uses its matching object. Record manual exchanges with save_exchange/remove_exchange; no delivery or attachments. Omitted task fields use defaults; read existing task before replacing it.",
            schema(
                json!({"project_id":id,"expected_revision":id,"actor":{"type":"string"},"change":{"oneOf":[
                    schema(json!({"operation":{"const":"save_task"},"task":{"type":"object","properties":{"id":{"type":"integer","minimum":0},"title":{"type":"string"},"description":{"type":"string"},"kind":{"enum":["task","issue","bug","inquiry","workaround"]},"status":{"enum":["todo","doing","blocked","done","cancelled"]},"assignee":{"type":["integer","null"]},"category":{"type":["integer","null"]},"start":{"type":["string","null"],"format":"date"},"due":{"type":["string","null"],"format":"date"},"duration_days":{"type":"integer","minimum":0,"maximum":3660},"estimate":{"type":"number","minimum":0},"progress":{"type":"integer","minimum":0,"maximum":100},"priority":{"enum":["low","normal","high","critical"]},"severity":{"enum":["unspecified","minor","major","critical"]},"review":{"enum":["not_required","pending","approved","changes_requested"]},"reviewer":{"type":["integer","null"]},"review_note":{"type":"string"},"include_in_effort":{"type":"boolean","default":true},"related_tasks":{"type":"array","items":id,"uniqueItems":true,"maxItems":1000},"archived":{"type":"boolean","default":false}},"required":["title"],"additionalProperties":false}}),&["operation","task"]),
                    schema(json!({"operation":{"const":"remove_task"},"id":id}),&["operation","id"]),
                    schema(json!({"operation":{"const":"restore_task"},"id":id}),&["operation","id"]),
                    schema(json!({"operation":{"const":"save_master"},"master":{"type":"object","properties":{"id":{"type":"integer","minimum":0},"name":{"type":"string"},"kind":{"enum":["category","assignee","holiday"]},"date":{"type":["string","null"],"format":"date"}},"required":["id","name","kind"],"additionalProperties":false}}),&["operation","master"]),
                    schema(json!({"operation":{"const":"remove_master"},"id":id}),&["operation","id"]),
                    schema(json!({"operation":{"const":"add_dependency"},"dependency":{"type":"object","properties":{"predecessor":id,"successor":id,"lag_days":{"type":"integer","minimum":0,"maximum":3660}},"required":["predecessor","successor"],"additionalProperties":false}}),&["operation","dependency"]),
                    schema(json!({"operation":{"const":"remove_dependency"},"predecessor":id,"successor":id}),&["operation","predecessor","successor"]),
                    schema(json!({"operation":{"const":"rename"},"name":{"type":"string"},"start":date,"finish":date}),&["operation","name","start","finish"]),
                    schema(json!({"operation":{"const":"schedule"},"allow_overrun":{"type":"boolean","default":false}}),&["operation"]),
                    schema(json!({"operation":{"const":"capture"},"day":date}),&["operation","day"])
                ]}}),
                &["project_id", "expected_revision", "actor", "change"],
            ),
            false,
        ),
        (
            "removal_preview",
            "Read-only archive impact: task, retained dependencies and related issues. remove_task archives without erasing data; restore_task reactivates it. Recheck expected_revision before applying.",
            schema(
                json!({"project_id":id,"task_id":id}),
                &["project_id", "task_id"],
            ),
            true,
        ),
        (
            "schedule_preview",
            "Read-only schedule differences and task IDs exceeding project finish. Apply schedule with expected_revision after review; allow_overrun defaults to false.",
            schema(json!({"project_id":id}), &["project_id"]),
            true,
        ),
        (
            "task_list",
            "Page tasks, bugs, issues, inquiries and workarounds. Optional query, status, kind, assignee, category, unassigned, incomplete, overdue_before, priority, severity, review and archived filters (AND). overdue_before excludes done/cancelled. archived defaults to false; true selects archived items only. Page size is capped at 200.",
            schema(
                json!({"project_id":id,"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":200},"status":{"enum":["todo","doing","blocked","done","cancelled"]},"kind":{"enum":["task","issue","bug","inquiry","workaround"]},"query":{"type":"string"},"assignee":id,"category":id,"unassigned":{"type":"boolean"},"incomplete":{"type":"boolean"},"overdue_before":date,"priority":{"enum":["low","normal","high","critical"]},"severity":{"enum":["unspecified","minor","major","critical"]},"review":{"enum":["not_required","pending","approved","changes_requested"]},"archived":{"type":"boolean","default":false}}),
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
    let mut filters =
        tools.iter().find(|t| t["name"] == "task_list").unwrap()["inputSchema"]["properties"]
            .clone();
    for key in ["project_id", "offset", "limit"] {
        filters.as_object_mut().unwrap().remove(key);
    }
    filters["columns"] = json!({"type":"object","additionalProperties":{"type":"string"}});
    tools.push(json!({"name":"case_statistics","description":"Read bug/issue daily convergence counts, cause/phase/difficulty distributions and mean resolution hours. UTC calendar days; reopening adds open items again; cancelled is separate. Current filters/attributes define the cohort. Undated legacy items are reported separately, not assigned invented dates. At most 3660 days.","inputSchema":schema(json!({"project_id":id,"kind":{"enum":["bug","issue"]},"from":date,"to":date,"filter":schema(filters.clone(),&[])}),&["project_id","kind","from","to"]),"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}}));
    tools.push(json!({"name":"management_statistics","description":"Read-only inquiry/workaround convergence, team/classification/status counts, overdue and unanswered/not-applied counts, period exchange counts and mean response/temporary/permanent hours. UTC days; current filters and classifications define the cohort. No messages are sent.","inputSchema":schema(json!({"project_id":id,"kind":{"enum":["inquiry","workaround"]},"from":date,"to":date,"filter":schema(filters.clone(),&[])}),&["project_id","kind","from","to"]),"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}}));
    tools.push(json!({"name":"csv_export","description":"Read-only CSV content with selected column keys and task filters, including investigation/inquiry/workaround fields and manual exchange logs. Returns UTF-8 CSV text with BOM and CRLF; the client saves it locally. Does not access server filesystem paths.","inputSchema":schema(json!({"project_id":id,"columns":{"type":"array","minItems":1,"uniqueItems":true,"items":{"enum":yy_project::table::all_columns().map(|c|c.key).collect::<std::collections::BTreeSet<_>>()}},"filter":schema(filters.clone(),&[])}),&["project_id","columns"]),"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}}));
    for tool in &mut tools {
        if tool["name"] == "project_change" {
            let variants = tool["inputSchema"]["properties"]["change"]["oneOf"]
                .as_array_mut()
                .unwrap();
            variants[0]["properties"]["task"]["properties"]["parent"] =
                json!({"type":["integer","null"],"minimum":1});
            variants[0]["properties"]["task"]["properties"]["milestone"] =
                json!({"type":"boolean","default":false});
            variants[3]["properties"]["master"]["properties"]["parent"] =
                json!({"type":["integer","null"],"minimum":1});
            variants[0]["properties"]["task"]["properties"]["completed_on"] =
                json!({"type":["string","null"],"format":"date"});
            let mut case_properties = serde_json::Map::new();
            for (key, value) in serde_json::to_value(yy_project::cases::CaseData::default())
                .unwrap()
                .as_object()
                .unwrap()
            {
                case_properties.insert(key.clone(),if key=="transitions" {json!({"type":"array","readOnly":true,"description":"Server maintained; supplied history is ignored on save","items":schema(json!({"at":{"type":"string","format":"date-time"},"status":{"enum":["todo","doing","blocked","done","cancelled"]}}),&["at","status"])})} else if key.ends_with("_at"){json!({"type":["string","null"],"format":"date-time"})}else if key.ends_with("_difficulty"){json!({"enum":["unspecified","low","medium","high","very_high"]})}else{let _=value;json!({"type":"string"})});
            }
            let mut case_schema = schema(Value::Object(case_properties), &[]);
            case_schema["type"] = json!(["object", "null"]);
            variants[0]["properties"]["task"]["properties"]["case"] = case_schema;
            for (key, default) in [
                (
                    "inquiry",
                    serde_json::to_value(yy_project::work::Inquiry::default()).unwrap(),
                ),
                (
                    "workaround",
                    serde_json::to_value(yy_project::work::Workaround::default()).unwrap(),
                ),
            ] {
                let mut properties = serde_json::Map::new();
                for (field, _) in default.as_object().unwrap() {
                    let spec=match field.as_str(){
                        "kind"=>json!({"enum":["request","question"]}),
                        "next_exchange_id"=>json!({"type":"integer","minimum":1,"readOnly":true}),
                        "transitions"=>variants[0]["properties"]["task"]["properties"]["case"]["properties"]["transitions"].clone(),
                        "exchanges"=>json!({"type":"array","readOnly":true,"description":"Use save_exchange/remove_exchange; replacement writes preserve saved entries","items":{"type":"object"}}),
                        k if k.ends_with("_at")=>json!({"type":["string","null"],"format":"date-time"}),
                        k if k.ends_with("_on")=>json!({"type":["string","null"],"format":"date"}),
                        _=>json!({"type":"string"})};
                    properties.insert(field.clone(), spec);
                }
                let mut object = schema(Value::Object(properties), &[]);
                object["type"] = json!(["object", "null"]);
                variants[0]["properties"]["task"]["properties"][key] = object;
            }
            variants.push(schema(json!({"operation":{"const":"save_exchange"},"id":id,"entry":schema(json!({"id":{"type":"integer","minimum":0},"at":{"type":["string","null"],"format":"date-time"},"direction":{"enum":["outgoing","incoming","note"]},"person":{"type":"string"},"text":{"type":"string","minLength":1,"maxLength":65536}}),&["text"])}),&["operation","id","entry"]));
            variants.push(schema(
                json!({"operation":{"const":"remove_exchange"},"id":id,"entry_id":id}),
                &["operation", "id", "entry_id"],
            ));
            let master = variants[3]["properties"]["master"].clone();
            variants.push(schema(json!({"operation":{"const":"save_masters"},"masters":{"type":"array","items":master,"minItems":1,"maxItems":2000}}),&["operation","masters"]));
            let mut task = variants[0]["properties"]["task"].clone();
            task["required"] = json!(["id", "title"]);
            variants.push(schema(json!({"operation":{"const":"save_tasks"},"tasks":{"type":"array","items":task,"minItems":1,"maxItems":2000}}),&["operation","tasks"]));
            variants.push(schema(json!({"operation":{"const":"move_task"},"id":id,"before":{"type":["integer","null"],"minimum":1}}),&["operation","id","before"]));
        }
        if tool["name"] == "task_list" {
            tool["inputSchema"]["properties"]["columns"] =
                json!({"type":"object","additionalProperties":{"type":"string"}});
        }
    }
    json!({"tools":tools})
}
fn call(store: &mut Store, name: &str, args: Value) -> Result<Value, Box<dyn std::error::Error>> {
    if !args.is_object() {
        return Err("Tool arguments must be an object".into());
    }
    let allowed: &[&str] = match name {
        "project_list" => &[],
        "project_get" | "schedule_preview" => &["project_id"],
        "removal_preview" => &["project_id", "task_id"],
        "task_list" => &[
            "project_id",
            "offset",
            "limit",
            "status",
            "kind",
            "query",
            "assignee",
            "category",
            "unassigned",
            "incomplete",
            "overdue_before",
            "priority",
            "severity",
            "review",
            "archived",
            "columns",
        ],
        "audit_list" => &["project_id", "after"],
        "case_statistics" | "management_statistics" => {
            &["project_id", "kind", "from", "to", "filter"]
        }
        "csv_export" => &["project_id", "columns", "filter"],
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
    if let Some(v) = args.get("priority") {
        serde_json::from_value::<yy_project::Priority>(v.clone())?;
    }
    if let Some(v) = args.get("severity") {
        serde_json::from_value::<yy_project::Severity>(v.clone())?;
    }
    if let Some(v) = args.get("review") {
        serde_json::from_value::<yy_project::ReviewState>(v.clone())?;
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
        "case_statistics" => {
            let kind = serde_json::from_value(args["kind"].clone())?;
            let from = serde_json::from_value(args["from"].clone())?;
            let to = serde_json::from_value(args["to"].clone())?;
            let filter = if let Some(v) = args.get("filter") {
                serde_json::from_value(v.clone())?
            } else {
                yy_project::TaskFilter::default()
            };
            serde_json::to_value(yy_project::cases::statistics(
                &store.get(id()?)?,
                kind,
                from,
                to,
                &filter,
            )?)?
        }
        "management_statistics" => {
            let kind = serde_json::from_value(args["kind"].clone())?;
            let from = serde_json::from_value(args["from"].clone())?;
            let to = serde_json::from_value(args["to"].clone())?;
            let filter = if let Some(v) = args.get("filter") {
                serde_json::from_value(v.clone())?
            } else {
                yy_project::TaskFilter::default()
            };
            serde_json::to_value(yy_project::work::statistics(
                &store.get(id()?)?,
                kind,
                from,
                to,
                &filter,
            )?)?
        }
        "csv_export" => {
            let keys = serde_json::from_value::<Vec<String>>(args["columns"].clone())?;
            let filter = if let Some(v) = args.get("filter") {
                serde_json::from_value(v.clone())?
            } else {
                yy_project::TaskFilter::default()
            };
            json!({"mime_type":"text/csv; charset=utf-8","content":yy_project::table::export_csv(&store.get(id()?)?,&filter,&keys)?})
        }
        "schedule_preview" => serde_json::to_value(store.get(id()?)?.schedule_preview()?)?,
        "removal_preview" => serde_json::to_value(
            store.get(id()?)?.removal_preview(
                args["task_id"]
                    .as_i64()
                    .filter(|n| *n > 0)
                    .ok_or("task_id must be positive")?,
            )?,
        )?,
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
            for key in ["assignee", "category"] {
                if args
                    .get(key)
                    .is_some_and(|v| v.as_i64().is_none_or(|id| id <= 0))
                {
                    return Err("Master IDs must be positive integers".into());
                }
            }
            let mut selection = args.clone();
            let object = selection.as_object_mut().unwrap();
            for key in ["project_id", "offset", "limit"] {
                object.remove(key);
            }
            let filter: yy_project::TaskFilter = serde_json::from_value(selection)?;
            filter.validate()?;
            let rows = p
                .ordered_tasks()
                .into_iter()
                .map(|(t, _)| t)
                .filter(|t| p.matches_filter(&filter, t))
                .collect::<Vec<_>>();
            json!({"revision":p.revision,"total":rows.len(),"tasks":rows.into_iter().skip(offset).take(limit).collect::<Vec<_>>()})
        }
        "audit_list" => json!({"changes":store.audit(id()?,args["after"].as_i64().unwrap_or(0))?}),
        _ => return Err("Unknown tool".into()),
    })
}
#[derive(Default)]
pub(crate) struct Session {
    initialized: bool,
    ready: bool,
}
impl Session {
    pub(crate) fn handle(&mut self, store: &mut Store, request: Value) -> Option<Value> {
        match request {
            request
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
            request => {
                let id = request.get("id").cloned();
                let method = request["method"].as_str().unwrap_or("");
                if id.is_none() {
                    if method == "notifications/initialized" && self.initialized {
                        self.ready = true;
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
                        "initialize" if !self.initialized => {
                            self.initialized = true;
                            let requested =
                                request["params"]["protocolVersion"].as_str().unwrap_or("");
                            let version =
                                if ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"]
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
                        _ if !self.ready => Err((
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
    }
}
pub fn serve(store: &mut Store, mut input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    let mut session = Session::default();
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
                Ok(request) => session.handle(store, request),
                Err(_) => Some(
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Invalid JSON"}}),
                ),
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
        assert_eq!(out[2]["result"]["tools"].as_array().unwrap().len(), 11);
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
