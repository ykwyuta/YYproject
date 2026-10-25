# 作業ルール

- このリポジトリは `core` と `app` のCargoワークスペース。変更前に `.github/workflows/ci.yml` を読み、WindowsとLinuxの検証を区別する。
- `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked` を実行する。警告を広いallowや試験の除外で隠さない。
- MCPはWindows／Linux、GUIはWindows専用。Linuxで成功してもGUIを検証済みと扱わない。`tools/test_mcp.py` で実際のstdioを、Windowsでは `yyprojectw --ui-smoke-test NEW_DIRECTORY` で専用DBと所有ウィンドウを使って確認する。
- UI試験の完了は期限付きの実状態で判定し、固定sleepで代用しない。実際のDPI、クライアント領域、スクロール位置を使う。所有するプロセスを終了・回収し、利用者のDB・設定を変更しない。
- `yy-ui-win` はyyeditorの公開済み完全コミットSHAを指定するGit依存。兄弟ディレクトリへのpath依存やUI本体の複製を追加しない。依存更新時はCargo.lockを更新し、Windows GUIとLinux MCPの両方を再検証する。yy-encoding本体はYYLibraryで管理し複製しない。
- SQLite更新ではトランザクション、プロジェクト版による競合検出、監査記録を維持する。データ破損や循環依存の検証を省略しない。
- 操作説明は `docs/user` と `help`、現行内部仕様は `docs/developer`、提案・進捗・検証記録は `docs/history` に置く。READMEは入口にする。リンク、ヘルプ埋め込み、配布物のライセンスも確認する。
- CIの成功は最新SHAの該当ジョブが完了してから報告する。進行中・未実行・スキップを成功と言わない。
