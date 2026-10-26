# Streamable HTTPサーバーとPAT

YYprojectを常駐プロセスとして起動し、`http://127.0.0.1:8740/mcp` をMCPエンドポイントとして提供できます。SQLiteとPostgreSQLに対応し、stdioと同じ8個のツールを使用します。GUIとは別に起動します。

## ユーザーとPATを作成する

```powershell
target/release/yyproject.exe --generate-pat alice --auth-config yyproject-server.toml
target/release/yyproject.exe --generate-pat viewer --auth-config yyproject-server.toml --read-only
```

JSONで表示された`pat`をクライアントへ登録してください。PATはOSの乱数から生成し、その場で1回だけ表示します。設定ファイルにはSHA-256ハッシュとユーザー名、読み取り専用設定だけを保存します。元のPATは復元できません。同じユーザー名で再実行するとPATを更新し、他のユーザーは保持します。更新とユーザー削除は次のHTTP要求から反映されます。削除は設定ファイルから該当する`[[users]]`ブロックを除いて保存します。空のユーザー一覧や不正な設定は接続を拒否します。

設定の形式は以下です。ハッシュはコマンドが記入します。

```toml
[server]
allowed_hosts = []
allowed_origins = []

[[users]]
name = "alice"
pat_sha256 = "ここには生成コマンドが64桁のハッシュを保存します"
read_only = false
```

標準の`yyproject-server.toml`はGitと配布物に含めません。設定を書き換えられる人は利用権限を変更できるため、管理者だけが書き込める場所で管理してください。生成時は同じ場所のロックファイルと一時ファイルを使い、設定を置き換えます。処理中にプロセスを強制終了して`.toml.lock`が残った場合は、生成プロセスが終了していることを確認してから、そのロックだけを削除します。

## サーバーを起動する

SQLite:

```powershell
target/release/yyproject.exe --mcp-http projects.sqlite3 --auth-config yyproject-server.toml
```

PostgreSQL（既存のComposeを使用）:

```powershell
python tools/with_postgres.py target/release/yyproject.exe --mcp-http-postgres --auth-config yyproject-server.toml
```

`--listen 127.0.0.1:9000`でポートを変更できます。`--read-only`を付けると全ユーザーを読み取り専用にします。接続先DBは起動時に固定します。配布ZIPでは実行ファイルを`yyproject.exe`、今回の別出力先では`target/kanban-build/release/yyproject.exe`に読み替えてください。Ctrl+Cで停止します。

## クライアントの設定

MCPクライアントでトランスポートをStreamable HTTP、URLを`http://127.0.0.1:8740/mcp`、HTTPヘッダーを次のように指定します。PATはそのクライアントの秘密情報設定へ保存してください。

```text
Authorization: Bearer <生成されたPAT>
```

初期化とセッション管理はMCPクライアントが行います。自作クライアントではPOSTの`Accept`に`application/json, text/event-stream`、`Content-Type`に`application/json`を指定します。初期化応答の`MCP-Session-Id`と、合意した`MCP-Protocol-Version`を以降の要求へ付けます。初期化後は`notifications/initialized`を送信します。

JSON応答を使うStreamable HTTPです。通知への応答は202、DELETEによるセッション終了は204、存在しないセッションは404です。SSE配信は使わず、GETは405を返します。従来のHTTP+SSE方式、OAuth認可、ブラウザー向けのCORSは提供しません。対応版は2025-11-25、2025-06-18、2025-03-26です。セッションは30分の無操作で失効し、同時256セッション・処理待ち64要求・要求本文2MiBを上限とします。

PATは毎要求検証し、セッションをユーザーとPATに結び付けます。PATを更新したクライアントは初期化からやり直します。`project_create`と`project_change`の監査のactorは、クライアントが送った値によらず認証ユーザー名になります。ユーザーの読み取り専用設定は両ツールによる変更を拒否します。ユーザーごとのプロジェクト閲覧制限はなく、接続DB全体を共有します。

## 別PCから利用する場合

初期状態はループバック接続だけです。別PCへ公開する場合は`--listen 0.0.0.0:8740`などを指定し、設定の`server.allowed_hosts`へクライアントが送るHost（例:`tasks.example.internal:8740`）を明示してください。外向けはHTTPSリバースプロキシでTLSを終端し、PATを平文のネットワークへ流さない構成にします。プロキシは設定と一致するHostを渡してください。Originがある要求は同じホストのHTTP Originまたは`server.allowed_origins`の明示値だけ許可します。ホスト・Originの設定はサーバー起動時に読み、変更時は再起動します。ユーザーとPATの変更には再起動は不要です。

仕様: [MCP Streamable HTTP](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)。
