# yyprojectの内部構成

このリポジトリは独立Cargoワークスペース（Rust 1.92以上）。`core` がSQLite／PostgreSQL・モデル・検証・日程・履歴、`app` がstdio MCPとWindowsネイティブUIを持つ。GUIはRust／Win32・GDIで実装し、ブラウザーやJavaScriptで業務処理を行わない。

既存資材は `yy-ui-win` の同梱UDEV Gothic・ヘルプ・BMP出力、yyeditor由来のビルドスクリプト（アイコン／DPI／Common Controlsのマニフェスト）、YYアイコン体系を使う。`yy-ui-win` はyyeditorの公開済みコミットをGit依存として参照し、UI・フォント・プレビュー本体を複製しない。ビルド補助とアイコン生成は本リポジトリの `app/build.rs` と `tools/gen_icon.py` で管理する。由来は [NOTICE.md](../../NOTICE.md)。

## 保存

SQLite 3はrusqliteのbundled機能で同梱。application_idは1499025482、user_versionは3（旧形式1・2も読込可能）。他アプリのDBや未知の版は拒否する。WALとforeign_keysを有効にする。

`projects(id,name,revision,document)` はプロジェクト集約をJSONとして保存する。タスク・マスタ・依存関係・日ごとの集計を1トランザクションで検証・保存するため、途中状態を読ませない。`audit(id,project_id,at,actor,revision,change)` にコミットと同じトランザクションで操作を追記する。監査時刻はUTC、日単位の進捗は実行環境のローカル日付を使う。

更新はBEGIN IMMEDIATE内で読み込んだ版とexpected_revisionを比較する。古い版は更新せずConflictを返す。IDはプロジェクト内で単調増加し、削除後に再利用しない。マスタ参照、日付、工数、進捗、重複依存、循環を保存前に確認する。参照中のカテゴリ・担当者を削除できない。

1プロジェクト20,000項目・100,000依存関係・文書16MiB。プロジェクト日程は10年以内。集約を読むため、大規模案件の更新は項目単位のSQL更新にはならない。この制約と監査ログの増加を考慮する。日程計算はトポロジカル順序と休日集合を使い、完全な週を飛ばして営業日を進める。担当者負荷の平準化、複数種の依存、クリティカルパス解析は行わない。

## MCP

[公式stdio仕様](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)に沿った改行区切りのJSON-RPC。stdoutにログを混ぜない。initialize／initialized、ping、tools/list、tools/callを提供する。[tools仕様](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)に沿ってJSON Schemaと注釈を返す。業務エラーはisError、壊れたJSONはParse Errorとして扱う。1メッセージ2MiBを超えた場合は次の行まで読み捨てて同期を回復する。

DBパスとread-onlyは起動時固定。任意SQL・HTTPポート・シェル実行・クライアント指定パスへの書き込みは公開しない。GUIと同じStoreへ構造化されたChangeを渡す。

## ビルド

Rust 1.92以上を使用します。CIはstableで検査します。WindowsはVisual Studio Build Tools（MSVC C++とWindows SDK）、LinuxはCコンパイラと標準ビルドツール、pkg-config、OpenSSL開発ヘッダーが必要です。SQLiteは同梱ソースからビルドします。共通UIとYYLibraryは公開Gitリポジトリから取得され、兄弟ディレクトリのチェックアウトは不要です。

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

新規DBは形式4。形式1・2・3への書き込み成功時に、同じトランザクションでuser_versionを4へ更新する。読み取りと拒否された書き込みでは形式を変更しない。旧アプリの形式チェックにより、更新後の新項目を旧アプリが落として保存することを防ぐ。SQLiteのテーブル構成は変更していない。CSV出力の保管項目はフィルターで選ぶ。SQLiteへの書き出しは保管項目も監査も含める。

## カンバンの入力と描画

カードのドラッグは押下位置・カード内の相対位置・現在位置・移動開始閾値を保持する。WindowsのDPI別ドラッグ閾値を超えるまでは選択だけを行い、マウスキャプチャ中のWM_MOUSEMOVEで追従表示を更新する。50msの専用タイマーで実クライアント領域の端からスクロールし、移動先は現在のスクロール位置を加えて判定する。画面外へのドロップ、Esc、右クリック、キャプチャ喪失、アプリ非アクティブ化、表示・案件切替、再読込で破棄する。タイマーとキャプチャを解放した後に既存のset_statusを呼び、完了ゲート・再開進捗・版競合・監査を維持する。同じ状態への移動は保存しない。

描画はメモリDCを介し、移動先の枠とカードのプレビューを最後に重ねる。カードの件名は2行、付加情報は別行とし、寸法はDPIに追従する。一覧の内容幅と描画は共通の列幅配列を使い、右端列まで水平スクロールできる。右クリックメニューは既存の編集・状態変更コマンドを呼ぶ。

## 階層・グリッド・ガント・CSV

Task.parentは任意の親タスクID。親はTask種別だけに限定し、循環・欠損親を拒否する。ordered_tasksは反復DFSで兄弟の保存順を維持し、深い階層でもコールスタックを使わない。Master.parentはカテゴリのみで、根を含めて最大3階層。親カテゴリでの絞り込みは子孫を含む。階層は集約や依存関係とは別で、自動集計・自動完了は行わない。

core/table.rsの共通列定義をGUI・コピー・CSV・列値フィルターで使う。一覧のカテゴリは件名の直前。native/grid.rsはセル選択・EDITコントロール・F2/Enter/Esc/Tab・矢印・Ctrl+C/V/A・Deleteを扱う。clipboardは固定SHAのyy-ui-win公開APIを使用し、yysheetの操作規約（sheet/mod.rsの範囲コピーとF2編集）を参照した。UI本体は複製していない。編集の版を保持し、外部更新後の古いセル編集を拒否する。SaveTasksは既存ID最大2000件を依存順に適用し、最後に全体を検証する。Storeの1トランザクション内なので一部の失敗で全部をロールバックする。MoveTaskは子孫をまとめて移動先の兄弟として並べ替え、子孫内への移動は拒否する。

native/timeline.rsは年・月・日・曜日の4段見出し、表示起点、可変の名前列と日幅、基準日と進捗率を結ぶ稲妻線、マイルストーンの菱形を描画する。milestoneはTaskのみで、同じ開始・期限、期間0、工数0を必須とする。表示設定はウィンドウの状態として保持する。

CSVはUTF-8 BOM、CRLF、引用符の二重化で出力し、タブ区切りのクリップボードも同じ引用処理で改行を保持する。列選択とフィルターはGUIで指定する。CSVは項目交換用で、プロジェクト・依存・監査全体の復元形式ではない。

## Jira用CSVの実装

Jira用CSVはcore/table.rsのexport_jira_csvを使う。Jira Cloudの既存の管理者CSV取り込み向けにSummary・Issue Type・Issue ID・Parentを必須とし、追加列を限定する。フィルター一致項目の祖先閉包を含め、Epic、標準課題、Sub-taskの順に安定ソートする。YYprojectのIDは取り込み内の一時IDで、Jira Issue Keyは出力しない。選択した2／3階層を超えた場合や不適合のSummaryは保存前に拒否する。GUIの出力フォームは検証時点のスナップショットを使い、DBは更新しない。

## PostgreSQL接続と移送の実装

StoreはSQLiteとPostgreSQLを振り分ける。PostgreSQLは同期postgresクライアントとnative-tlsを使用し、OSの証明書ストアを利用する。接続文字列はYYPROJECT_DATABASE_URLで受け渡し、表示・監査に含めない。専用yyprojectスキーマのmetaにapplication_id・形式版4・変更通番を保持する。接続初期化はadvisory lock、更新はSELECT FOR UPDATEとexpected_revision、監査・変更通番は同じトランザクションで適用する。GUIは変更通番を読み、他接続からの更新を再読込する。接続・文・ロックのタイムアウトを設定する。read-onlyモードはクライアントと接続の双方で更新を拒否する。

移送はDatabaseDumpを介し、SQLiteは読み取りトランザクション、PostgreSQLはREPEATABLE READの読み取り専用トランザクションでスナップショットを得る。プロジェクトと監査全件を検証し、空の移行先に1トランザクションで挿入する。ID・版・日時・履歴を保持し、PostgreSQLの採番シーケンスも更新する。既存データへの統合や上書きはしない。SQLiteへの出力は同じフォルダの一時DBへ構築・チェックポイント後、persist_noclobberで新規ファイルとして確定する。

compose.yamlはPostgreSQL 17、ループバックの15432ポート、healthcheck、永続ボリュームを定義する。tools/with_postgres.pyはローカルの.envを読み、接続秘密を表示せず子プロセスへ渡す。.envは配布しない。tools/test_postgres.pyは空の専用DBを前提に実MCP、2接続の競合、読み取り専用、失敗時の原子性、100件超の監査、双方向移送を検証する。CIにはLinuxのPostgreSQLジョブを追加した。Windows GUIは別途 --ui-smoke-test NEW_DIRECTORY、PostgreSQL接続では --ui-smoke-postgres NEW_DIRECTORY を使用し、後者は空の専用DBが必要。

## 一覧編集・寸法・日次描画の拡張

native/grid.rsはMASTER_COLUMNSを含む共通セル選択・編集を扱い、master_grid.rsはマスタ描画と一括貼り付けを扱う。SaveMastersは既存の一意ID最大2000件を一括置換し、最終文書を検証して1トランザクションで保存する。MCPのproject_changeにも同じoperationを公開する。

native/sizing.rsはDPI・現在のスクロール・動的な各列幅とIDごとの行高から境界を判定する。ドラッグ開始寸法を保持し、Esc・キャプチャ喪失・表示切替で復元、ボタン解放で確定する。寸法はDBへ書かない。native/bulk.rsは範囲選択とCtrlによる行ID集合を扱う。batch_task_attributesは選択IDだけへ属性を適用し、Project.changeの複製による事前検証と実保存の同じ業務条件を使う。保存はフォームを開いた時点の版で競合検出する。

Task.completed_onは任意の実績完了日で、旧文書の省略値はNone。予定のdue、状態、進捗とは独立する。形式4で保存し、SQLiteは形式1〜4、PostgreSQLは形式3〜4を読み、正常な更新・移送時だけ形式4へ上げる。旧アプリは形式4への接続を拒否する。

native/charts.rsは1日20論理px以上の横スクロール可能な格子を作り、7日ごとに日付を表示する。休日はProject.is_workday、線幅は3論理pxを使用。依存ネットワークの選択系統は先行・後続を方向別に探索した和集合とし、他の分岐を巻き込む無向連結成分にはしない。

## Streamable HTTPとPAT

http_server.rsはAxumの/mcpでPOSTのJSON応答、通知202、GET405、DELETE204を扱う。mcp::Sessionはstdioと共通の初期化・ツール処理を保持する。同期Storeはspawn_blocking上のMutexで操作し、PostgreSQLの同期ランタイムをTokioの非同期文脈で実行しない。初期化ごとにOS乱数256bitのID、合意したプロトコル版、ユーザー名、PATハッシュ、最終要求時刻を保持する。30分無操作で破棄し、最大256セッション、同時処理待ち64要求、本文2MiB、本文読取15秒を制限とする。JSON応答形式なのでSSE・再送イベントは公開しない。

pat.rsはTOMLのusersを検証し、毎要求のBearer PATをSHA-256化して定数時間で比較する。設定不正は503で拒否する。セッションを名前とPATハッシュへ結び付け、別ユーザーや更新後PATからの流用を拒否する。HTTPの書き込みツールはactorを認証名へ固定し、ユーザーread_onlyとStore全体のread_onlyを併用する。PAT生成は256bit OS乱数、同一ユーザー更新、排他サイドカーロック、一時ファイルの同期と置換を行い、平文PATを設定へ保存しない。ホスト・Origin許可設定は起動時固定、ユーザー設定は毎要求再読込。PATはDBではなくサーバーの設定ファイルで管理する。

tools/test_http.pyは実プロセス・一時設定・専用DBを使い、期限付きでエンドポイント起動を待ち、終了時にプロセスを回収する。PAT更新、なりすましactorの防止、読み取り専用、Origin/Host、HTTPヘッダー、セッション、同時更新の競合、2MiB上限を検証する。--postgresで既存の専用テストDBにも接続する。

## 種類別マスタと同列コピー

マスタの子タブはmaster_kindで排他的に切り替え、master_columns(kind)から必要な列だけを得る。grid_rows・描画・ヒット判定・編集・貼り付けを同じ種類とID列で処理し、プロジェクト全体のmasters添字を表示添字と混同しない。切替時は編集中の保存を検証してから選択・スクロールを解除する。

BULK_TASK_KEYSをGUI一括フォーム・複数行貼り付け・クリア・fill_tasks_downの共通許可項目とする。個別セルの編集可能性と一括変更可能性を分離し、件名などは複数行の操作で拒否する。Ctrl+Dとメニューのfill-downは可視順の先頭選択行をコピー元とする。参照マスタは表示名ではなくIDでコピーし、同名担当者も取り違えない。1セルの貼り付けは同列の選択行へ展開する。SaveTasks／SaveMastersの最終検証・版競合・監査の原子性は維持する。

チャートの日付軸は上から曜日・日・月・年とし、曜日は日次、日の数字は7日ごと、月・年は境界で区切る。休日背景は曜日・日の段まで描画し、月・年の表示を塗りつぶさない。
