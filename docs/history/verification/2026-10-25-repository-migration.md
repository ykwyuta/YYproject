# YYprojectリポジトリへの分離

2026-10-25。yyeditorの未コミットのyyproject初期実装を [YYproject](https://github.com/ykwyuta/YYproject) で管理する方針へ変更した。

移行対象は `core`、`app`、SQLiteモデル・MCP・Windows UI試験、ヘルプ、アイコンとマニフェスト、ビルド補助、文書、ライセンス。作業用DB・キャッシュ・デバッグ描画は配布ソースに含めない。日付付きの初期実装検証記録は移行前の記録として保持する。

`yy-ui-win` はyyeditorの公開済み `a5c7fa2a43ec6ff531426610133688ff3875d3ff` へ固定したGit依存へ変更し、兄弟ディレクトリを必要としない。UI・フォント・プレビュー本体を複製しない。ビルド補助とアイコン生成は本リポジトリで管理し、出自をNOTICEに記載する。アイコンの再生成結果は移行前とバイト一致した。

専用CIでWindows／Linuxの書式・Clippy・試験・リリースビルド・実stdio MCP、WindowsのUI試験、文書とライセンス込みの配布アーカイブ作成を実行する。ローカル検証の結果とGitHub CIの結果は区別して記載する。

## ローカル検証結果

- Windows x64／Rust 1.99.0 MSVC：fmt、全ターゲットClippy、モデル6試験・MCP2試験、リリースビルド成功。
- Windowsの新バイナリ：実stdio MCP、8画面を1100×700／640×480で描画、DPI192で実座標を使う操作試験、タスク・マスタ入力、AI変更の反映、プロジェクト切り替え、カンバンドラッグが成功。
- Linux x64／Rust 1.99.0 bookwormコンテナ：移行先だけを `/src` に読み取り専用で渡し、fmt、Clippy、同じ8試験、リリースビルド、実stdio MCPが成功。ホスト側yyeditorのチェックアウトはコンテナに渡していない。
- Windows配布ZIPの整合性と必須ファイル（GUI・MCP・ヘルプ・GPL・追加許可・フォントライセンス）を検証。文書リンクとCI YAMLの読み取りも成功。

この記録はローカル検証の結果であり、GitHub CI成功を意味しない。
