mod demo;
mod http_server;
mod management_demo;
mod mcp;
mod pat;
mod web;
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
        Some(mode @ ("--generate-pat" | "--generate-password")) => {
            let user = args
                .get(1)
                .and_then(|s| s.to_str())
                .ok_or("Usage: yyproject --generate-pat/--generate-password USER [--auth-config FILE] [--read-only]")?;
            let mut config = std::path::PathBuf::from("yyproject-server.toml");
            let mut read_only = false;
            let mut i = 2;
            while i < args.len() {
                match args[i].to_str() {
                    Some("--auth-config") => {
                        i += 1;
                        config = args.get(i).ok_or("Missing auth config path")?.into();
                    }
                    Some("--read-only") => read_only = true,
                    _ => return Err("Unknown credential option".into()),
                }
                i += 1;
            }
            let web = mode == "--generate-password";
            let token = if web {
                pat::generate_password(&config, user, read_only)
            } else {
                pat::generate(&config, user, read_only)
            }
            .map_err(|e| e.to_string())?;
            println!(
                "{}",
                if web {
                    serde_json::json!({"user":user,"password":token,"config":config,"read_only":read_only})
                } else {
                    serde_json::json!({"user":user,"pat":token,"config":config,"read_only":read_only})
                }
            );
        }
        Some(mode @ ("--mcp-http" | "--mcp-http-postgres" | "--web" | "--web-postgres")) => {
            let postgres = mode.ends_with("-postgres");
            let mut i = if postgres { 1 } else { 2 };
            let path = if postgres {
                None
            } else {
                Some(args.get(1).ok_or("Usage: yyproject --mcp-http DATABASE [--listen IP:PORT] [--auth-config FILE] [--read-only]")?)
            };
            let mut listen = "127.0.0.1:8740".parse()?;
            let mut config = std::path::PathBuf::from("yyproject-server.toml");
            let mut read_only = false;
            while i < args.len() {
                match args[i].to_str() {
                    Some("--listen") => {
                        i += 1;
                        listen = args
                            .get(i)
                            .and_then(|s| s.to_str())
                            .ok_or("Missing listen IP:PORT")?
                            .parse()?;
                    }
                    Some("--auth-config") => {
                        i += 1;
                        config = args.get(i).ok_or("Missing auth config path")?.into();
                    }
                    Some("--read-only") => read_only = true,
                    _ => return Err("Unknown HTTP server option".into()),
                }
                i += 1;
            }
            let store = if let Some(path) = path {
                Store::open(Path::new(path), read_only)?
            } else {
                Store::connect_postgres(
                    &std::env::var("YYPROJECT_DATABASE_URL")
                        .map_err(|_| "Set YYPROJECT_DATABASE_URL")?,
                    read_only,
                )?
            };
            http_server::serve(store, listen, config, mode.starts_with("--web"), read_only)?;
        }
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
        Some("--init-management-demo") => {
            if args.len() != 2 {
                return Err("Usage: yyproject --init-management-demo NEW_DATABASE".into());
            }
            management_demo::create(Path::new(&args[1]))?;
            println!("Created management demo");
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
