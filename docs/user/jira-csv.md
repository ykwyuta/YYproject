# Jira用CSV出力

「ファイル → Jira用CSVを出力」で、Jira Cloudの会社管理プロジェクトへ新しい課題を登録するためのCSVを保存できます。Jiraへの接続・登録はこの操作では実行しません。

## 出力する内容

「出力条件」で検索語と階層を選びます。現在のカテゴリ・担当者・状態・列の絞り込みも適用します。条件に一致した項目に加え、その親・祖先を出力します。祖先は条件外や保管済みでも含め、親IDが欠けないようにします。祖先の説明なども出力対象なので、出力内容を確認してください。

- 3階層: 最上位タスクをEpic、子をTask、孫をSub-taskにします。単独の最上位タスクもEpicになります。
- 2階層: 最上位タスクをTask、子をSub-taskにします。
- 最上位の「課題」はBugにします。

「課題タイプ」で各タイプ名を取り込み先の名前に変更できます。選択した階層より深い項目が出力対象に含まれる場合はエラーにし、平坦化や親子関係の削除はしません。孫があるプロジェクトは3階層を選んでください。JiraのSummaryに合わせ、件名は改行なし・255文字以内で出力します。

必須列はSummary（件名）、Issue Type、Issue ID（YYprojectの項目ID）、Parent（親の項目ID）です。親の階層を先に、同じ階層では一覧の順に並べます。Issue IDはCSV内の関係を表す一時IDで、既存Jira課題のキーではありません。この機能は既存課題の更新・重複回避には使えません。

「出力列」でDescription（説明）、Status（状態）、Priority（優先度）、Assignee（担当者）、Due Date（期限）、Component（カテゴリのフルパス）を選びます。担当者は初期設定では出力しません。出力する場合の値はYYprojectの担当者名なので、JiraのアカウントID／メールアドレスなど取り込み先で解決できる値へCSV上で対応付けてください。状態・優先度はYYprojectの表示名を出力し、取り込み時にJiraの値へ対応付けます。カテゴリは1つのComponent名として扱い、Jira側にカテゴリの3階層は作りません。マイルストーンは通常の項目として扱い、Jira固有の節目属性には変換しません。

文字コードはUTF-8 BOM付き、改行はCRLFです。説明の改行・カンマ・引用符はCSV引用で保持します。期限はYYYY-MM-DDです。

## Jira Cloudでの取り込み

Jira管理者の「設定 → システム → 外部システムのインポート → CSV」で、既存の取り込み画面（Switch to the old experience）を使います。取り込み先でEpic／Task／Sub-taskなどの課題タイプと親子階層、サブタスクを有効にしてください。

1. CSVを選び、UTF-8と取り込み先プロジェクトを指定します。
2. Summary、Issue Type、Issue ID、Parentを同名のJira項目へ対応付けます。Issue IDをIssue Keyへ対応付けないでください。
3. 選択した追加列も対応付けます。Due Dateを使う場合は日付形式を`yyyy-MM-dd`にします。
4. 課題タイプ・状態・優先度などの値を、取り込み先に存在する値へ対応付けます。担当者・Componentも解決できることを確認します。
5. Jira側で検証してから取り込み、親子関係を確認します。

Jiraの設定や必須カスタム項目によって追加の列・値が必要になる場合は、保存したCSVに追記してください。Data Centerや別の取り込み画面では項目名・階層の扱いが異なるため、この手順の対象には含めません。

仕様の根拠: [AtlassianのCSV取り込み説明](https://support.atlassian.com/jira-cloud-administration/docs/import-data-from-a-csv-file/)、[親子関係とIssue ID／Parentの対応](https://support.atlassian.com/jira/kb/keep-issue-parent-child-mapping-during-csv-import-to-jira-cloud/)。ファイル構造とGUIからの生成は試験していますが、実際のJiraサーバーへの取り込みは未検証です。

依頼・質問／暫定対応はJira用の課題タイプへ自動変換しません。これらが対象に入る場合は出力を拒否するため、種類をタスク・バグ・課題のいずれかに絞るか、標準CSVを使ってください。
