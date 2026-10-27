//! Additional fictional cross-team and workaround scenarios, saved through Store.
use chrono::{Duration, Utc};
use std::path::Path;
use yy_project::{work::*, *};
pub fn create(path: &Path) -> std::result::Result<(), Box<dyn std::error::Error>> {
    super::demo::create(path)?;
    let mut s = Store::open(path, false)?;
    let today = Utc::now().date_naive();
    let at = |offset| {
        (today + Duration::days(offset))
            .and_hms_opt(9, 0, 0)
            .unwrap()
            .and_utc()
    };
    let mut p = s.create(
        "他チーム連携・暫定対応（架空デモ）".into(),
        today - Duration::days(30),
        today + Duration::days(60),
        "demo",
    )?;
    for (kind, name, date) in [
        (MasterKind::Assignee, "架空の開発担当", None),
        (MasterKind::Assignee, "架空の運用担当", None),
        (MasterKind::Category, "他チーム連携", None),
        (MasterKind::Holiday, "架空の休業日", Some(today)),
    ] {
        p = s.apply(
            p.id,
            p.revision,
            &Change::SaveMaster {
                master: Master {
                    id: 0,
                    kind,
                    name: name.into(),
                    date,
                    parent: None,
                },
            },
            "management-demo",
        )?;
    }
    for kind in [Kind::Inquiry, Kind::Workaround, Kind::Bug, Kind::Issue] {
        for n in 0..6 {
            let status = match n {
                0 | 1 => Status::Done,
                2 => Status::Cancelled,
                3 => Status::Doing,
                _ => Status::Todo,
            };
            let task = Task {
                kind,
                assignee: Some((n % 2 + 1) as i64),
                category: Some(3),
                title: format!(
                    "{} {}",
                    match kind {
                        Kind::Inquiry => [
                            "API仕様確認",
                            "環境構築依頼",
                            "不要になった質問",
                            "性能回答待ち",
                            "権限設定の依頼",
                            "期限超過の調査依頼"
                        ][n],
                        Kind::Workaround => [
                            "手動再送を終了",
                            "再起動運用を終了",
                            "不要な回避策を中止",
                            "夜間監視を追加",
                            "パッチ待ちの手動照合",
                            "恒久予定を超過"
                        ][n],
                        Kind::Bug => "結合試験の不具合",
                        _ => "運用移行の課題",
                    },
                    n + 1
                ),
                status,
                include_in_effort: false,
                inquiry: (kind == Kind::Inquiry).then(|| {
                    Box::new(Inquiry {
                        kind: if n % 2 == 0 {
                            InquiryKind::Question
                        } else {
                            InquiryKind::Request
                        },
                        target_team: ["基盤チーム", "業務チーム", "品質チーム"][n % 3].into(),
                        contact: format!("窓口{}", n % 3 + 1),
                        requested_at: Some(at(-14 + n as i64)),
                        response_due_at: Some(at(if n == 5 { -2 } else { 4 })),
                        answered_at: (n < 2 || n == 3).then(|| at(-4)),
                        closed_at: matches!(status, Status::Done | Status::Cancelled)
                            .then(|| at(-2)),
                        ..Default::default()
                    })
                }),
                workaround: (kind == Kind::Workaround).then(|| {
                    Box::new(Workaround {
                        reason: "外部基盤の修正が完了するまでの運用継続".into(),
                        permanent_requirements: "基盤修正版の受領、検証環境と回帰試験".into(),
                        action: if n < 4 {
                            "監視を追加し、失敗データを手動で再送\nダブルチェックを実施".into()
                        } else {
                            String::new()
                        },
                        classification: ["手作業", "監視強化", "設定変更"][n % 3].into(),
                        detected_at: Some(at(-14 + n as i64)),
                        applied_at: (n < 4).then(|| at(-7)),
                        permanent_at: (status == Status::Done).then(|| at(-2)),
                        cancelled_at: (status == Status::Cancelled).then(|| at(-2)),
                        permanent_due_on: Some(today + Duration::days(if n == 5 { -1 } else { 7 })),
                        ..Default::default()
                    })
                }),
                case: matches!(kind, Kind::Bug | Kind::Issue).then(|| {
                    Box::new(cases::CaseData {
                        cause: ["仕様漏れ", "実装誤り", "環境差分"][n % 3].into(),
                        occurrence_phase: "設計".into(),
                        detection_phase: "結合試験".into(),
                        detected_at: Some(at(-14 + n as i64)),
                        closed_at: matches!(status, Status::Done | Status::Cancelled)
                            .then(|| at(-2)),
                        ..Default::default()
                    })
                }),
                ..Default::default()
            };
            p = s.apply(
                p.id,
                p.revision,
                &Change::SaveTask { task },
                "management-demo",
            )?;
            if kind == Kind::Inquiry {
                let id = p.tasks.last().unwrap().id;
                for (offset, direction, text) in [
                    (
                        -6,
                        Direction::Outgoing,
                        "確認を依頼しました。\n資料の前提を記録",
                    ),
                    (-5, Direction::Incoming, "調査中、明日までに回答予定"),
                    (-4, Direction::Note, "定例で状況を確認。回答待ちとして管理"),
                ] {
                    p = s.apply(
                        p.id,
                        p.revision,
                        &Change::SaveExchange {
                            id,
                            entry: Exchange {
                                at: Some(at(offset)),
                                direction,
                                person: "架空の担当者".into(),
                                text: text.into(),
                                ..Default::default()
                            },
                        },
                        "management-demo",
                    )?;
                }
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn expanded_scenario_keeps_original_and_management_separate() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("demo.db");
        super::create(&path).unwrap();
        let s = yy_project::Store::open(&path, true).unwrap();
        let p = s.get(4).unwrap();
        assert_eq!(p.tasks.len(), 24);
        assert_eq!(p.tasks.iter().filter(|t| t.inquiry.is_some()).count(), 6);
        assert_eq!(
            p.tasks
                .iter()
                .filter_map(|t| t.inquiry.as_ref())
                .map(|d| d.exchanges.len())
                .sum::<usize>(),
            18
        );
        assert!(super::create(&path).is_err());
    }
}
