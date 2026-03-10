use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "codix", about = "Code symbol indexer for AI agents")]
pub struct Cli {
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
    /// Create a new codix project in the current directory
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
    /// Find all references to a symbol
    Refs {
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
        pattern: String,
        #[arg(short = 'f', long, default_value = "text")]
        format: Format,
        #[arg(short = 'i', long)]
        case_insensitive: bool,
        #[arg(short = 'k', long)]
        kind: Option<String>,
    },
    /// Find supertypes (extends/implements)
    Supers {
        pattern: String,
        #[arg(short = 'f', long, default_value = "text")]
        format: Format,
        #[arg(short = 'i', long)]
        case_insensitive: bool,
        #[arg(short = 'k', long)]
        kind: Option<String>,
    },
    /// Find callers of a method
    Callers {
        pattern: String,
        #[arg(short = 'f', long, default_value = "text")]
        format: Format,
        #[arg(short = 'i', long)]
        case_insensitive: bool,
        #[arg(short = 'k', long)]
        kind: Option<String>,
    },
    /// Find methods called by a method
    Callees {
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
