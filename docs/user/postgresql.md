# PostgreSQLとDocker Compose

SQLiteモードは従来どおりサーバーなしで利用できます。複数のPCで同じ案件を扱う場合はPostgreSQLモードを選べます。両モードとも版競合・トランザクション・監査記録を使います。

## 起動

リポジトリまたは配布フォルダの `.env.example` を `.env` にコピーし、`YYPROJECT_POSTGRES_PASSWORD` を設定します。`.env` はGitと配布アーカイブに含めません。

```powershell
if (-not (Test-Path .env)) { Copy-Item .env.example .env }
# 初回起動前に .env のパスワードを設定する
docker compose up -d --wait postgres
python tools/with_postgres.py target/release/yyprojectw.exe --postgres
```

ComposeはPostgreSQL 17を起動し、`127.0.0.1:15432` に公開します。データは名前付きボリュームに保持します。ポートは `.env` の `YYPROJECT_POSTGRES_PORT` で変更できます。`docker compose stop` はデータを保持して停止します。

配布物では `target/release/yyprojectw.exe` を `yyprojectw.exe` に読み替えます。別のCargo出力先でビルドした場合は、その実行ファイルを指定してください。

既に環境変数 `YYPROJECT_DATABASE_URL` を設定している場合は `yyprojectw --postgres`、MCPは `yyproject --mcp-postgres [--read-only]` で直接接続します。接続文字列は環境変数で受け渡し、GUIの表示・監査に記録しません。Compose補助スクリプトはローカル接続を設定します。外部サーバーではTLS接続の接続文字列とOSの証明書設定を利用します。

GUIでは「ファイル → PostgreSQLへ接続（環境変数）」でも切り替えられます。アプリ専用の `yyproject` スキーマを使用します。

## SQLiteからの移行

移行先のYYproject領域が空であることが必要です。既存プロジェクトへ上書き・統合はしません。

```powershell
python tools/with_postgres.py target/release/yyproject.exe --migrate-to-postgres C:\projects\team.sqlite3
```

GUIではSQLiteを開き、「ファイル → 現在のSQLiteからPostgreSQLへ移行」を選べます。成功するとPostgreSQLへ接続を切り替えます。移行元のSQLiteを削除・変更することはありません。

元のID・親子階層・カテゴリ・依存関係・進捗履歴・プロジェクト版・監査全件を保持します。読み取り時点の整合したスナップショットを作り、移行先へ1トランザクションで適用します。移行中も元DBを使い続けた場合、そのスナップショット以降の変更は移行先へ自動反映しません。移行後は各GUI・MCPの接続先を切り替えてください。

## SQLiteへの書き出し

```powershell
python tools/with_postgres.py target/release/yyproject.exe --export-sqlite C:\projects\exported.sqlite3
```

GUIでは「ファイル → SQLiteへ書き出し」で実行できます。新規ファイルだけに出力し、既存ファイルは上書きしません。読み取り専用の整合したスナップショットから、監査も含めて書き出します。出力したファイルを `yyprojectw exported.sqlite3` または `yyproject --mcp exported.sqlite3` で利用できます。

## MCP

```powershell
python tools/with_postgres.py target/release/yyproject.exe --mcp-postgres
```

MCPクライアントから起動する場合は `--mcp-postgres` と、接続文字列を保持する `YYPROJECT_DATABASE_URL` を設定します。データベース接続先は起動時固定です。MCPへSQL・接続パスワード・移行先のファイルパスを渡す操作は公開しません。
