# v0.4.0 リリース準備（2026-10-27）

パスワード認証付き内蔵Web UIとWeb版に合わせたWindowsネイティブGUIのデザイン更新を機能リリースとして公開する。ワークスペースとCargo.lock内の自パッケージ、Windowsマニフェストの版を0.4.0へ更新する。保存形式6、第三者依存と固定Git SHA、ライセンスは変更しない。

機能コミット `6cf778480cd4479bdde02363313c66dc89a7891f` の[CI](https://github.com/ykwyuta/YYproject/actions/runs/40451865396)はWindows・Linux・PostgreSQLの全ジョブが完了し成功した。Webの実ブラウザー操作とWindows専用GUI試験も成功している。詳細は[Webの検証](yyproject-web-ui-2026-10-27.md)と[ネイティブ版の検証](yyproject-native-web-design-2026-10-27.md)を参照。

版更新後の最新SHAでCIが完了・成功してから、その実行のWindows ZIPとLinux tar.gzを取得する。配布物の実行ファイルの版、ヘルプ、文書、ライセンス、配布用デモとSHA-256を確認し、同じSHAを指すv0.4.0タグとGitHub Releaseを作成する。CIの具体的な実行とチェックサムはリリースノートへ記録する。利用者の変更済みデモDBをコミット・配布しない。

Webは `--generate-password USER --auth-config FILE` でパスワードを発行し、`--web DATABASE --auth-config FILE` または `--web-postgres --auth-config FILE` で起動する。外部公開時はHTTPSとSecure Cookieの設定が必要。PATは独立しており同じポートの `/mcp` で利用できる。任意の短いパスワード登録ではなく、PATと同じ手順による高エントロピーの発行値を使用する。
