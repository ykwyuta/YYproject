# YYシリーズ開発部門の運用検証デモ

`yyproject-demo.sqlite3` は、3案件・90項目をそのまま開けるSQLite DBです。配布ZIP／tar.gzにもこのフォルダを含みます。**すべて架空のデータであり、実際のYYシリーズの進捗・障害・担当者・計画を表しません。**

| 案件 | タスク | 課題 | 履歴 | 状況 |
| --- | ---: | ---: | ---: | --- |
| YY安定化リリース | 48 | 16 | 8時点 | 8名の兼務チーム。納期据え置きの追加要求、外部待ち、差し戻し、承認待ち |
| YY緊急保守 | 12 | 4 | 3時点 | 同じSSH・QA・運用担当者を緊急投入し、通常開発と競合 |
| YY前回リリース | 8 | 2 | 3時点 | 全項目完了。現行案件との切り替え・比較用 |

カテゴリ・担当者・休日マスタは案件ごとに登録します。同じ名前の担当者もDB上は案件ごとの別マスタです。現行モデルには共通の稼働率・組織全体の負荷集計がないため、[scenario.json](scenario.json)の `role_capacity` に比較用の稼働上限を記載しています。

## 開く

空の作業ディレクトリでデモをコピーし、元データを残して操作します。リポジトリでビルドした場合:

```powershell
Copy-Item demo/yyproject-demo.sqlite3 my-operations.sqlite3
./target/release/yyprojectw.exe my-operations.sqlite3
```

配布ZIPなら同じ場所で `./yyprojectw.exe my-operations.sqlite3` を実行します。案件名を画面上部で切り替えてください。

MCPでは `yyproject --mcp demo/yyproject-demo.sqlite3 --read-only` で参照できます。Windowsでは `.exe` を付けます。操作・MCP設定は[ヘルプ](../help/help.md)を参照してください。

## 運用を検証する

- [運用シナリオと役割分担](operations-playbook.md)：進捗会議、外部待ち、差し戻し、追加要求、担当変更、リリース判定を実際に操作する手順
- [受入チェックリスト](acceptance-checklist.md)：確認できた制約と、人が追加測定する項目
- [検証結果と改善の優先順位](../docs/history/verification/yyproject-operations-2026-10-26.md)：MCPでの再現証拠、GUIの実描画例、現行設計で運用できる範囲
- [第1次改善の結果](../docs/history/verification/yyproject-improvements-2026-10-26.md)：検索・完了ゲート・再計画の安全性の再検証
- [シナリオ原本](scenario.json)：出来事の日付・主体・変更理由を含む再生成可能な入力

```powershell
python tools/validate_operations.py target/release/yyproject.exe
```

配布ZIPを使う場合は `python tools/validate_operations.py ./yyproject.exe`（Linuxは `./yyproject`）を実行します。Python 3の標準ライブラリだけを使います。

検証ツールは専用コピーだけを更新し、`target/operations-validation/operations.json` と `operations.md` に証拠を出します。`supported` は確認できた保護、`gap` は再現した不足です。終了コード0でも業務運用が合格したことにはなりません。

## 日付を更新して再生成する

```powershell
./target/release/yyproject.exe --init-demo new-operations.sqlite3
```

生成日を基準に、シナリオの相対日付で同じ出来事を再生します。既存ファイルは上書きしません。同梱DBの日付は固定で、開くたびに現在へ移動しません。

履歴チャートは各時点の集計値です。過去のタスク全体をGUIで巻き戻す機能ではありません。出来事の業務日付はシナリオと説明・監査のactorへ明記しますが、監査の `at` は実際のDB書込時刻です。過去日に監査時刻を偽装していません。

## 分類・レビューと工数の例

主案件のPRJ-03は確認待ち、SHT-02は差し戻しとして確認者と理由を設定しています。ISS-001は最優先・致命的で、SSH-02/SSH-03を対応タスクとして関連付けています。デモの課題は工数集計から除外しています。元DBをコピーし、課題の見積もりを増やしてもチャートの総工数が増えないこと、明示的な算入設定で増えることを試せます。

[分類・工数・保管復元の第2次改善結果](../docs/history/verification/yyproject-improvements-2-2026-10-26.md)も参照してください。
