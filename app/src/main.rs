mod demo;
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
        Some("--mcp-postgres") => {
            if !(1..=2).contains(&args.len()) || (args.len() == 2 && args[1] != "--read-only") {
                return Err(
                    "Usage: yyproject --mcp-postgres [--read-only] (YYPROJECT_DATABASE_URL)".into(),
                );
            }
            let url = std::env::var("YYPROJECT_DATABASE_URL")
                .map_err(|_| "Set YYPROJECT_DATABASE_URL")?;
            let mut store = Store::connect_postgres(&url, args.len() == 2)?;
            mcp::serve(&mut store, io::stdin().lock(), io::stdout().lock())?;
        }
        Some("--migrate-to-postgres") => {
            if args.len() != 2 {
                return Err(
                    "Usage: yyproject --migrate-to-postgres SOURCE_SQLITE (YYPROJECT_DATABASE_URL)"
                        .into(),
                );
            }
            let mut source = Store::open(Path::new(&args[1]), true)?;
            let dump = source.snapshot()?;
            let url = std::env::var("YYPROJECT_DATABASE_URL")
                .map_err(|_| "Set YYPROJECT_DATABASE_URL")?;
            let mut target = Store::connect_postgres(&url, false)?;
            target.import(&dump)?;
            println!(
                "Migrated {} projects and {} audit records",
                dump.projects.len(),
                dump.audit.len()
            );
        }
        Some("--export-sqlite") => {
            if args.len() != 2 {
                return Err(
                    "Usage: yyproject --export-sqlite NEW_SQLITE (YYPROJECT_DATABASE_URL)".into(),
                );
            }
            let url = std::env::var("YYPROJECT_DATABASE_URL")
                .map_err(|_| "Set YYPROJECT_DATABASE_URL")?;
            let mut source = Store::connect_postgres(&url, true)?;
            source.export_sqlite(Path::new(&args[1]))?;
            println!("Exported PostgreSQL data to SQLite");
        }
        Some("--init-demo") => {
            if args.len() != 2 {
                return Err("Usage: yyproject --init-demo NEW_DATABASE".into());
            }
            let path = &args[1];
            demo::create(Path::new(path))?;
            println!("Created {}", Path::new(path).display());
        }
        Some("--help") | None => print!("{}", include_str!("../../help/help.md")),
        _ => return Err("Unknown option; use --help. The Windows GUI is yyprojectw.".into()),
    }
    Ok(())
}
