# yyprojectの内部構成

このリポジトリは独立Cargoワークスペース（Rust 1.92以上）。`core` がSQLite・モデル・検証・日程・履歴、`app` がstdio MCPとWindowsネイティブUIを持つ。GUIはRust／Win32・GDIで実装し、ブラウザーやJavaScriptで業務処理を行わない。

既存資材は `yy-ui-win` の同梱UDEV Gothic・ヘルプ・BMP出力、yyeditor由来のビルドスクリプト（アイコン／DPI／Common Controlsのマニフェスト）、YYアイコン体系を使う。`yy-ui-win` はyyeditorの公開済みコミットをGit依存として参照し、UI・フォント・プレビュー本体を複製しない。ビルド補助とアイコン生成は本リポジトリの `app/build.rs` と `tools/gen_icon.py` で管理する。由来は [NOTICE.md](../../NOTICE.md)。

## 保存

SQLite 3はrusqliteのbundled機能で同梱。application_idは1499025482、user_versionは1。他アプリのDBや未知の版は拒否する。WALとforeign_keysを有効にする。

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
