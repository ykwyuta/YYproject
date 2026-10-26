"""Exercise bug/issue investigation and statistics over the actual stdio transport."""
import json
from pathlib import Path
import subprocess
import tempfile
from test_mcp import exchange, initialize, tool


def exercise_cases(executable, database=None):
    def call(name, args, read_only=False):
        messages=initialize()+[tool(2,name,args)]
        if database is not None:
            rows=exchange(executable,database,messages,read_only)
        else:
            result=subprocess.run([str(executable),'--mcp-postgres']+(['--read-only'] if read_only else []),input=''.join(json.dumps(m,ensure_ascii=False)+'\n' for m in messages),capture_output=True,text=True,encoding='utf-8',check=True,timeout=30)
            assert not result.stderr,result.stderr
            rows=[json.loads(line) for line in result.stdout.splitlines()]
        return rows[-1]['result']
    def data(name,args,read_only=False):
        result=call(name,args,read_only)
        assert not result['isError'],result
        return result['structuredContent']
    project=data('project_create',{'name':'バグ課題の実接続','start':'2026-01-01','finish':'2026-12-31','actor':'test'})
    pid=project['id']
    for kind,status,closed in [('bug','done','2026-01-02T09:00:00Z'),('bug','todo',None),('issue','todo',None)]:
        task={'title':kind+status,'kind':kind,'status':status,'include_in_effort':False,'case':{'detected_at':'2026-01-01T18:00:00+09:00','cause':'仕様漏れ','occurrence_phase':'設計','detection_phase':'結合試験','temporary_plan':'回避策\n2行目','permanent_plan':'恒久対策案件ABC','permanent_difficulty':'high','closed_at':closed}}
        project=data('project_change',{'project_id':pid,'expected_revision':project['revision'],'actor':'cases','change':{'operation':'save_task','task':task}})
    selected=data('task_list',{'project_id':pid,'kind':'bug','query':'恒久対策案件ABC','columns':{'case.cause':'仕様漏れ'}},True)
    assert selected['total']==2
    stats_args={'project_id':pid,'kind':'bug','from':'2026-01-01','to':'2026-01-03'}
    stats=data('case_statistics',stats_args,True)
    assert stats['total']==2 and stats['open']==1 and stats['resolved']==1
    assert [d['open'] for d in stats['daily']]==[2,1,1]
    assert stats['causes']=={'仕様漏れ':2} and stats['mean_resolution_hours']==24
    assert data('case_statistics',dict(stats_args,kind='issue'),True)['total']==1
    bad=dict(project['tasks'][1]);bad['case']=dict(bad['case'],occurred_at='2026-01-02T00:00:00Z')
    failed=call('project_change',{'project_id':pid,'expected_revision':project['revision'],'actor':'bad','change':{'operation':'save_task','task':bad}})
    assert failed['isError']
    assert data('project_get',{'project_id':pid})==project
    readonly=call('project_change',{'project_id':pid,'expected_revision':project['revision'],'actor':'bad','change':{'operation':'save_task','task':bad}},True)
    assert readonly['isError']
    assert call('case_statistics',dict(stats_args,kind='task'))['isError']
    if database is None:
        with tempfile.TemporaryDirectory(prefix='yyproject-case-export-') as directory:
            out=Path(directory)/'cases.sqlite3'
            subprocess.run([str(executable),'--export-sqlite',str(out)],check=True,capture_output=True,timeout=30)
            restored=exchange(executable,out,initialize()+[tool(2,'project_get',{'project_id':pid})],True)[-1]['result']['structuredContent']
            assert restored==project
    print('Real MCP bug/issue isolation, investigation fields, UTC dates, daily convergence, classifications, readonly statistics, atomic invalid dates and full SQLite transfer: OK')


if __name__=='__main__':
    import sys
    with tempfile.TemporaryDirectory(prefix='yyproject-cases-') as directory:
        exercise_cases(Path(sys.argv[1]).resolve(),Path(directory)/'cases.sqlite3')
