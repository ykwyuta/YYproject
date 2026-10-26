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
