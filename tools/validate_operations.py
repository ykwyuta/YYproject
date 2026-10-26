#!/usr/bin/env python3
"""Exercise actual MCP against isolated copies; report supported controls and gaps."""
import argparse
from collections import defaultdict
from datetime import date, timedelta, datetime, timezone
from hashlib import sha256
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
from test_mcp import exchange, initialize, tool

ROOT = Path(__file__).resolve().parents[1]

class Client:
    def __init__(self, exe, path, readonly=False):
        self.exe, self.path, self.readonly = exe, path, readonly
    def raw(self, name, args):
        return exchange(self.exe, self.path, initialize() + [tool(2, name, args)], self.readonly)[-1]["result"]
    def call(self, name, args):
        result = self.raw(name, args)
        if result.get("isError"):
            raise AssertionError(result)
        return result["structuredContent"]
    def get(self, pid=1):
        return self.call("project_get", {"project_id": pid})
    def change(self, project, change, actor="operations-validation", raw=False):
        args = {"project_id": project["id"], "expected_revision": project["revision"], "actor": actor, "change": change}
        return self.raw("project_change", args) if raw else self.call("project_change", args)

def task(project, key):
    return next(t for t in project["tasks"] if t["title"].startswith(f"[{key}]"))
def save(client, project, row):
    return client.change(project, {"operation": "save_task", "task": row})
def remaining(project):
    return sum(t["estimate"] * (100-t["progress"])/100 for t in project["tasks"] if t["status"] not in ("done", "cancelled"))
def completed(project):
    return sum(t["estimate"] for t in project["tasks"] if t["status"] == "done")

def validate(executable, output):
    source = ROOT / "demo/yyproject-demo.sqlite3"
    scenario = json.loads((ROOT / "demo/scenario.json").read_text(encoding="utf-8"))
    findings = []
    def record(code, severity, title, gap, evidence, action):
        findings.append(dict(id=code, severity=severity, title=title, status="gap" if gap else "supported", evidence=evidence, suggested_action=action))
    with tempfile.TemporaryDirectory(prefix="yyproject-operations-") as directory:
        def fresh(label):
            path = Path(directory) / (label + ".sqlite3")
            shutil.copy2(source, path)
            return Client(executable, path)
        client = fresh("inventory")
        listing = client.call("project_list", {})["projects"]
        projects = [client.get(p["id"]) for p in listing]
        assert [len(p["tasks"]) for p in projects] == [64,16,10]
        primary = projects[0]
        as_of = date.fromisoformat(primary["history"][-1]["day"])
        overdue = [dict(project=p["id"], id=t["id"], title=t["title"], due=t["due"]) for p in projects for t in p["tasks"] if t["status"] not in ("done","cancelled") and t["due"] and date.fromisoformat(t["due"]) < as_of]
        unassigned = [dict(project=p["id"], id=t["id"], title=t["title"]) for p in projects for t in p["tasks"] if t["status"] not in ("done","cancelled") and t["assignee"] is None]
        # Planned effort/day, not observed work: no capacity field exists in the model.
        loads = defaultdict(lambda: dict(hours=0., projects=set(), tasks=[]))
        for p in projects:
            names = {m["id"]:m["name"] for m in p["masters"]}
            holidays = {m["date"] for m in p["masters"] if m["kind"]=="holiday"}
            for t in p["tasks"]:
                if t.get("archived",False) or not t.get("include_in_effort",True) or t["status"] in ("done","cancelled") or not t["assignee"] or not t["start"] or not t["due"]: continue
                day = max(as_of,date.fromisoformat(t["start"]))
                end = date.fromisoformat(t["due"])
                workdays=[]
                while day <= end:
                    if day.weekday()<5 and day.isoformat() not in holidays:
                        workdays.append(day)
                    day += timedelta(days=1)
                hours=t["estimate"]*(100-t["progress"])/100/max(1,len(workdays))
                for day in workdays:
                    if day > as_of+timedelta(days=14) or hours == 0: continue
                    row=loads[(names[t["assignee"]],day.isoformat())]
                    row["hours"] += hours
                    row["projects"].add(p["id"]);row["tasks"].append(t["title"])
        over=[]
        for (owner,day),row in loads.items():
            capacity=scenario["role_capacity"].get(owner,6)
            if row["hours"]>capacity:
                over.append(dict(owner=owner,day=day,hours=round(row["hours"],2),capacity=capacity,projects=sorted(row["projects"]),tasks=row["tasks"]))
        over.sort(key=lambda r:r["hours"]/r["capacity"],reverse=True)
        assert overdue and unassigned and any(len(r["projects"])>1 for r in over)
        record("OPS-01","P1","兼務担当者の稼働上限・案件横断の負荷をモデルで表せない",True,{"planned_over_capacity_days":len(over),"examples":over[:8],"assumption":"残工数を基準日以降の残り予定営業日に均等配分した机上比較。実稼働ではない。期限超過の残工数は別集計で、この負荷に含めない。"},"稼働カレンダー・配賦率・案件横断集計と、過負荷時の警告を設計する")
        filtered_overdue=[];filtered_unassigned=[]
        for project in projects:
            filtered_overdue += client.call("task_list", {"project_id":project["id"],"overdue_before":as_of.isoformat()})["tasks"]
            filtered_unassigned += client.call("task_list", {"project_id":project["id"],"unassigned":True,"incomplete":True})["tasks"]
        assert len(filtered_overdue)==len(overdue) and len(filtered_unassigned)==len(unassigned)
        record("OPS-02","P1","期限超過・未割当を共通条件で絞り込める",False,{"overdue_count":len(filtered_overdue),"unassigned_count":len(filtered_unassigned),"gui_scope":"カンバン・タスク一覧・課題一覧"},"保存ビューと並べ替え、案件横断表示は引き続き改善対象")
        # Correctness controls use real separate MCP processes and expected_revision.
        client=fresh("concurrency");p=client.get(); stale=json.loads(json.dumps(p))
        row=task(p,"EDT-03");row["description"]+="\nGUI役が会議メモを追記";p=save(client,p,row)
        other=task(stale,"SHT-03");other["description"]+="\nAI役が別項目を更新"
        error=client.change(stale,{"operation":"save_task","task":other},raw=True)
        assert error.get("isError") and client.get()["revision"]==p["revision"]
        record("SAFE-01","control","古い版での上書きを拒否する",False,{"first_revision":stale["revision"],"current_revision":p["revision"],"second_update_rejected":True},"安全性は維持する。別項目でも競合するため、再読込・差分表示・再適用のUXを改善する")
        before=p;row=task(p,"EDT-03");row["assignee"]=999999
        assert client.change(p,{"operation":"save_task","task":row},raw=True).get("isError")
        edge=p["dependencies"][0]
        assert client.change(p,{"operation":"add_dependency","dependency":{"predecessor":edge["successor"],"successor":edge["predecessor"],"lag_days":0}},raw=True).get("isError")
        assert client.get()["revision"]==before["revision"]
        readonly=Client(executable,client.path,True)
        assert readonly.change(p,{"operation":"remove_task","id":p["tasks"][0]["id"]},raw=True).get("isError")
        record("SAFE-02","control","不正参照・循環・読み取り専用更新を拒否する",False,{"invalid_owner_rejected":True,"cycle_rejected":True,"readonly_rejected":True,"revision_unchanged_on_error":True},"基本の整合性と読み取り専用運用を継続する")
        client=fresh("reopen");p=client.get();row=task(p,"SHT-01")
        assert row["status"]=="done" and row["estimate"]>0
        row["status"]="doing"
        assert client.change(p,{"operation":"save_task","task":row},raw=True).get("isError")
        assert client.get()["revision"]==p["revision"]
        row["progress"]=40;p=save(client,p,row);current=task(p,"SHT-01")
        assert current["progress"]==40
        record("OPS-03","P1","完了から100%のまま再開する操作を拒否する",False,{"unchanged_progress_rejected":True,"explicit_progress":40,"remaining_hours":current["estimate"]*.6},"承認状態の独立管理は別途改善する")
        client=fresh("gate");p=client.get();pred=task(p,"SSH-02");assert pred["status"]=="blocked"
        row=task(p,"SSH-03");row.update(status="done",progress=100)
        assert client.change(p,{"operation":"save_task","task":row},raw=True).get("isError")
        assert client.get()["revision"]==p["revision"]
        record("OPS-04","P1","未完了の先行作業がある後続の完了を拒否する",False,{"predecessor":pred["title"],"predecessor_status":pred["status"],"completion_rejected":True},"不要な依存は解除してから完了する。例外承認機構は未実装")
        client=fresh("schedule");p=client.get();old={t["id"]:[t["start"],t["due"]] for t in p["tasks"] if t["status"]=="done"}
        first=task(p,"SHT-01")
        p=client.change(p,{"operation":"save_master","master":{"id":0,"kind":"holiday","name":"検証: 完了項目の開始日に臨時休業","date":first["start"]}})
        saved_before_preview=client.get()
        preview=client.call("schedule_preview",{"project_id":p["id"]})
        assert client.get()==saved_before_preview
        p=client.change(p,{"operation":"schedule","allow_overrun":True})
        changed=[t["id"] for t in p["tasks"] if t["id"] in old and old[t["id"]]!=[t["start"],t["due"]]]
        assert not changed
        record("OPS-05","P1","日程再計算で完了済みの開始日・期限を保持する",False,{"changed_completed_tasks":len(changed),"preview_changes":len(preview["changes"]),"preview_wrote_nothing":True},"基準計画・実績日を別フィールドへ分離する改善は残る")
        row={"title":"検証: 期限を超える必須作業","duration_days":90,"estimate":40,"start":as_of.isoformat()}
        p=save(client,p,row);late=p["tasks"][-1]
        p=client.change(p,{"operation":"add_dependency","dependency":{"predecessor":late["id"],"successor":task(p,"REL-03")["id"],"lag_days":0}})
        saved_before_preview=client.get()
        preview=client.call("schedule_preview",{"project_id":p["id"]});assert preview["overdue_ids"]
        assert client.change(p,{"operation":"schedule"},raw=True).get("isError")
        assert client.get()==saved_before_preview
        p=client.change(p,{"operation":"schedule","allow_overrun":True});ship=task(p,"REL-03")
        assert ship["due"]>p["finish"]
        record("OPS-06","P1","納期超過はプレビューと明示承認が必要",False,{"project_finish":p["finish"],"release_due":ship["due"],"default_rejected":True,"explicit_acceptance_recorded":True},"実行者と承認者の分離は未実装。承認操作は監査記録に残る")
        client=fresh("fields");p=client.get();row=task(p,"PRJ-03")
        assert row["review"]=="pending" and row["reviewer"] and row["priority"]=="high"
        selected=client.call("task_list",{"project_id":p["id"],"review":"pending","priority":"high"})
        assert any(t["id"]==row["id"] for t in selected["tasks"])
        row["status"]="done"
        assert client.change(p,{"operation":"save_task","task":row},raw=True).get("isError")
        row=task(p,"CR-06");row.update(priority="critical",severity="major");p=save(client,p,row)
        record("OPS-07","P1","レビュー・確認者・優先度・重大度を構造化する",False,{"pending_review_count":selected["total"],"completion_without_approval_rejected":True,"saved_priority":task(p,"CR-06")["priority"],"saved_severity":task(p,"CR-06")["severity"]},"確認者はマスタ参照。本人認証・権限分離やレビュー履歴の専用画面は未実装。変更は監査に記録")
        client=fresh("issue-effort");p=client.get();scope=p["history"][-1]["scope"]
        row=task(p,"ISS-001");assert row["related_tasks"] and not row["include_in_effort"]
        row["estimate"]=8;p=save(client,p,row);after=p["history"][-1]["scope"]
        assert after==scope
        row=task(p,"ISS-001");row["include_in_effort"]=True;p=save(client,p,row)
        assert p["history"][-1]["scope"]==scope+8
        record("OPS-08","P2","課題の対応タスクを関連付け、工数算入を選べる",False,{"scope_before":scope,"scope_after_excluded_issue_effort":after,"explicit_opt_in_scope":p["history"][-1]["scope"],"related_task_ids":row["related_tasks"]},"新規GUI課題は工数集計から除外。既存DB/MCP既定は従来どおり算入。重複の自動推定・自動除外は行わない")
        client=fresh("history");p=client.get();day=p["history"][0]["day"];old=p["history"][0]
        row=task(p,"CR-06");row["estimate"]+=100;p=save(client,p,row)
        p=client.change(p,{"operation":"capture","day":day});new=next(s for s in p["history"] if s["day"]==day)
        record("OPS-09","P2","過去日へ現在の集計を上書きできる",old!=new,{"day":day,"before":old,"after":new},"過去締め・補正理由・訂正履歴を設計する。現行captureは明示指定による置換として公開されている")
        client=fresh("deletion");p=client.get();row=task(p,"SSH-02");edges=p["dependencies"]
        before=client.get();impact=client.call("removal_preview",{"project_id":p["id"],"task_id":row["id"]})
        assert client.get()==before and impact["dependencies"]
        p=client.change(p,{"operation":"remove_task","id":row["id"]})
        assert task(p,"SSH-02")["archived"] and p["dependencies"]==edges
        active=client.call("task_list",{"project_id":p["id"],"query":"[SSH-02]"})
        archived=client.call("task_list",{"project_id":p["id"],"query":"[SSH-02]","archived":True})
        assert active["total"]==0 and archived["total"]==1
        p=client.change(p,{"operation":"restore_task","id":row["id"]})
        assert task(p,"SSH-02")==row and p["dependencies"]==edges
        record("OPS-10","P2","保管前の影響を確認し、項目と関係を復元できる",False,{"retained_dependencies":len(impact["dependencies"]),"related_issues":impact["related_issue_ids"],"active_query_hidden":True,"archived_query_found":True,"restored_same_id_and_fields":True},"保管中は依存が無効になる。復元すると同じ関係が有効になる。保管項目も容量上限に含め、完全消去・自動世代整理は提供しない")
        result=dict(generated_at=datetime.now(timezone.utc).isoformat(),as_of=as_of.isoformat(),server_version=subprocess.run([str(executable),"--version"],capture_output=True,text=True,encoding="utf-8",check=True).stdout.strip(),database_sha256=sha256(source.read_bytes()).hexdigest(),scenario_sha256=sha256((ROOT/"demo/scenario.json").read_bytes()).hexdigest(),inventory=[dict(id=p["id"],name=p["name"],items=len(p["tasks"]),issues=sum(t["kind"]=="issue" for t in p["tasks"]),checkpoints=len(p["history"])) for p in projects],findings=findings)
    output.mkdir(parents=True,exist_ok=True)
    (output/"operations.json").write_text(json.dumps(result,ensure_ascii=False,indent=2)+"\n",encoding="utf-8")
    lines=["# MCP運用シナリオの実測結果","",f"基準日: {result['as_of']} / {result['server_version']}","","これは隔離コピーに対する実MCP操作の結果です。gapは試験の成功を意味しません。現在の設計で再現した不足です。","","| ID | 区分 | 結果 | 確認事項 |","| --- | --- | --- | --- |"]
    lines += [f"| {f['id']} | {f['severity']} | {f['status']} | {f['title']} |" for f in findings]
    for f in findings:
        lines += ["",f"## {f['id']} {f['title']}","",f["suggested_action"],"","```json",json.dumps(f["evidence"],ensure_ascii=False,indent=2),"```"]
    (output/"operations.md").write_text("\n".join(lines)+"\n",encoding="utf-8")
    print(json.dumps(dict(projects=len(projects),items=sum(len(p["tasks"]) for p in projects),supported=sum(f["status"]=="supported" for f in findings),gaps=sum(f["status"]=="gap" for f in findings),output=str(output)),ensure_ascii=False))

if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable",type=Path)
    parser.add_argument("--output",type=Path,default=ROOT/"target/operations-validation")
    args=parser.parse_args()
    validate(args.executable.resolve(),args.output.resolve())
