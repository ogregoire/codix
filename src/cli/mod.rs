use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "codix", about = "Code symbol indexer for AI agents")]
pub struct Cli {
    /// Show diagnostic info (files reindexed, timing)
    #[arg(short = 'v', long, global = true)]
    pub verbose: bool,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Clone, ValueEnum)]
pub enum Format {
    Text,
    Json,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize codix project and index all source files
    Init,
    /// Full reindex — drop all data and rebuild from scratch
    Index,
    /// Find symbol definitions matching a pattern
    Find {
        /// Glob pattern (e.g. UserService, User*, *.save*)
        pattern: String,
        #[arg(short = 'f', long, default_value = "text")]
        format: Format,
        #[arg(short = 'i', long)]
        case_insensitive: bool,
        #[arg(short = 'k', long)]
        kind: Option<String>,
    },
    /// Find symbols that reference a given symbol (extends, implements, calls, field types, annotations)
    Refs {
        /// Symbol name or qualified name (e.g. Repository, com.foo.Repository)
        pattern: String,
        #[arg(short = 'f', long, default_value = "text")]
        format: Format,
        #[arg(short = 'i', long)]
        case_insensitive: bool,
        #[arg(short = 'k', long)]
        kind: Option<String>,
    },
    /// Find implementations of an interface or subclasses of a class
    Impls {
        /// Interface or class name (e.g. Repository, com.foo.Repository)
        pattern: String,
        #[arg(short = 'f', long, default_value = "text")]
        format: Format,
        #[arg(short = 'i', long)]
        case_insensitive: bool,
        #[arg(short = 'k', long)]
        kind: Option<String>,
    },
    /// Find supertypes of a class or interface (what it extends/implements)
    Supers {
        /// Class or interface name
        pattern: String,
        #[arg(short = 'f', long, default_value = "text")]
        format: Format,
        #[arg(short = 'i', long)]
        case_insensitive: bool,
        #[arg(short = 'k', long)]
        kind: Option<String>,
    },
    /// Find methods that call a given method (resolves receiver types via field/param declarations)
    Callers {
        /// Method name or qualified name (e.g. save*, com.foo.Repository.save*)
        pattern: String,
        #[arg(short = 'f', long, default_value = "text")]
        format: Format,
        #[arg(short = 'i', long)]
        case_insensitive: bool,
        #[arg(short = 'k', long)]
        kind: Option<String>,
    },
    /// Find methods called by a given method (resolves receiver types via field/param declarations)
    Callees {
        /// Method name or qualified name
        pattern: String,
        #[arg(short = 'f', long, default_value = "text")]
        format: Format,
        #[arg(short = 'i', long)]
        case_insensitive: bool,
        #[arg(short = 'k', long)]
        kind: Option<String>,
    },
    /// List symbols defined in a file
    Symbols {
        /// Path to the file (relative to CWD or project root)
        file: PathBuf,
        #[arg(short = 'f', long, default_value = "text")]
        format: Format,
        #[arg(short = 'k', long)]
        kind: Option<String>,
    },
    /// Show index statistics (file count, symbol count, relationships per language)
    Status,
    /// List symbols in a package
    Package {
        /// Package name pattern (e.g. com.foo, com.foo.*)
        pattern: String,
        #[arg(short = 'f', long, default_value = "text")]
        format: Format,
        #[arg(short = 'i', long)]
        case_insensitive: bool,
        #[arg(short = 'k', long)]
        kind: Option<String>,
    },
}
