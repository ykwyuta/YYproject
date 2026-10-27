# Web UI追加と実操作検証（2026-10-27）

Linux／Windowsの既存 `yyproject` に静的画面と認証済みAPIを内蔵した。`--web`／`--web-postgres` でWeb UIとPAT付き `/mcp` を同じポートに提供する。通常のMCP起動ではWeb画面を公開しない。配信用のNode.jsは不要。起動・認証設定は[操作ガイド](../../user/web-ui.md)を参照。

## 操作とデータ

15画面、種類別一覧とマスタ、個別フォーム、セル範囲コピー・貼り付け・同列コピー、一括属性編集、ID行とカンバンカードのドラッグ、列幅・行高変更、右クリック絞り込み・子タスク追加を実装した。ガント、日次チャート、系統強調、CSV／Jira CSV、手動やりとり記録、SQLiteバックアップも提供する。ネイティブと共通の列定義・変換・一括許可属性・Storeを使い、版競合、監査、トランザクション、依存検証を維持する。

パスワードはPATと同じ設定更新手順で256bit乱数から発行し、設定にはハッシュだけを保存する。PATとは独立したユーザー表を持ち、パスワードだけでログインする。HttpOnly／SameSite Cookie、CSRFとOrigin、認証変更によるセッション失効、読み取り専用を検証した。短い任意パスワードの登録機能ではない。外部公開時はHTTPSとSecure Cookieを設定する。

## 実施結果

ベースコミットは `ca10b9615975c21ad805948729c0573ec0602103`。ここに記録した結果はローカル検証であり、GitHub CIの実行・成功や公開リリースを表す記録ではない。

|検証|Windows|Linux|
|---|---|---|
|fmt／全workspace・all-targets Clippy（警告エラー）|成功|成功|
|全workspace Rust試験|36件成功|35件成功|
|実stdio MCP、PAT Streamable HTTP|成功|成功|
|Web認証・API・競合・監査・SQLiteバックアップ|成功|成功|
|専用PostgreSQL DBでWeb API|成功|成功|
|実ブラウザー操作|Edge、17項目成功|Chromium、17項目成功|
|既存ネイティブGUI専用DB smoke|成功|対象外|

LinuxはDocker上のRust 1.92とPlaywright 1.62.1を使用した。最終ブラウザー・stdio・HTTP試験は配布用releaseバイナリで実施した。Windowsは専用debugバイナリと所有するheadless Edgeを使用した。ブラウザー版の実値は[Windows結果](evidence/yyproject-web-windows.json)と[Linux結果](evidence/yyproject-web-linux.json)に記録している。

17項目は実際のCtrl+C/V、引用符付き複数行、F2/Enter、Ctrl+D、担当者一括変更、禁止項目、一行のIDドラッグ、列・行境界のポインター操作、見出しメニュー、CSVダウンロード、各種類フォーム・統計、マスタ編集、右クリック子タスク、カードドラッグ、ガント4段軸・稲妻・境界、ネットワーク系統、再ログイン不要の再読込、参照専用を含む。実状態への期限付き待機を使い、固定sleepでは判定しない。1440×1000と1024×768で確認した。スクリーンショットだけを操作成功の根拠にはしていない。

Windowsの再実行では既定TEMPへのアクセス拒否が1回発生し、作業領域の専用TEMP/TMPを設定して全件再実行・成功した。Linux検証コンテナにはrustfmt／clippyを明示導入した。警告のallow追加や試験除外は行っていない。

既存GUIは専用ディレクトリで `yyprojectw --ui-smoke-test` を実行し、所有プロセスの終了コード0と[実状態ログ](evidence/yyproject-web-native-ui.txt)を確認した。Linuxの成功をWindows GUI検証として扱っていない。利用者の稼働中実行ファイルや変更済みデモDBは更新しない。配布物はHEADの基本デモを使う分離ソースから作成し、認証情報は含めない。

## 画面の目視確認

- [Linuxカンバン](images/yyproject-web-linux-kanban.png)
- [Linuxガント](images/yyproject-web-linux-gantt.png)：初期行高を52pxとし予定日程の見切れを解消。
- [Linux暫定対応統計](images/yyproject-web-linux-statistics.png)：日次格子、週表示、休日色、整数件数目盛り。
- [Windows一覧1024×768](images/yyproject-web-windows-grid.png)

ネイティブと共通操作・保存規則を実装したが、ブラウザー固有の描画やクリップボード権限は異なる。HTTP等でClipboard APIを使用できない場合もキーボード操作と貼り付けダイアログを提供する。DB接続先変更・双方向移行は管理CLIを使用する。ブラウザーでの試験対象は上記2エンジンである。

CIにはWindows／Linuxの実ブラウザー試験とPostgreSQL Web試験、結果アップロードを追加した。第三者Rust依存と固定Git SHA、保存形式、既存ライセンスは変更していない。
