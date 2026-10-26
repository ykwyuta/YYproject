"""Real stdio + isolated PostgreSQL DB: migration, conflicts, atomicity and round trip.
Set YYPROJECT_DATABASE_URL to an EMPTY dedicated test database. No SQL is exposed by MCP.
"""
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
from test_mcp import initialize, tool, exchange


def pg_exchange(executable, messages, read_only=False):
    args=[str(executable),'--mcp-postgres']+(['--read-only'] if read_only else [])
    result=subprocess.run(args,input=''.join(json.dumps(m,ensure_ascii=False)+'\n' for m in messages),text=True,encoding='utf-8',capture_output=True,timeout=60,check=True)
    assert not result.stderr, result.stderr
    return [json.loads(line) for line in result.stdout.splitlines()]


def data(row):
    assert not row['result'].get('isError'), row
    return row['result']['structuredContent']


def pg_tool(executable,name,args,read_only=False):
    return pg_exchange(executable,initialize()+[tool(2,name,args)],read_only)[-1]


def main():
    assert os.environ.get('YYPROJECT_DATABASE_URL'), 'Set YYPROJECT_DATABASE_URL to an empty test database'
    exe=Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix='yyproject-pg-') as folder:
        root=Path(folder);source=root/'source.sqlite3';target=root/'roundtrip.sqlite3'
        messages=initialize()+[tool(2,'project_create',{'name':'移行と往復','start':'2026-01-01','finish':'2026-12-31','actor':'test'})]
        revision=1;identifier=3
        changes=[]
        for i in range(3):
            changes.append({'operation':'save_master','master':{'id':0,'name':f'分類{i+1}','kind':'category','parent':i if i else None}})
        for i in range(3):
            changes.append({'operation':'save_task','task':{'title':f'階層タスク{i+1}','parent':i if i else None,'category':3,'description':'全文語\n日本語,"引用"','review_note':'メモ','estimate':8}})
        changes.append({'operation':'save_task','task':{'title':'節目','milestone':True,'start':'2026-12-30','due':'2026-12-30'}})
        for i in range(110):
            changes.append({'operation':'rename','name':f'移行と往復 {i}','start':'2026-01-01','finish':'2026-12-31'})
        for change in changes:
            messages.append(tool(identifier,'project_change',{'project_id':1,'expected_revision':revision,'actor':'fixture','change':change}));revision+=1;identifier+=1
        for row in exchange(exe,source,messages):
            if row['id']>=2:data(row)
        original=data(exchange(exe,source,initialize()+[tool(2,'project_get',{'project_id':1})])[-1])
        assert data(pg_tool(exe,'project_list',{}))['projects']==[]
        subprocess.run([str(exe),'--migrate-to-postgres',str(source)],check=True,capture_output=True,text=True,encoding='utf-8',timeout=30)
        assert data(pg_tool(exe,'project_get',{'project_id':1}))==original
        first=data(pg_tool(exe,'audit_list',{'project_id':1}))['changes'];second=data(pg_tool(exe,'audit_list',{'project_id':1,'after':first[-1]['id']}))['changes']
        assert len(first)==100 and len(first+second)==revision
        search=data(pg_tool(exe,'task_list',{'project_id':1,'query':'全文語','category':1}));assert search['total']==3
        assert data(pg_tool(exe,'task_list',{'project_id':1,'columns':{'parent':'2'}}))['total']==1
        stale=pg_tool(exe,'project_change',{'project_id':1,'expected_revision':1,'actor':'stale','change':{'operation':'remove_task','id':1}});assert stale['result']['isError']
        readonly=pg_tool(exe,'project_change',{'project_id':1,'expected_revision':revision,'actor':'readonly','change':{'operation':'remove_task','id':1}},True);assert readonly['result']['isError']
        invalid=[dict(t) for t in original['tasks'][:2]];invalid[0]['title']='must roll back';invalid[1]['parent']=99999
        failed=pg_tool(exe,'project_change',{'project_id':1,'expected_revision':revision,'actor':'grid','change':{'operation':'save_tasks','tasks':invalid}});assert failed['result']['isError']
        assert data(pg_tool(exe,'project_get',{'project_id':1}))==original
        duplicate=subprocess.run([str(exe),'--migrate-to-postgres',str(source)],capture_output=True,text=True,encoding='utf-8',timeout=30);assert duplicate.returncode!=0
        added=data(pg_tool(exe,'project_change',{'project_id':1,'expected_revision':revision,'actor':'postgres','change':{'operation':'save_task','task':{'title':'移行後もIDを継続','parent':2}}}));assert added['tasks'][-1]['id']==5
        # Two independently connected clients race with the same observed revision.
        def compete(number):
            return pg_tool(exe,'project_change',{'project_id':1,'expected_revision':revision+1,'actor':f'parallel-{number}','change':{'operation':'save_task','task':dict(added['tasks'][-1],title=f'競合更新 {number}')}})
        with ThreadPoolExecutor(max_workers=2) as pool:
            results=list(pool.map(compete,range(2)))
        successes=[row for row in results if not row['result']['isError']]
        assert len(successes)==1 and sum(row['result']['isError'] for row in results)==1
        added=data(successes[0])
        subprocess.run([str(exe),'--export-sqlite',str(target)],check=True,capture_output=True,text=True,encoding='utf-8',timeout=30)
        restored=data(exchange(exe,target,initialize()+[tool(2,'project_get',{'project_id':1})])[-1]);assert restored==added
        with closing(sqlite3.connect(source)) as conn:old_audit=conn.execute('SELECT id,project_id,at,actor,revision,change FROM audit ORDER BY id').fetchall()
        with closing(sqlite3.connect(target)) as conn:new_audit=conn.execute('SELECT id,project_id,at,actor,revision,change FROM audit ORDER BY id').fetchall()
        # JSON object key order may be canonicalized by serialization; compare values.
        normalize=lambda rows:[list(r[:5])+[json.loads(r[5])] for r in rows]
        assert normalize(new_audit[:-2])==normalize(old_audit)
        assert new_audit[-1][4]==revision+2
        before=target.read_bytes();duplicate=subprocess.run([str(exe),'--export-sqlite',str(target)],capture_output=True,timeout=30);assert duplicate.returncode!=0 and target.read_bytes()==before
        created=data(pg_tool(exe,'project_create',{'name':'ID継続','start':'2026-01-01','finish':'2026-12-31','actor':'test'}));assert created['id']==2
    print('PostgreSQL real MCP, full-text/column filters, hierarchy, milestone, revisions, read-only, batch rollback, empty-target guard, all audit rows, SQLite round-trip and ID sequences: OK')

if __name__=='__main__':main()
