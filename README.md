# YYproject

タスク・課題・進捗をSQLiteで管理するRustアプリです。Windows GUI (`yyprojectw`) と、Windows／Linux向けstdio MCPサーバー (`yyproject`) を提供します。

<img src="app/res/yyproject-256.png" alt="YYproject" width="96">

カンバン、ガント、タスク／課題一覧、依存ネットワーク、バーンダウン／バーンアップ、カテゴリ・担当者・休日マスタに対応します。GUIとAIは同じモデルを使用し、更新競合を検出します。SQLiteとUDEV Gothicフォントは同梱されます。

- [使い方](docs/user/yyproject.md) / [同梱ヘルプとMCP設定](help/help.md)
- [開発・ビルド・検証](docs/developer/yyproject.md)
- [検討経緯・検証記録](docs/history/README.md)

```sh
cargo build --workspace --release --locked
# Windows GUI
./target/release/yyprojectw.exe
# Windows / Linux MCP (Windowsでは.exeを付ける)
./target/release/yyproject --mcp projects.sqlite3
```

サンプルDBは `yyproject --init-demo NEW_DATABASE` で作成できます。WindowsではMSVCとWindows SDK、LinuxではCコンパイラが必要です。[開発手順](docs/developer/yyproject.md)を参照してください。

本体のライセンスは [GPL-3.0-or-later](COPYING) です。WebView2との結合に関する[追加の許可](COPYING.EXCEPTION)と[素材・依存の案内](NOTICE.md)も参照してください。
