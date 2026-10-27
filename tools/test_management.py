"""Actual MCP acceptance scenario; isolated SQLite or caller-owned PostgreSQL DB."""
import csv
from datetime import datetime, timezone
from hashlib import sha256
import io
import json
from pathlib import Path
import subprocess
import tempfile
from test_mcp import exchange, initialize, tool


def exercise_management(executable, database=None):
    checks=[]
    def call(name,args,readonly=False):
        messages=initialize()+[tool(2,name,args)]
        if database is not None:
            rows=exchange(executable,database,messages,readonly)
        else:
            run=subprocess.run([str(executable),'--mcp-postgres']+(['--read-only'] if readonly else []),input=''.join(json.dumps(m,ensure_ascii=False)+'\n' for m in messages),capture_output=True,text=True,encoding='utf-8',timeout=30,check=True)
            assert not run.stderr,run.stderr
            rows=[json.loads(s) for s in run.stdout.splitlines()]
        return rows[-1]['result']
    def data(name,args,readonly=False):
        r=call(name,args,readonly);assert not r['isError'],r
        return r['structuredContent']
    p=data('project_create',{'name':'依頼と暫定対応の受入試験','start':'2026-01-01','finish':'2026-12-31','actor':'test'})
    pid=p['id']
    def change(c):
        nonlocal p
        p=data('project_change',{'project_id':pid,'expected_revision':p['revision'],'actor':'management-acceptance','change':c})
    for n in range(2):
        change({'operation':'save_task','task':{'kind':'inquiry','title':f'質問{n}','inquiry':{'kind':'question','target_team':'基盤','requested_at':'2026-01-01T09:00:00Z','response_due_at':'2026-01-02T09:00:00Z'}}})
        change({'operation':'save_task','task':{'kind':'workaround','title':f'暫定{n}','workaround':{'reason':'外部製品の修正待ち','permanent_requirements':'修正版と回帰試験','classification':'手作業','detected_at':'2026-01-01T09:00:00Z','permanent_due_on':'2026-01-02'}}})
    inquiries=[t for t in p['tasks'] if t['kind']=='inquiry']; iid=inquiries[0]['id']
    for direction in ['outgoing','incoming','note']:
        change({'operation':'save_exchange','id':iid,'entry':{'direction':direction,'at':'2026-01-02T09:00:00+09:00','person':'窓口','text':'引用,"仕様"\n回答検索語 '+direction}})
    checks.append('separate kinds and exchange directions')
    task=next(t for t in p['tasks'] if t['id']==iid)
    entry=dict(task['inquiry']['exchanges'][0],text='修正版,"仕様"\n回答検索語')
    change({'operation':'save_exchange','id':iid,'entry':entry})
    task=next(t for t in p['tasks'] if t['id']==iid)
    task['inquiry']['exchanges']=[]
    change({'operation':'save_task','task':task})
    assert len(next(t for t in p['tasks'] if t['id']==iid)['inquiry']['exchanges'])==3
    assert data('task_list',{'project_id':pid,'kind':'inquiry','query':'回答検索語','columns':{'inquiry.target_team':'基盤'},'limit':1},True)['total']==1
    exported=data('csv_export',{'project_id':pid,'columns':['title','inquiry.target_team','inquiry.exchange_log'],'filter':{'kind':'inquiry','query':'回答検索語'}},True)
    rows=list(csv.reader(io.StringIO(exported['content'].lstrip('\ufeff'))))
    assert len(rows)==2 and '修正版,"仕様"\n回答検索語' in rows[1][2]
    checks.append('exchange edit, replacement preservation, full-text/column filters and quoted multiline CSV')
    change({'operation':'remove_exchange','id':iid,'entry_id':2})
    change({'operation':'save_exchange','id':iid,'entry':{'text':'採番継続'}})
    assert next(t for t in p['tasks'] if t['id']==iid)['inquiry']['exchanges'][-1]['id']==4
    for kind,key,value in [('inquiry','inquiry','target_team'),('workaround','workaround','classification')]:
        tasks=[t for t in p['tasks'] if t['kind']==kind]
        for t in tasks:t[key][value]='一括変更'
        change({'operation':'save_tasks','tasks':tasks})
        change({'operation':'move_task','id':tasks[-1]['id'],'before':tasks[0]['id']})
        assert data('task_list',{'project_id':pid,'kind':kind,'columns':{key+'.'+value:'一括変更'}},True)['total']==2
    checks.append('bulk/save_tasks, same-column values, row reorder')
    stats_args={'project_id':pid,'kind':'inquiry','from':'2026-01-01','to':'2026-01-03'}
    st=data('management_statistics',stats_args,True)
    assert st['total']==2 and st['open']==2 and st['unanswered']==2 and st['overdue']==2
    assert [d['open'] for d in st['daily']]==[2,2,2]
    st=data('management_statistics',dict(stats_args,kind='workaround'),True)
    assert st['total']==2 and st['not_applied']==2 and st['overdue']==2
    assert call('management_statistics',dict(stats_args,kind='task'))['isError']
    checks.append('daily convergence, classification, overdue, unanswered and temporary-action statistics')
    before=json.loads(json.dumps(p))
    for c,rev,ro in [({'operation':'save_exchange','id':iid,'entry':{'text':''}},p['revision'],False),({'operation':'remove_exchange','id':iid,'entry_id':999},p['revision'],False),({'operation':'remove_task','id':iid},p['revision']-1,False),({'operation':'remove_task','id':iid},p['revision'],True)]:
        assert call('project_change',{'project_id':pid,'expected_revision':rev,'actor':'bad','change':c},ro)['isError']
        assert data('project_get',{'project_id':pid})==before
    checks.append('invalid log, missing ID, stale revision and read-only rollback')
    change({'operation':'remove_task','id':iid})
    assert data('task_list',{'project_id':pid,'kind':'inquiry','archived':True},True)['total']==1
    change({'operation':'restore_task','id':iid})
    assert len(next(t for t in p['tasks'] if t['id']==iid)['inquiry']['exchanges'])==3
    audit=data('audit_list',{'project_id':pid},True)['changes']
    assert any(a['change']['operation']=='remove_exchange' for a in audit)
    checks.append('archive/restore preserves logs and audited changes')
    change({'operation':'save_master','master':{'id':0,'kind':'assignee','name':'管理担当'}})
    assignee=p['masters'][-1]['id']
    updates=[]
    for kind in ['inquiry','workaround']:
        t=next(t for t in p['tasks'] if t['kind']==kind)
        t['status']='done';t['assignee']=assignee
        if kind=='inquiry':
            t['inquiry'].update(contact='相手担当',answered_at='2026-01-02T09:00:00Z',closed_at='2026-01-03T09:00:00Z')
        else:
            t['workaround'].update(reason='全項目の発生理由',permanent_requirements='恒久対応の必要条件',action='手動再送',applied_at='2026-01-02T09:00:00Z',permanent_at='2026-01-03T09:00:00Z',permanent_due_on='2026-01-04')
        updates.append(t)
    change({'operation':'save_tasks','tasks':updates})
    for kind,mean in [('inquiry','mean_answer_hours'),('workaround','mean_temporary_hours')]:
        st=data('management_statistics',dict(stats_args,kind=kind,filter={'assignee':assignee}),True)
        assert st['total']==1 and st['resolved']==1 and st[mean]==24 and st['mean_resolution_hours']==48
    assert data('task_list',{'project_id':pid,'kind':'workaround','query':'恒久対応の必要条件'},True)['total']==1
    checks.append('all management fields, assignee batch, completion dates and filtered response/temporary/permanent means')
    if database is None:
        with tempfile.TemporaryDirectory() as td:
            out=Path(td)/'transfer.sqlite3'
            subprocess.run([str(executable),'--export-sqlite',str(out)],capture_output=True,check=True,timeout=30)
            restored=exchange(executable,out,initialize()+[tool(2,'project_get',{'project_id':pid})],True)[-1]['result']['structuredContent']
            assert restored==p
        checks.append('PostgreSQL to SQLite transfer retains management records and exchanges')
    for c in checks:print('PASS management: '+c)
    return checks


if __name__=='__main__':
    import sys
    with tempfile.TemporaryDirectory(prefix='yyproject-management-') as td:
        exe=Path(sys.argv[1]).resolve()
        checks=exercise_management(exe,Path(td)/'management.sqlite3')
        demo=Path(td)/'expanded.sqlite3'
        subprocess.run([str(exe),'--init-management-demo',str(demo)],capture_output=True,check=True,timeout=30)
        listing=exchange(exe,demo,initialize()+[tool(2,'project_list',{}),tool(3,'project_get',{'project_id':4})],True)
        assert len(listing[1]['result']['structuredContent']['projects'])==4
        p=listing[2]['result']['structuredContent']
        assert len(p['tasks'])==24 and sum(len(t['inquiry']['exchanges']) for t in p['tasks'] if t['inquiry'])==18
        checks.append('expanded demo CLI: four projects, 114 items and 18 exchange records')
    out=Path('target/management-validation');out.mkdir(parents=True,exist_ok=True)
    (out/'mcp.json').write_text(json.dumps({'generated_at':datetime.now(timezone.utc).isoformat(),'executable_sha256':sha256(exe.read_bytes()).hexdigest(),'transport':'stdio','checks':checks,'result':'passed'},ensure_ascii=False,indent=2),encoding='utf-8')
