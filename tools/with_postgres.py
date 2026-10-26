"""Run a YYproject command using local Compose credentials without echoing them."""
import os
from pathlib import Path
import subprocess
import sys
from urllib.parse import quote
root=Path(__file__).resolve().parents[1]
values={}
for line in (root/'.env').read_text(encoding='utf-8-sig').splitlines():
    if '=' in line and not line.lstrip().startswith('#'):
        key,value=line.split('=',1);values[key.strip()]=value.strip().strip('"\'')
password=values.get('YYPROJECT_POSTGRES_PASSWORD') or os.environ.get('YYPROJECT_POSTGRES_PASSWORD')
if not password:raise SystemExit('Set YYPROJECT_POSTGRES_PASSWORD in .env')
port=os.environ.get('YYPROJECT_POSTGRES_PORT',values.get('YYPROJECT_POSTGRES_PORT','15432'))
host=os.environ.get('YYPROJECT_POSTGRES_HOST','127.0.0.1')
database=os.environ.get('YYPROJECT_POSTGRES_DATABASE','yyproject')
env=dict(os.environ)
env['YYPROJECT_DATABASE_URL']=f'postgresql://yyproject:{quote(password,safe="")}@{host}:{port}/{quote(database,safe="")}?sslmode=disable'
if len(sys.argv)<2:raise SystemExit('Usage: python tools/with_postgres.py EXECUTABLE [ARGS...]')
raise SystemExit(subprocess.run(sys.argv[1:],env=env).returncode)
