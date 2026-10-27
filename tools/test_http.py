"""Exercise the real Streamable HTTP server with isolated DB, users and owned process."""
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from queue import Queue
from threading import Thread
import hashlib
import http.client
import json
import os
import subprocess
import sys
import tempfile
import tomllib


def generate(exe, config, user, readonly=False):
    result = subprocess.run([str(exe), '--generate-pat', user, '--auth-config', str(config)] +
                            (['--read-only'] if readonly else []), capture_output=True,
                            text=True, encoding='utf-8', check=True, timeout=15)
    return json.loads(result.stdout)['pat']


class Server:
    def __init__(self, exe, config, database, readonly=False, postgres=False):
        args = [str(exe), '--mcp-http-postgres'] if postgres else [str(exe), '--mcp-http', str(database)]
        self.process = subprocess.Popen(args + ['--listen', '127.0.0.1:0', '--auth-config', str(config)] +
                                        (['--read-only'] if readonly else []), stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, text=True, encoding='utf-8')
        lines = Queue()
        def read():
            for line in self.process.stderr:
                lines.put(line)
            lines.put('PROCESS EXITED')
        self.reader = Thread(target=read, daemon=True)
        self.reader.start()
        try:
            line = lines.get(timeout=30)
            assert line.startswith('YYproject MCP Streamable HTTP: http://127.0.0.1:'), line
            self.port = int(line.split(':')[-1].split('/')[0])
        except BaseException:
            self.close()
            raise

    def close(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)
        self.reader.join(timeout=5)
        stdout = self.process.stdout.read()
        self.process.stdout.close()
        self.process.stderr.close()
        assert not stdout, 'HTTP server wrote non-protocol output to stdout'

    def request(self, token=None, message=None, session=None, method='POST', extra=None, raw=None):
        headers = {'Content-Type': 'application/json', 'Accept': 'application/json, text/event-stream',
                   'MCP-Protocol-Version': '2025-11-25'}
        if token:
            headers['Authorization'] = 'Bearer ' + token
        if session:
            headers['MCP-Session-Id'] = session
        headers.update(extra or {})
        body = raw if raw is not None else json.dumps(message, ensure_ascii=False).encode('utf-8') if message is not None else None
        connection = http.client.HTTPConnection('127.0.0.1', self.port, timeout=20)
        try:
            connection.request(method, '/mcp', body=body, headers=headers)
            response = connection.getresponse()
            data = response.read()
            return response.status, dict(response.getheaders()), json.loads(data) if data else None
        finally:
            connection.close()

    def initialize(self, token, notify=True):
        message = {'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {
            'protocolVersion': '2025-11-25', 'capabilities': {}, 'clientInfo': {'name': 'http-test', 'version': '1'}}}
        status, headers, body = self.request(token, message)
        assert status == 200 and body['result']['protocolVersion'] == '2025-11-25', (status, body)
        assert headers['content-type'].startswith('application/json')
        session = headers['mcp-session-id']
        assert len(session) == 64
        if notify:
            assert self.request(token, {'jsonrpc': '2.0', 'method': 'notifications/initialized'}, session)[::2] == (202, None)
        return session

    def tool(self, token, session, name, arguments):
        status, _, body = self.request(token, {'jsonrpc': '2.0', 'id': 2, 'method': 'tools/call',
                                               'params': {'name': name, 'arguments': arguments}}, session)
        assert status == 200, (status, body)
        return body['result']


def data(result):
    assert not result['isError'], result
    return result['structuredContent']


def main():
    exe = Path(sys.argv[1]).resolve()
    postgres = '--postgres' in sys.argv[2:]
    with tempfile.TemporaryDirectory(prefix='yyproject-http-') as folder:
        root = Path(folder)
        config = root / 'auth.toml'
        database = root / 'server.sqlite3'
        alice = generate(exe, config, 'alice')
        bob = generate(exe, config, 'bob', True)
        text = config.read_text(encoding='utf-8')
        assert alice not in text and bob not in text
        users = tomllib.loads(text)['users']
        assert next(u for u in users if u['name'] == 'alice')['pat_sha256'] == hashlib.sha256(alice.encode()).hexdigest()
        server = Server(exe, config, database, postgres=postgres)
        try:
            assert server.request()[0] == 401
            assert server.request('yyp_' + '0'*64)[0] == 401
            assert server.request(alice, extra={'Origin': 'http://evil.invalid'})[0] == 403
            assert server.request(alice, extra={'Host': 'evil.invalid'})[0] == 403
            assert server.request(alice, extra={'Accept': 'application/json'})[0] == 406
            assert server.request(alice, extra={'Content-Type': 'text/plain'})[0] == 415
            assert server.request(alice, raw=b'[')[0] == 400
            assert server.request(alice, raw=b'x'*(2*1024*1024+1))[0] == 413
            assert server.request(alice, {'jsonrpc': '2.0', 'id': 1, 'method': 'initialize'})[0] == 400
            a = server.initialize(alice, False)
            status, _, reply = server.request(alice, {'jsonrpc': '2.0', 'id': 3, 'method': 'tools/list'}, a)
            assert status == 200 and reply['error']['code'] == -32000
            assert server.request(alice, {'jsonrpc': '2.0', 'method': 'notifications/initialized'}, a)[0] == 202
            b = server.initialize(bob)
            assert a != b
            assert server.request(alice, {'jsonrpc': '2.0', 'id': 3, 'method': 'tools/list'}, a)[2]['result']['tools']
            assert server.request(alice, method='GET', session=a)[0] == 405
            assert server.request(alice, {'jsonrpc': '2.0', 'id': 3, 'method': 'ping'})[0] == 400
            assert server.request(bob, {'jsonrpc': '2.0', 'id': 3, 'method': 'ping'}, a)[0] == 403
            assert server.request(alice, {'jsonrpc': '2.0', 'id': 3, 'method': 'ping'}, a,
                                  extra={'MCP-Protocol-Version': '2099-01-01'})[0] == 400
            project = data(server.tool(alice, a, 'project_create', {'name': 'HTTP 日本語', 'start': '2026-01-01',
                           'finish': '2026-12-31', 'actor': 'spoofed'}))
            pid = project['id']
            assert server.tool(bob, b, 'project_create', {'name': 'blocked', 'start': '2026-01-01',
                                                       'finish': '2026-12-31', 'actor': 'bob'})['isError']
            assert data(server.tool(bob, b, 'project_get', {'project_id': pid})) == project
            def compete(index):
                return server.tool(alice, a, 'project_change', {'project_id': pid, 'expected_revision': project['revision'],
                                   'actor': 'spoofed', 'change': {'operation': 'save_task', 'task': {'title': f'HTTP task {index}',
                                   'completed_on': '2026-10-27'}}})
            with ThreadPoolExecutor(max_workers=2) as pool:
                results = list(pool.map(compete, range(2)))
            assert sum(not r['isError'] for r in results) == 1
            audits = data(server.tool(alice, a, 'audit_list', {'project_id': pid}))['changes']
            assert len(audits) == 2 and all(row['actor'] == 'alice' for row in audits)
            current = data(server.tool(alice, a, 'project_get', {'project_id': pid}))
            case = data(server.tool(alice, a, 'project_change', {'project_id': pid, 'expected_revision': current['revision'], 'actor': 'spoofed',
                        'change': {'operation': 'save_task', 'task': {'title': 'HTTP bug', 'kind': 'bug', 'include_in_effort': False,
                                   'case': {'detected_at': '2026-01-01T09:00:00Z', 'cause': 'HTTP原因', 'permanent_difficulty': 'high'}}}}))
            statistics = data(server.tool(bob, b, 'case_statistics', {'project_id': pid, 'kind': 'bug', 'from': '2026-01-01', 'to': '2026-01-03'}))
            assert statistics['total'] == 1 and statistics['daily'][-1]['open'] == 1 and statistics['causes'] == {'HTTP原因': 1}
            assert case['tasks'][-1]['case']['permanent_difficulty'] == 'high'
            for kind,details in [('inquiry',{'target_team':'HTTP基盤','requested_at':'2026-01-01T09:00:00Z'}),('workaround',{'reason':'HTTP修正待ち','detected_at':'2026-01-01T09:00:00Z'})]:
                case=data(server.tool(alice,a,'project_change',{'project_id':pid,'expected_revision':case['revision'],'actor':'spoofed','change':{'operation':'save_task','task':{'kind':kind,'title':'HTTP '+kind,kind:details}}}))
                mid=case['tasks'][-1]['id']
                if kind=='inquiry':
                    change={'operation':'save_exchange','id':mid,'entry':{'text':'HTTP回答記録'}}
                    assert server.tool(bob,b,'project_change',{'project_id':pid,'expected_revision':case['revision'],'actor':'spoofed','change':change})['isError']
                    case=data(server.tool(alice,a,'project_change',{'project_id':pid,'expected_revision':case['revision'],'actor':'spoofed','change':change}))
                st=data(server.tool(bob,b,'management_statistics',{'project_id':pid,'kind':kind,'from':'2026-01-01','to':'2026-01-03'}))
                assert st['total']==1 and st['open']==1
                exported=data(server.tool(bob,b,'csv_export',{'project_id':pid,'columns':['title',kind+'.'+('target_team' if kind=='inquiry' else 'reason')],'filter':{'kind':kind}}))
                assert 'HTTP '+kind in exported['content']
            audits=data(server.tool(alice,a,'audit_list',{'project_id':pid}))['changes']
            assert all(row['actor']=='alice' for row in audits)
            assert server.request(alice, session=a, method='DELETE')[0] == 204
            assert server.request(alice, {'jsonrpc': '2.0', 'id': 3, 'method': 'ping'}, a)[0] == 404
            assert server.request(alice, session=a, method='GET')[0] == 404
            a = server.initialize(alice)
            new_alice = generate(exe, config, 'alice')
            assert new_alice != alice
            assert server.request(alice, {'jsonrpc': '2.0', 'id': 3, 'method': 'ping'}, a)[0] == 401
            assert server.request(new_alice, {'jsonrpc': '2.0', 'id': 3, 'method': 'ping'}, a)[0] == 403
            a = server.initialize(new_alice)
            assert data(server.tool(new_alice, a, 'project_get', {'project_id': pid}))['tasks'][0]['completed_on'] == '2026-10-27'
            blocks = config.read_text(encoding='utf-8').split('[[users]]')
            config.write_text(blocks[0] + ''.join('[[users]]' + block for block in blocks[1:]
                                                if 'name = "bob"' not in block), encoding='utf-8')
            assert server.request(bob, {'jsonrpc': '2.0', 'id': 3, 'method': 'ping'}, b)[0] == 401
            config.write_text('invalid config', encoding='utf-8')
            assert server.request(new_alice, {'jsonrpc': '2.0', 'id': 3, 'method': 'ping'}, a)[0] == 503
        finally:
            server.close()
        # Restore a valid config and verify the global read-only mode independently of user permissions.
        config.write_text(text, encoding='utf-8')
        server = Server(exe, config, database, readonly=True, postgres=postgres)
        try:
            a = server.initialize(alice)
            assert server.tool(alice, a, 'project_change', {'project_id': pid, 'expected_revision': 2, 'actor': 'alice',
                               'change': {'operation': 'save_task', 'task': {'title': 'blocked'}}})['isError']
        finally:
            server.close()
    print('Streamable HTTP real endpoint, PAT hashes/rotation/user binding, lifecycle, headers, limits, read-only, audit identity and concurrent conflicts: OK')


if __name__ == '__main__':
    main()
