mod cli;
mod engine;
mod model;
mod plugin;
mod store;

use clap::Parser;
use cli::{Cli, Commands, Format};
use engine::{indexer, project};
use engine::indexer::ReindexStats;
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
    let verbose = cli.verbose;
    match cli.command {
        Commands::Init => cmd_init(verbose),
        Commands::Index => cmd_index(verbose),
        Commands::Status => cmd_status(verbose),
        Commands::Find {
            pattern,
            format,
            case_insensitive,
            kind,
        } => cmd_find(pattern, format, case_insensitive, kind, verbose),
        Commands::Refs {
            pattern,
            format,
            case_insensitive,
            kind,
        } => cmd_relational("refs", pattern, format, case_insensitive, kind, verbose),
        Commands::Impls {
            pattern,
            format,
            case_insensitive,
            kind,
        } => cmd_relational("impls", pattern, format, case_insensitive, kind, verbose),
        Commands::Supers {
            pattern,
            format,
            case_insensitive,
            kind,
        } => cmd_relational("supers", pattern, format, case_insensitive, kind, verbose),
        Commands::Callers {
            pattern,
            format,
            case_insensitive,
            kind,
        } => cmd_relational("callers", pattern, format, case_insensitive, kind, verbose),
        Commands::Callees {
            pattern,
            format,
            case_insensitive,
            kind,
        } => cmd_relational("callees", pattern, format, case_insensitive, kind, verbose),
        Commands::Symbols { file, format, kind } => cmd_symbols(file, format, kind, verbose),
        Commands::Package {
            pattern,
            format,
            case_insensitive,
            kind,
        } => cmd_package(pattern, format, case_insensitive, kind, verbose),
    }
}

fn cmd_init(verbose: bool) -> anyhow::Result<()> {
    let cwd = env::current_dir()?;
    project::init_project(&cwd)?;
    let store = SqliteStore::open(&project::db_path(&cwd).to_string_lossy())?;
    let registry = PluginRegistry::new();
    let start = std::time::Instant::now();
    let counts = indexer::full_index(&cwd, &store, &registry)?;
    println!("Initialized codix project in {}", cwd.display());
    print_index_counts(&counts);
    if verbose {
        eprintln!("[verbose] full index in {}ms", start.elapsed().as_millis());
    }
    Ok(())
}

fn cmd_index(verbose: bool) -> anyhow::Result<()> {
    let cwd = env::current_dir()?;
    let root = project::find_project_root(&cwd)?;
    let store = SqliteStore::open(&project::db_path(&root).to_string_lossy())?;
    let registry = PluginRegistry::new();
    let start = std::time::Instant::now();
    let counts = indexer::full_index(&root, &store, &registry)?;
    print_index_counts(&counts);
    if verbose {
        eprintln!("[verbose] full index in {}ms", start.elapsed().as_millis());
    }
    Ok(())
}

fn cmd_status(verbose: bool) -> anyhow::Result<()> {
    let (store, _root) = open_store_and_reindex(verbose)?;
    let registry = PluginRegistry::new();
    let stats = store.index_stats()?;

    if stats.is_empty() {
        println!("No files indexed.");
        return Ok(());
    }

    for (lang, ls) in &stats {
        let display = registry.display_name_for(lang);
        println!("{} {} {}", ls.files, display, if ls.files == 1 { "file" } else { "files" });
    }
    Ok(())
}

fn print_index_counts(counts: &std::collections::BTreeMap<String, u64>) {
    for (lang, count) in counts {
        println!("Indexed {} {} {}.", count, lang, if *count == 1 { "file" } else { "files" });
    }
    if counts.is_empty() {
        println!("No files to index.");
    }
}

fn cmd_find(
    pattern: String,
    format: Format,
    case_insensitive: bool,
    kind: Option<String>,
    verbose: bool,
) -> anyhow::Result<()> {
    let (store, root) = open_store_and_reindex(verbose)?;
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
    verbose: bool,
) -> anyhow::Result<()> {
    let (store, root) = open_store_and_reindex(verbose)?;
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

fn cmd_symbols(file: PathBuf, format: Format, kind: Option<String>, verbose: bool) -> anyhow::Result<()> {
    let (store, root) = open_store_and_reindex(verbose)?;
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
    verbose: bool,
) -> anyhow::Result<()> {
    let (store, root) = open_store_and_reindex(verbose)?;
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

fn open_store_and_reindex(verbose: bool) -> anyhow::Result<(SqliteStore, PathBuf)> {
    let cwd = env::current_dir()?;
    let root = project::find_project_root(&cwd)?;
    let store = SqliteStore::open(&project::db_path(&root).to_string_lossy())?;
    let registry = PluginRegistry::new();
    let stats = indexer::incremental_reindex(&root, &store, &registry)?;
    if verbose {
        print_reindex_stats(&stats);
    }
    Ok((store, root))
}

fn print_reindex_stats(stats: &ReindexStats) {
    let changed = !stats.added.is_empty() || !stats.modified.is_empty() || !stats.deleted.is_empty();
    if !changed {
        eprintln!("[verbose] index up-to-date ({} files, {}ms)", stats.unchanged, stats.elapsed_ms);
        return;
    }
    for f in &stats.added {
        eprintln!("[verbose] added: {}", f);
    }
    for f in &stats.modified {
        eprintln!("[verbose] modified: {}", f);
    }
    for f in &stats.deleted {
        eprintln!("[verbose] deleted: {}", f);
    }
    eprintln!(
        "[verbose] reindexed in {}ms ({} added, {} modified, {} deleted, {} unchanged)",
        stats.elapsed_ms, stats.added.len(), stats.modified.len(), stats.deleted.len(), stats.unchanged
    );
}
