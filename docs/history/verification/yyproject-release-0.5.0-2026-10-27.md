# v0.5.0 リリース準備（2026-10-27）

親子タスクの階層表示と開閉、メニューの描画修正を公開する。版はワークスペース、Cargo.lockの自パッケージ、Windowsマニフェストを0.5.0にする。保存形式6、第三者依存と固定Git SHA、ライセンスは変更しない。

機能と検証は[階層表示](yyproject-task-outline-2026-10-27.md)と[メニュー描画](yyproject-sidebar-repaint-2026-10-27.md)を参照。機能コミットea82904の[CI](https://github.com/ykwyuta/YYproject/actions/runs/40496956387)ではLinuxとPostgreSQLが成功し、Windowsは描画の再入試験で失敗した。100% DPIの文字を2画素おきに間引いて判定すると細い文字の明るい画素を取りこぼすため、全画素の検査に変更する。文字の存在判定と背景色の検査は維持し、除外やallowで隠さない。失敗時のDPIと検出画素数も出力する。

修正・版更新後の最新SHAで、Windows／Linux／PostgreSQLの全CIジョブが完了して成功してから公開する。両OSの実ブラウザー19操作、Windows専用GUI試験も確認する。配布物は当該実行のWindows ZIP／Linux tar.gzを使用し、版番号、ヘルプ、文書、ライセンス、Gitの配布用デモ2個とSHA-256を検証する。利用者の変更済みデモDB2個はコミット・配布しない。公開対象SHA、CIとチェックサムはリリースノートに記録する。
