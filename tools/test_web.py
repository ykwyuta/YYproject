"""Real password/cookie Web UI and Store API; all processes and data are owned."""
from contextlib import closing
from pathlib import Path
import http.client
import json
import os
import sqlite3
import subprocess
import sys
import tempfile
import tomllib
from test_http import Server, generate
from test_mcp import exchange,initialize,tool


def password(exe,config,name,readonly=False):
    r=subprocess.run([str(exe),'--generate-password',name,'--auth-config',str(config)]+(['--read-only'] if readonly else []),capture_output=True,text=True,encoding='utf-8',check=True,timeout=15)
    return json.loads(r.stdout)['password']


class BrowserClient:
    def __init__(self,server):
        self.server=server;self.cookie='';self.csrf=''
    def request(self,path,value=None,csrf=True,origin=True,method='POST'):
        connection=http.client.HTTPConnection('127.0.0.1',self.server.port,timeout=20)
        headers={'Content-Type':'application/json'}
        if origin:headers['Origin']=f'http://127.0.0.1:{self.server.port}'
        if self.cookie:headers['Cookie']=self.cookie
        if csrf:headers['X-YY-CSRF']=self.csrf
        try:
            connection.request(method,path,json.dumps(value).encode() if value is not None else None,headers)
            r=connection.getresponse();raw=r.read();head=dict(r.getheaders());body=json.loads(raw) if head.get('content-type','').startswith('application/json') else raw
            return r.status,head,body
        finally:connection.close()
    def login(self,pw):
        status,head,body=self.request('/web/login',{'password':pw});assert status==200,(status,body)
        self.cookie=head['set-cookie'].split(';')[0];self.csrf=body['csrf']
        assert 'HttpOnly' in head['set-cookie'] and 'SameSite=Strict' in head['set-cookie']
        return body
    def api(self,value):
        status,_,body=self.request('/web/api',value);assert status==200,(status,body)
        return body
    def call(self,name,args):return self.api({'action':'call','name':name,'args':args})


def main():
    exe=Path(sys.argv[1]).resolve();postgres='--postgres' in sys.argv
    with tempfile.TemporaryDirectory(prefix='yyproject-web-') as td:
        root=Path(td);config=root/'auth.toml';db=root/'web.sqlite3'
        alice=password(exe,config,'alice');bob=password(exe,config,'bob',True)
        pat=generate(exe,config,'agent')
        raw=config.read_text(encoding='utf-8');assert alice not in raw and bob not in raw and pat not in raw
        assert len(tomllib.loads(raw)['web_users'])==2
        server=Server(exe,config,db,postgres=postgres,web=True)
        try:
            client=BrowserClient(server)
            assert client.request('/',method='GET')[0]==200
            assert client.request('/web/app.js',method='GET')[0]==200
            assert client.request('/web/api',{'action':'metadata'})[0]==401
            assert client.request('/web/login',{'password':pat})[0]==401
            assert client.request('/web/login',{'password':alice},origin=False)[0]==403
            assert client.login(alice)['user']=='alice'
            assert client.request('/web/api',{'action':'metadata'},csrf=False)[0]==403
            metadata=client.api({'action':'metadata'});assert len(metadata['columns'])==5 and 'title' not in metadata['bulk']
            assert client.request('/web/session',method='GET',csrf=False)[2]['csrf']==client.csrf
            p=client.call('project_create',{'name':'Web受入','start':'2026-01-01','finish':'2026-12-31','actor':'forged'})
            pid=p['id']
            def edit(values,kind='task',item=0,parent=None):
                nonlocal p
                p=client.api({'action':'edit','project_id':pid,'expected_revision':p['revision'],'id':item,'kind':kind,'parent':parent,'values':values})
            edit({'title':'親','description':'<script>テキストとして表示</script>'})
            edit({'title':'子'},parent=1)
            edit({'title':'孫'},parent=2)
            edit({'title':'依頼','inquiry.target_team':'基盤'},kind='inquiry')
            def grid(ids,keys,text,**extra):return client.api({'action':'grid','edit':{'project_id':pid,'expected_revision':p['revision'],'ids':ids,'keys':keys,'text':text,**extra}})
            p=grid([1,2],['priority'],'high\r\n',fill_down=True)
            assert all(t['priority']=='high' for t in p['tasks'][:2])
            p=grid([1],['description'],'"引用,""改行""\n2行目"\r\n')
            assert p['tasks'][0]['description']=='引用,"改行"\n2行目'
            before=p
            bad={'action':'grid','edit':{'project_id':pid,'expected_revision':p['revision'],'ids':[1,2],'keys':['title'],'text':'一括上書き'}}
            assert client.request('/web/api',bad)[0]==400
            assert client.call('project_get',{'project_id':pid})==before
            p=client.api({'action':'edit_master','project_id':pid,'expected_revision':p['revision'],'id':0,'kind':'assignee','values':{'name':'担当A'}})
            p=client.call('project_change',{'project_id':pid,'expected_revision':p['revision'],'change':{'operation':'save_exchange','id':4,'entry':{'text':'確認を依頼'}}})
            stats=client.call('management_statistics',{'project_id':pid,'kind':'inquiry','from':'2026-01-01','to':'2026-12-31'})
            assert stats['total']==1
            csv=client.call('csv_export',{'project_id':pid,'columns':['title','inquiry.exchange_log'],'filter':{'kind':'inquiry'}})
            assert '確認を依頼' in csv['content']
            stale={'action':'edit','project_id':pid,'expected_revision':1,'id':1,'kind':'task','values':{'title':'競合'}}
            assert client.request('/web/api',stale)[0]==409
            assert client.call('audit_list',{'project_id':pid})['changes'][0]['actor']=='alice'
            reader=BrowserClient(server);assert reader.login(bob)['read_only']
            assert reader.request('/web/api',{'action':'call','name':'project_change','args':{'project_id':pid,'expected_revision':p['revision'],'change':{'operation':'remove_task','id':1}}})[0]==400
            assert reader.request('/web/api',bad)[0]==400
            assert reader.api({'action':'project','project_id':pid})['project']==p
            assert client.request('/web/api',{'action':'call','name':'project_create','args':42})[0]==400
            assert client.api({'action':'metadata'})['bulk']
            status,headers,backup=reader.request('/web/backup',{})
            assert status==200 and backup.startswith(b'SQLite format 3')
            exported=root/'backup.sqlite3';exported.write_bytes(backup)
            restored=exchange(exe,exported,initialize()+[tool(2,'project_get',{'project_id':pid})],True)[-1]['result']['structuredContent']
            assert restored==p
            session=server.initialize(pat)
            assert server.tool(pat,session,'project_get',{'project_id':pid})['structuredContent']==p
            new_alice=password(exe,config,'alice');assert new_alice!=alice
            assert client.request('/web/api',{'action':'metadata'})[0]==401
            assert client.request('/web/login',{'password':alice})[0]==401
            client.login(new_alice)
            client.request('/web/logout',{});assert client.request('/web/api',{'action':'metadata'})[0]==401
        finally:server.close()
        server=Server(exe,config,db,readonly=True,postgres=postgres,web=True)
        try:
            client=BrowserClient(server);assert client.login(new_alice)['read_only']
            assert client.request('/web/api',{'action':'edit','project_id':pid,'expected_revision':p['revision'],'id':1,'kind':'task','values':{'title':'拒否'}})[0]==400
        finally:server.close()
        if not postgres:
            with closing(sqlite3.connect(db)) as con:
                p=json.loads(con.execute('select document from projects where id=?',(pid,)).fetchone()[0]);assert p['tasks'][0]['title']=='親'
        if '--browser' in sys.argv:
            demo=root/'browser.sqlite3'
            subprocess.run([str(exe),'--init-management-demo',str(demo)],check=True,capture_output=True,timeout=30)
            server=Server(exe,config,demo,web=True)
            try:
                env=dict(os.environ,YYPROJECT_TEST_PASSWORD=new_alice,YYPROJECT_TEST_READONLY=bob)
                subprocess.run(['node',str(Path(__file__).with_name('test_web_browser.cjs')),f'http://127.0.0.1:{server.port}'],env=env,check=True,timeout=180)
            finally:server.close()
    print('Web password generation/rotation, PAT separation, HttpOnly session, CSRF/origin, readonly, grid atomicity/hierarchy/TSV, master edits, conflict, audit identity, inquiry statistics/CSV and logout: OK')


if __name__=='__main__':main()
