# yyproject

タスク・課題・進捗をSQLiteに保存するプロジェクト管理アプリです。

Windows GUI: `yyprojectw [DATABASE]`

Windows / Linux MCP: `yyproject --mcp DATABASE [--read-only]`

サンプル作成: `yyproject --init-demo NEW_DATABASE`

バージョン: `yyproject --version`

SQLiteは同梱しています。MCPの標準出力はJSON-RPCメッセージ専用です。

## データの扱い

同じホスト上のGUIとMCPは、ローカルディスク上の同じデータベースを開けます。ネットワーク共有上のDBは使用しないでください。遠隔DB共有サービスは提供しません。更新時はプロジェクトの版番号を確認し、競合時には上書きせず再読み込みを求めます。変更は即座にトランザクションで保存します。WALモードを使用するため、起動中にSQLite本体ファイルだけをコピーせず、終了後にバックアップしてください。

工数は時間、進捗は0～100%です。完了時は100%になり、中止した項目は集計対象外です。バーンダウンは残工数、バーンアップは完了工数と総工数を表示します。同日の記録は最新状態に更新し、通常の編集では過去の日付の記録を変更しません。MCPのcaptureで日付を明示すると、その日の記録を現在の集計値に置き換えます。過去データがない期間の実績は生成しません。

依存関係は先行項目の完了後に後続項目を開始する関係です。循環・不明な項目への参照を拒否します。期間は営業日単位で、0は1営業日として計算します。日程再計算は土日・休日マスタを除き、期間と先行項目の終了・待機営業日から開始日と期限を計算します。担当者のリソース平準化は行いません。

## MCP設定

MCPクライアントの設定例（パスは実環境に置き換えてください）:

```json
{"mcpServers":{"yyproject":{"command":"yyproject","args":["--mcp","/path/to/projects.sqlite3"]}}}
```

`project_list` / `project_get` でプロジェクトと版を読み、`project_create` で新規作成します。`project_change` には `project_id`、`expected_revision`、`actor`、`change` を指定します。

```json
{"project_id":1,"expected_revision":1,"actor":"AI","change":{"operation":"save_task","task":{"title":"要件を整理する","estimate":8,"duration_days":1,"status":"todo"}}}
```

変更操作は `rename`、`save_task`、`remove_task`、`save_master`、`remove_master`、`add_dependency`、`remove_dependency`、`schedule`、`capture`。新規タスク・マスタのIDは0です。既存タスクは全項目置換なので、まず現在のタスクを取得してください。日付はYYYY-MM-DD。マスタの種類はcategory・assignee・holidayで、休日にはdateが必要です。使用中のカテゴリ・担当者を削除すると失敗します。

`task_list` はstatus・kindによる絞り込みとoffset・limitのページング、`audit_list` は変更履歴のページングに対応します。スキーマの詳細は `tools/list` を参照してください。

プロジェクトは最大20,000項目・100,000依存関係、日程は最長10年です。MCPの1メッセージ上限は2MiBです。任意SQLの実行や、クライアントからのDBパス切り替えは提供しません。
