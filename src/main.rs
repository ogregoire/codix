mod cli;
mod engine;
mod model;
mod plugin;
mod store;

use clap::Parser;
use cli::{Cli, Commands, Format};
use engine::{indexer, project};
use model::{Symbol, SymbolKind, SymbolQuery};
use plugin::PluginRegistry;
use std::env;
use std::path::PathBuf;
use std::process;
use store::sqlite::SqliteStore;
use store::Store;

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Commands::Init => cmd_init(),
        Commands::Index => cmd_index(),
        Commands::Find {
            pattern,
            format,
            case_insensitive,
            kind,
        } => cmd_find(pattern, format, case_insensitive, kind),
        Commands::Refs {
            pattern,
            format,
            case_insensitive,
            kind,
        } => cmd_relational("refs", pattern, format, case_insensitive, kind),
        Commands::Impls {
            pattern,
            format,
            case_insensitive,
            kind,
        } => cmd_relational("impls", pattern, format, case_insensitive, kind),
        Commands::Supers {
            pattern,
            format,
            case_insensitive,
            kind,
        } => cmd_relational("supers", pattern, format, case_insensitive, kind),
        Commands::Callers {
            pattern,
            format,
            case_insensitive,
            kind,
        } => cmd_relational("callers", pattern, format, case_insensitive, kind),
        Commands::Callees {
            pattern,
            format,
            case_insensitive,
            kind,
        } => cmd_relational("callees", pattern, format, case_insensitive, kind),
        Commands::Symbols { file, format, kind } => cmd_symbols(file, format, kind),
        Commands::Package {
            pattern,
            format,
            case_insensitive,
            kind,
        } => cmd_package(pattern, format, case_insensitive, kind),
    }
}

fn cmd_init() -> anyhow::Result<()> {
    let cwd = env::current_dir()?;
    project::init_project(&cwd)?;
    let store = SqliteStore::open(&project::db_path(&cwd).to_string_lossy())?;
    let registry = PluginRegistry::new();
    let count = indexer::full_index(&cwd, &store, &registry)?;
    println!("Initialized codix project in {}", cwd.display());
    println!("Indexed {} files.", count);
    Ok(())
}

fn cmd_index() -> anyhow::Result<()> {
    let cwd = env::current_dir()?;
    let root = project::find_project_root(&cwd)?;
    let store = SqliteStore::open(&project::db_path(&root).to_string_lossy())?;
    let registry = PluginRegistry::new();
    let count = indexer::full_index(&root, &store, &registry)?;
    println!("Indexed {} files.", count);
    Ok(())
}

fn cmd_find(
    pattern: String,
    format: Format,
    case_insensitive: bool,
    kind: Option<String>,
) -> anyhow::Result<()> {
    let (store, root) = open_store_and_reindex()?;
    let cwd = env::current_dir()?;
    let kind = parse_kind(kind)?;
    let query = SymbolQuery {
        pattern,
        case_insensitive,
        kind,
    };
    let results = store.find_symbol(&query)?;
    print_symbols(&results, &format, &root, &cwd);
    Ok(())
}

fn cmd_relational(
    command: &str,
    pattern: String,
    format: Format,
    case_insensitive: bool,
    kind: Option<String>,
) -> anyhow::Result<()> {
    let (store, root) = open_store_and_reindex()?;
    let cwd = env::current_dir()?;
    let parsed_kind = parse_kind(kind.clone())?;
    let query = SymbolQuery {
        pattern: pattern.clone(),
        case_insensitive,
        kind: parsed_kind,
    };
    let matches = store.find_symbol(&query)?;

    if matches.is_empty() {
        anyhow::bail!("No symbol found matching '{}'", pattern);
    }
    if matches.len() > 1 {
        let mut flags = String::new();
        if case_insensitive { flags.push_str(" -i"); }
        if let Some(k) = &kind { flags.push_str(&format!(" -k '{}'", k.replace('\'', "'\\''"))); }
        match format {
            Format::Json => flags.push_str(" -f json"),
            Format::Text => {}
        }
        let mut msg = format!("Multiple symbols match '{}'. Be more specific:\n", pattern);
        for sym in &matches {
            let path = project::display_path(&root, &cwd, &sym.file_path);
            let label = sym.signature.as_deref().unwrap_or(&sym.name);
            let escaped_name = sym.qualified_name.replace('\'', "'\\''");
            msg.push_str(&format!(
                "  {}:{}  {} {} {}\n  → codix {} '{}'{}\n",
                path,
                sym.line,
                sym.visibility.as_str(),
                sym.kind.as_str(),
                label,
                command,
                escaped_name,
                flags
            ));
        }
        anyhow::bail!("{}", msg.trim_end());
    }

    let sym = &matches[0];
    let results = match command {
        "refs" => store.find_references(sym.id)?,
        "impls" => store.find_implementations(sym.id)?,
        "supers" => store.find_supertypes(sym.id)?,
        "callers" => store.find_callers(sym.id)?,
        "callees" => store.find_callees(sym.id)?,
        _ => unreachable!(),
    };
    print_symbols(&results, &format, &root, &cwd);
    Ok(())
}

fn cmd_symbols(file: PathBuf, format: Format, kind: Option<String>) -> anyhow::Result<()> {
    let (store, root) = open_store_and_reindex()?;
    let cwd = env::current_dir()?;
    let kind = parse_kind(kind)?;

    // Convert file path to relative-to-root
    let abs_path = if file.is_absolute() {
        file
    } else {
        cwd.join(&file)
    };
    let rel_path = project::relative_to_root(&root, &abs_path);

    let mut results = store.symbols_in_file(&rel_path)?;
    if let Some(k) = kind {
        results.retain(|s| s.kind == k);
    }
    print_symbols(&results, &format, &root, &cwd);
    Ok(())
}

fn cmd_package(
    pattern: String,
    format: Format,
    case_insensitive: bool,
    kind: Option<String>,
) -> anyhow::Result<()> {
    let (store, root) = open_store_and_reindex()?;
    let cwd = env::current_dir()?;
    let kind = parse_kind(kind)?;
    let query = SymbolQuery {
        pattern: "*".to_string(),
        case_insensitive,
        kind,
    };
    let results = store.symbols_in_package(&pattern, &query)?;
    print_symbols(&results, &format, &root, &cwd);
    Ok(())
}

fn print_symbols(symbols: &[Symbol], format: &Format, root: &std::path::Path, cwd: &std::path::Path) {
    match format {
        Format::Text => {
            if symbols.is_empty() {
                println!("No results found.");
            }
            for sym in symbols {
                let path = project::display_path(root, cwd, &sym.file_path);
                let label = sym.signature.as_deref().unwrap_or(&sym.name);
                println!(
                    "{}:{}  {} {} {}",
                    path,
                    sym.line,
                    sym.visibility.as_str(),
                    sym.kind.as_str(),
                    label
                );
            }
        }
        Format::Json => {
            println!("{}", serde_json::to_string_pretty(symbols).expect("Symbol serialization should not fail"));
        }
    }
}

fn parse_kind(kind: Option<String>) -> anyhow::Result<Option<SymbolKind>> {
    match kind {
        None => Ok(None),
        Some(k) => SymbolKind::parse_kind(&k).map(Some).ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown symbol kind: '{}'. Valid kinds: class, interface, enum, record, annotation, method, field, constructor",
                k
            )
        }),
    }
}

fn open_store_and_reindex() -> anyhow::Result<(SqliteStore, PathBuf)> {
    let cwd = env::current_dir()?;
    let root = project::find_project_root(&cwd)?;
    let store = SqliteStore::open(&project::db_path(&root).to_string_lossy())?;
    let registry = PluginRegistry::new();
    indexer::incremental_reindex(&root, &store, &registry)?;
    Ok((store, root))
}
