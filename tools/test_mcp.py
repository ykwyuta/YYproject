"""Exercise the real stdio executable with bounded waits and isolated SQLite files."""
from contextlib import closing
import json
import shutil
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile


def exchange(executable, database, messages, read_only=False):
    args = [str(executable), "--mcp", str(database)]
    if read_only:
        args.append("--read-only")
    result = subprocess.run(args, input="".join(json.dumps(m, ensure_ascii=False) + "\n" for m in messages),
                            text=True, encoding="utf-8", capture_output=True, timeout=30, check=True)
    assert not result.stderr, result.stderr
    return [json.loads(line) for line in result.stdout.splitlines()]


def request(identifier, method, params=None):
    return {"jsonrpc": "2.0", "id": identifier, "method": method, "params": params or {}}


def initialize():
    return [request(1, "initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                                      "clientInfo": {"name": "yyproject-test", "version": "1"}}),
            {"jsonrpc": "2.0", "method": "notifications/initialized"}]


def tool(identifier, name, arguments):
    return request(identifier, "tools/call", {"name": name, "arguments": arguments})


def main():
    executable = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="yyproject-mcp-") as directory:
        database = Path(directory) / "日本語 project.sqlite3"
        messages = initialize() + [request(2, "tools/list"),
            tool(3, "project_create", {"name": "MCP 実接続", "start": "2026-01-01", "finish": "2026-12-31", "actor": "test"}),
            tool(4, "project_change", {"project_id": 1, "expected_revision": 1, "actor": "AI", "change": {
                "operation": "save_task", "task": {"title": "実プロセスから保存", "estimate": 8, "duration_days": 2}}}),
            tool(5, "project_change", {"project_id": 1, "expected_revision": 1, "actor": "stale", "change": {
                "operation": "remove_task", "id": 1}}),
            tool(6, "task_list", {"project_id": 1, "limit": 1}),
            tool(7, "audit_list", {"project_id": 1})]
        rows = {row["id"]: row for row in exchange(executable, database, messages)}
        assert rows[1]["result"]["protocolVersion"] == "2025-11-25"
        assert len(rows[2]["result"]["tools"]) == 9
        assert rows[4]["result"]["structuredContent"]["revision"] == 2
        assert rows[5]["result"]["isError"]
        assert rows[6]["result"]["structuredContent"]["tasks"][0]["title"] == "実プロセスから保存"
        assert rows[7]["result"]["structuredContent"]["changes"][-1]["actor"] == "AI"
        readonly = exchange(executable, database, initialize() + [tool(8, "project_change", {
            "project_id": 1, "expected_revision": 2, "actor": "readonly", "change": {"operation": "remove_task", "id": 1}})], True)
        assert readonly[-1]["result"]["isError"]
        with closing(sqlite3.connect(database)) as connection:
            revision, document = connection.execute("SELECT revision,document FROM projects WHERE id=1").fetchone()
            assert revision == 2 and len(json.loads(document)["tasks"]) == 1
        preview = exchange(executable,database,initialize()+[tool(30,"removal_preview",{"project_id":1,"task_id":1})],True)[-1]["result"]["structuredContent"]
        assert preview["revision"]==2 and preview["task"]["id"]==1
        operations=exchange(executable,database,initialize()+[
            tool(31,"project_change",{"project_id":1,"expected_revision":2,"actor":"archive","change":{"operation":"remove_task","id":1}}),
            tool(32,"task_list",{"project_id":1}),
            tool(33,"task_list",{"project_id":1,"archived":True}),
            tool(34,"project_change",{"project_id":1,"expected_revision":3,"actor":"restore","change":{"operation":"restore_task","id":1}})])
        results={r["id"]:r["result"]["structuredContent"] for r in operations if r["id"] >= 31}
        assert results[31]["tasks"][0]["archived"]
        assert results[32]["total"]==0 and results[33]["total"]==1
        assert not results[34]["tasks"][0]["archived"] and results[34]["revision"]==4
        print("Read-only archive impact and actual MCP archive/filter/restore: OK")
        demo = Path(directory) / "YY-demo.sqlite3"
        shutil.copy2(Path(__file__).resolve().parents[1] / "demo/yyproject-demo.sqlite3", demo)
        demo_rows = {row["id"]: row for row in exchange(executable, demo, initialize() + [
            tool(10, "project_get", {"project_id": 1}),
            tool(11, "task_list", {"project_id": 1, "kind": "issue"})], True)}
        project = demo_rows[10]["result"]["structuredContent"]
        assert len(project["tasks"]) == 64 and len(project["history"]) == 8
        assert len(demo_rows[11]["result"]["structuredContent"]["tasks"]) == 16
        queries = exchange(executable, demo, initialize() + [
            tool(20, "schedule_preview", {"project_id": 1}),
            tool(21, "task_list", {"project_id": 1, "query": "[ISS-001]", "kind": "issue"}),
            tool(22, "task_list", {"project_id": 1, "unassigned": True, "incomplete": True}),
            tool(23, "task_list", {"project_id": 1, "unassigned": "yes"}),
            tool(24, "task_list", {"project_id": 1, "assignee": -1}),
            tool(25, "project_get", {"project_id": 1})], True)
        results = {row["id"]: row["result"] for row in queries}
        assert results[20]["structuredContent"]["revision"] == project["revision"]
        assert results[21]["structuredContent"]["total"] == 1
        assert results[22]["structuredContent"]["total"] == 2
        assert results[23]["isError"] and results[24]["isError"]
        assert results[25]["structuredContent"] == project
        print("Read-only preview and composed filters, invalid arguments: OK")
        print("Bundled demo: MCP project and issues: OK")
        print("MCP stdio initialize, discovery, Unicode task persistence, conflict, audit, read-only: OK")
        from test_cases import exercise_cases
        exercise_cases(executable,Path(directory)/'cases.sqlite3')


if __name__ == "__main__":
    main()
