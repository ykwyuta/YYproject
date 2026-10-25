# 素材と共有資材

- `app/build.rs` とWindowsマニフェストは [yyeditor](https://github.com/ykwyuta/yyeditor/tree/a5c7fa2a43ec6ff531426610133688ff3875d3ff) のビルド資材を基にしています。アイコン生成コードは同リポジトリのYYスタイルを基にyyproject用に整理しています。GPL-3.0-or-laterを維持します。
- Windowsのフォント、ヘルプ、描画補助は同じコミットの `yy-ui-win` をGit依存で使用します。UI本体はここに複製しません。依存の固定値は `app/Cargo.toml` と `Cargo.lock` に記録しています。
- 埋め込むUDEV GothicにはSIL Open Font License 1.1が適用されます。[フォントのライセンス](licenses/LICENSE-UDEVGothic.txt)を配布物に含めます。
- ヘルプのWeb表示はMicrosoft Edge WebView2を利用します。ランタイムがない場合は共通ヘルプのテキスト表示を使用します。追加の許可は [COPYING.EXCEPTION](COPYING.EXCEPTION) を参照してください。
- SQLiteはrusqliteのbundled機能で同梱します。Rust依存の正確な版と取得元は `Cargo.lock` を参照してください。
