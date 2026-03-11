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
    // — Indexing —
    /// Initialize codix project and index all source files
    #[command(display_order = 1)]
    Init {
        /// Set config values before indexing (repeatable, e.g. -c index.languages java)
        #[arg(short = 'c', long = "config", value_names = ["KEY", "VALUE"], num_args = 2)]
        configs: Vec<String>,
    },
    /// Full reindex — drop all data and rebuild from scratch
    #[command(display_order = 2)]
    Index,

    // — Configuration —
    /// Show index statistics (file count per language)
    #[command(display_order = 10)]
    Status,
    /// Get or set configuration values
    #[command(display_order = 11)]
    Config {
        /// Config key in section.key format (e.g. index.languages)
        key: Option<String>,
        /// Value to set. If omitted, prints the current value
        value: Option<String>,
        /// Remove the key from the config file
        #[arg(short = 'r', long, conflicts_with = "all")]
        remove: bool,
        /// Show all configuration values
        #[arg(short = 'a', long, conflicts_with = "remove")]
        all: bool,
    },

    // — Symbol lookup —
    /// Find symbol definitions matching a pattern
    #[command(display_order = 20)]
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
    /// List symbols defined in a file
    #[command(display_order = 21)]
    Symbols {
        /// Path to the file (relative to CWD or project root)
        file: PathBuf,
        #[arg(short = 'f', long, default_value = "text")]
        format: Format,
        #[arg(short = 'k', long)]
        kind: Option<String>,
    },
    /// List symbols in a package
    #[command(display_order = 22)]
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

    // — Relationships —
    /// Find symbols that reference a given symbol (extends, implements, calls, field types, annotations)
    #[command(display_order = 30)]
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
    #[command(display_order = 31)]
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
    #[command(display_order = 32)]
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
    #[command(display_order = 33)]
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
    #[command(display_order = 34)]
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
}
