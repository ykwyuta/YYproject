# yyprojectの内部構成

このリポジトリは独立Cargoワークスペース（Rust 1.92以上）。`core` がSQLite・モデル・検証・日程・履歴、`app` がstdio MCPとWindowsネイティブUIを持つ。GUIはRust／Win32・GDIで実装し、ブラウザーやJavaScriptで業務処理を行わない。

既存資材は `yy-ui-win` の同梱UDEV Gothic・ヘルプ・BMP出力、yyeditor由来のビルドスクリプト（アイコン／DPI／Common Controlsのマニフェスト）、YYアイコン体系を使う。`yy-ui-win` はyyeditorの公開済みコミットをGit依存として参照し、UI・フォント・プレビュー本体を複製しない。ビルド補助とアイコン生成は本リポジトリの `app/build.rs` と `tools/gen_icon.py` で管理する。由来は [NOTICE.md](../../NOTICE.md)。

## 保存

SQLite 3はrusqliteのbundled機能で同梱。application_idは1499025482、user_versionは2（旧形式1も読込可能）。他アプリのDBや未知の版は拒否する。WALとforeign_keysを有効にする。

`projects(id,name,revision,document)` はプロジェクト集約をJSONとして保存する。タスク・マスタ・依存関係・日ごとの集計を1トランザクションで検証・保存するため、途中状態を読ませない。`audit(id,project_id,at,actor,revision,change)` にコミットと同じトランザクションで操作を追記する。監査時刻はUTC、日単位の進捗は実行環境のローカル日付を使う。

更新はBEGIN IMMEDIATE内で読み込んだ版とexpected_revisionを比較する。古い版は更新せずConflictを返す。IDはプロジェクト内で単調増加し、削除後に再利用しない。マスタ参照、日付、工数、進捗、重複依存、循環を保存前に確認する。参照中のカテゴリ・担当者を削除できない。

1プロジェクト20,000項目・100,000依存関係・文書16MiB。プロジェクト日程は10年以内。集約を読むため、大規模案件の更新は項目単位のSQL更新にはならない。この制約と監査ログの増加を考慮する。日程計算はトポロジカル順序と休日集合を使い、完全な週を飛ばして営業日を進める。担当者負荷の平準化、複数種の依存、クリティカルパス解析は行わない。

## MCP

[公式stdio仕様](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)に沿った改行区切りのJSON-RPC。stdoutにログを混ぜない。initialize／initialized、ping、tools/list、tools/callを提供する。[tools仕様](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)に沿ってJSON Schemaと注釈を返す。業務エラーはisError、壊れたJSONはParse Errorとして扱う。1メッセージ2MiBを超えた場合は次の行まで読み捨てて同期を回復する。

DBパスとread-onlyは起動時固定。任意SQL・HTTPポート・シェル実行・クライアント指定パスへの書き込みは公開しない。GUIと同じStoreへ構造化されたChangeを渡す。

## ビルド

Rust 1.92以上を使用します。CIはstableで検査します。WindowsはVisual Studio Build Tools（MSVC C++とWindows SDK）、LinuxはCコンパイラと標準ビルドツールが必要です。SQLiteは同梱ソースからビルドします。共通UIとYYLibraryは公開Gitリポジトリから取得され、兄弟ディレクトリのチェックアウトは不要です。

```sh
cargo build --workspace --release --locked
python tools/package.py
```

配布物は `target/dist` に作成します。WindowsはGUIとMCPの2バイナリ、LinuxはMCPのみを含みます。ヘルプ、文書、ライセンスも同梱します。アイコンの再生成にはPillowを導入して `python tools/gen_icon.py` を実行します。

## 検証

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
python tools/test_mcp.py target/debug/yyproject.exe
```

Linuxでは実行ファイルの.exeを外す。GUIはWindows専用で、Linuxの成功をGUIの検証済みとは扱わない。Windowsの `yyprojectw --ui-smoke-test NEW_DIRECTORY` は専用DB・所有するウィンドウだけを使い、入力ダイアログ・マスタ・8画面・サイズ変更・カンバンドラッグを検証する。ディレクトリ内に報告と実描画関数によるBMPを出力する。完了判定はモデル内容と描画範囲。表示待ちの固定sleepは使用しない。

## デモ資材の更新

`demo/scenario.json` が題材と時系列の出来事、`app/src/demo.rs` がStore経由の再生処理、`demo/yyproject-demo.sqlite3` が配布DBです。`yyproject --init-demo NEW_DATABASE` で別名へ生成し、すべての接続を閉じてから差し替えます。WAL／SHMファイルをコミットしません。デモ生成・既存ファイル上書き拒否・同梱DBの内容はRust試験で、実MCPと3案件×8画面×2サイズのGUI描画は各スモーク試験で検証します。データを変える場合は `demo/README.md` の件数も更新してください。

配布用DBは、読み取り専用の場所でも参照できるようDELETEジャーナルモードで確定します。新しく生成したDBの全接続を閉じ、Python等で `PRAGMA journal_mode=DELETE` を実行して接続を閉じてから差し替えます。配布処理はデモのWAL／SHMが存在する場合に失敗させます。

## 運用シナリオの検証

```powershell
python tools/validate_operations.py target/release/yyproject.exe
```

Linuxは.exeを外す。元DBは触らず、各ケースは独立した一時コピーを実MCPプロセスで更新する。競合・不正参照・循環・read-onlyを保護として確認し、再開、依存、日程、納期、状態/項目、課題工数、履歴補正、削除を検証する。gapは現行仕様の不足として報告し、終了コード0を運用適合の意味にしない。保護の不成立や手順の実行失敗は例外で失敗する。

`target/operations-validation` へJSONとMarkdownを出力する。担当者負荷はscenario.jsonの上限と残工数を残営業日に均等配分した仮定で、実稼働や製品内のリソース計算ではない。GUIスモークはBMPに加えてDPI・範囲・ヒット数・描画関数の単回時間をJSONへ保存する。子コントロールを除くキャンバス描画であり、対話性能ベンチマークではない。結果は日付付きでdocs/historyへ保存し、現行操作の説明と区別する。

## 状態遷移と再計画の共通制約

Storeの更新トランザクション内で、完了から再開する時の進捗100%と、未完了先行項目がある後続の新規完了を拒否する。拒否時は版・監査・進捗を更新しない。既存の完了項目への属性編集は可能。Cancelledの先行項目も完了ゲートを通さない。

Project::schedule_previewはコピー上で計算し、版・差分・納期超過IDを返す。日付指定済みのDone/Cancelledは固定。Change::Scheduleはallow_overrun（serde既定false）を持ち、納期超過の無承認保存を拒否する。既存JSONのoperation:scheduleは互換。MCPのschedule_previewはreadOnlyHint:trueで公開する。GUI確認中も更新時CASを維持する。

TaskFilterのAND判定をGUI一覧/カンバンとMCPで共有する。ガント・ネットワーク・チャートはフィルター対象外。未割当と未完了は別の条件で、GUIの未割当モードは両方を指定する。期限の当日は超過に含めない。

## 分類・レビュー・集計と保管

TaskへPriority/Severity/ReviewStateとreviewer、review_note、include_in_effort、related_tasks、archivedを追加した。serdeのコンテナ既定値で従来のJSONを読む。既存工数の算入を維持するためTask::defaultのinclude_in_effortはtrue。GUIの新規課題とデモ課題のみfalseを明示する。Task::remainingとProject::snapshotは除外・保管項目を計上しない。

reviewerは担当者マスタ参照。レビューありなら確認者必須、承認・差し戻しなら非空の理由必須、DoneとPending/ChangesRequestedの組合せは禁止する。承認者本人の認証や権限制御はない。related_tasksは課題からTaskへの最大1000件の一意参照で、保管項目への参照は保持する。

RemoveTaskはarchived=trueにする保管。RestoreTaskはfalseへ戻す。項目と依存レコードを消さず、全体DAGとマスタ参照の検証も継続する。active_dependenciesは保管項目へ接続する関係を除外し、日程・描画・完了ゲートに利用する。マスタ削除は保管項目の参照も検査する。保管項目は文書サイズ/項目数上限に含む。removal_previewは読取専用で版・項目・依存・関連課題を返す。

新規DBは形式2。形式1への書き込み成功時に、同じトランザクションでuser_versionを2へ更新する。読み取りと拒否された書き込みでは形式を変更しない。旧アプリの形式チェックにより、更新後の新項目を旧アプリが落として保存することを防ぐ。SQLiteのテーブル構成は変更していない。プロジェクトJSON出力には保管項目も含める。
