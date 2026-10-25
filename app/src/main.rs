mod mcp;
use std::{io, path::Path};
use yy_project::Store;
fn main() {
    if let Err(error) = run() {
        eprintln!("yyproject: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.first().and_then(|s| s.to_str()) {
        Some("--version") => println!("yyproject {}", env!("CARGO_PKG_VERSION")),
        Some("--mcp") => {
            if !(2..=3).contains(&args.len()) || (args.len() == 3 && args[2] != "--read-only") {
                return Err("Usage: yyproject --mcp DATABASE [--read-only]".into());
            }
            let mut store = Store::open(Path::new(&args[1]), args.len() == 3)?;
            mcp::serve(&mut store, io::stdin().lock(), io::stdout().lock())?;
        }
        Some("--init-demo") => {
            let path = args
                .get(1)
                .ok_or("Usage: yyproject --init-demo NEW_DATABASE")?;
            if Path::new(path).exists() {
                return Err("Demo destination already exists".into());
            }
            let mut store = Store::open(Path::new(path), false)?;
            let start = chrono::Local::now().date_naive();
            let mut p = store.create(
                "サンプル開発プロジェクト".into(),
                start,
                start + chrono::Days::new(30),
                "demo",
            )?;
            for (title, status, progress) in [
                ("要件の整理", yy_project::Status::Done, 100),
                ("設計", yy_project::Status::Doing, 50),
                ("実装", yy_project::Status::Todo, 0),
                ("検証環境の調整", yy_project::Status::Blocked, 0),
            ] {
                p = store.apply(
                    p.id,
                    p.revision,
                    &yy_project::Change::SaveTask {
                        task: yy_project::Task {
                            title: title.into(),
                            status,
                            progress,
                            duration_days: 3,
                            estimate: 24.0,
                            ..Default::default()
                        },
                    },
                    "demo",
                )?;
            }
            for (predecessor, successor) in [(1, 2), (2, 3), (2, 4)] {
                p = store.apply(
                    p.id,
                    p.revision,
                    &yy_project::Change::AddDependency {
                        dependency: yy_project::Dependency {
                            predecessor,
                            successor,
                            lag_days: 0,
                        },
                    },
                    "demo",
                )?;
            }
            store.apply(p.id, p.revision, &yy_project::Change::Schedule, "demo")?;
            println!("Created {}", Path::new(path).display());
        }
        Some("--help") | None => print!("{}", include_str!("../../help/help.md")),
        _ => return Err("Unknown option; use --help. The Windows GUI is yyprojectw.".into()),
    }
    Ok(())
}
