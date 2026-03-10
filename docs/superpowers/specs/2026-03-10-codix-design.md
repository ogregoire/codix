# Codix Design Spec

A Rust CLI tool that indexes code symbols and relationships for fast querying. Primary consumer: AI code agents (e.g. Claude Code). Avoids token-burning exploration by providing precise, indexed lookups.

## Architecture

**Approach**: Monolithic index-then-query. Single binary, single SQLite database, compile-time language plugins.

- **Parsing**: Tree-sitter (incremental, multi-language grammars).
- **Storage**: SQLite behind an abstract `Store` trait (swappable backends later).
- **Plugins**: Compile-time `LanguagePlugin` trait. Each language is a Rust module implementing the trait. Starting with Java. Ecosystem tools (e.g. Maven) are separate plugins, not part of the language plugin.

## Data Model

### Files table
| Column   | Type    | Notes                          |
|----------|---------|--------------------------------|
| id       | PK      | Auto-increment                 |
| path     | TEXT    | Relative to codix root, unique |
| mtime    | INTEGER | Last modified timestamp        |
| hash     | TEXT    | Optional, for extra safety     |
| language | TEXT    | Which plugin indexed it        |

### Symbols table
| Column           | Type    | Notes                                         |
|------------------|---------|-----------------------------------------------|
| id               | PK      | Auto-increment                                |
| name             | TEXT    | Simple name (e.g. `save`)                     |
| signature        | TEXT    | Nullable. For methods/constructors: `save(Person)` |
| qualified_name   | TEXT    | Full name including signature when applicable |
| kind             | TEXT    | class, interface, enum, record, annotation, method, field, constructor |
| visibility       | TEXT    | public, protected, package-private, private   |
| file_id          | FK      | References Files.id                           |
| line             | INTEGER | Start line                                    |
| column           | INTEGER | Start column                                  |
| end_line         | INTEGER | End line                                      |
| end_column       | INTEGER | End column                                    |
| parent_symbol_id | FK      | Nullable. Links method/field to enclosing type |
| package          | TEXT    | e.g. `com.foo`                                |

### Relationships table
| Column           | Type | Notes                                                        |
|------------------|------|--------------------------------------------------------------|
| source_symbol_id | FK   | References Symbols.id                                        |
| target_symbol_id | FK   | References Symbols.id                                        |
| file_id          | FK   | References Files.id (file where this relationship was found) |
| kind             | TEXT | extends, implements, calls, field_type, parameter_type, return_type, throws, overrides |

Primary key: `(source_symbol_id, target_symbol_id, kind)`. The `file_id` column enables efficient cleanup when a file is re-indexed.

### References

The `refs` command is a composite query: it returns all symbols linked to the target via any relationship kind in the Relationships table. For example, `codix refs UserService` returns all symbols that extend, implement, call, or reference `UserService` in any way. This is not a separate data source — it aggregates across relationship kinds.

### Cross-file Relationship Resolution

Relationships are resolved in a two-pass approach:

1. **Pass 1 — Extract**: Each file is parsed and symbols are extracted. Relationships are emitted with the target identified by qualified name (string), not by ID, since the target may be in another file.
2. **Pass 2 — Resolve**: After all files are indexed, unresolved qualified name references are matched against the symbols table to obtain `target_symbol_id`. Relationships that cannot be resolved (e.g. referencing a third-party library symbol not in the index) are stored with `target_symbol_id = NULL` and a `target_qualified_name` column for future resolution.

This means the Relationships table also has:

| Column               | Type | Notes                                              |
|----------------------|------|----------------------------------------------------|
| target_qualified_name | TEXT | Unresolved target name, used when target_symbol_id is NULL |

## Plugin Trait

```rust
trait LanguagePlugin {
    /// Plugin identifier, e.g. "java"
    fn name(&self) -> &str;

    /// File extensions this plugin handles, e.g. ["java"]
    fn file_extensions(&self) -> &[&str];

    /// Valid symbol kinds for this language
    fn symbol_kinds(&self) -> &[&str];

    /// Return the tree-sitter language grammar
    fn tree_sitter_language(&self) -> tree_sitter::Language;

    /// Extract symbols and relationships from a parsed tree
    fn extract_symbols(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &Path,
    ) -> ExtractionResult;
}

struct ExtractionResult {
    symbols: Vec<ExtractedSymbol>,
    relationships: Vec<ExtractedRelationship>,
}

struct ExtractedSymbol {
    local_id: usize,               // Temporary ID for intra-file references
    name: String,
    signature: Option<String>,
    qualified_name: String,
    kind: String,
    visibility: String,
    line: u32,
    column: u32,
    end_line: u32,
    end_column: u32,
    parent_local_id: Option<usize>, // References another ExtractedSymbol in same file
    package: String,
}

struct ExtractedRelationship {
    source_local_id: usize,         // References ExtractedSymbol.local_id
    target_qualified_name: String,   // Resolved to symbol ID after all files are indexed
    kind: String,                    // extends, implements, calls, etc.
}
```

## Store Trait

```rust
trait Store {
    // File operations
    fn upsert_file(&mut self, file: &FileRecord) -> Result<FileId>;
    fn get_file(&self, path: &Path) -> Result<Option<FileRecord>>;
    fn list_files(&self) -> Result<Vec<FileRecord>>;
    fn delete_file(&mut self, file_id: FileId) -> Result<()>;

    // Symbol operations
    fn insert_symbols(&mut self, file_id: FileId, symbols: &[ExtractedSymbol]) -> Result<()>;
    fn delete_symbols_for_file(&mut self, file_id: FileId) -> Result<()>;

    // Relationship operations
    fn insert_relationships(&mut self, relationships: &[ExtractedRelationship]) -> Result<()>;
    fn delete_relationships_for_file(&mut self, file_id: FileId) -> Result<()>;

    // Query operations
    fn find_symbol(&self, query: &SymbolQuery) -> Result<Vec<Symbol>>;
    fn find_references(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>>;
    fn find_implementations(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>>;
    fn find_supertypes(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>>;
    fn find_callers(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>>;
    fn find_callees(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>>;
    fn symbols_in_file(&self, file_id: FileId) -> Result<Vec<Symbol>>;
    fn symbols_in_package(&self, package: &str) -> Result<Vec<Symbol>>;

    // Lifecycle
    fn begin_transaction(&mut self) -> Result<()>;
    fn commit_transaction(&mut self) -> Result<()>;
    fn rollback_transaction(&mut self) -> Result<()>;
}
```

## Indexing Engine

1. **Discovery**: Walk the project directory from the codix root. Ask each `LanguagePlugin` if it handles each file (by extension).
2. **Staleness check** (on query commands): Compare file mtime against the `files` table.
   - New file: parse and insert.
   - Modified file: delete old data, insert new (within a transaction).
   - Deleted file: remove symbols, relationships, and file record.
3. **Full reindex** (`codix index`): Drop all data and rebuild from scratch.
4. **Auto-reindex**: Every query command runs the staleness check before executing.

## CLI

### Commands

```
codix init                          # Create .codix/ in current directory
codix index                         # Full reindex (drop + rebuild)
codix find <pattern>                # Find symbol definitions
codix refs <pattern>                # Find references to a symbol
codix impls <pattern>               # Find implementations
codix supers <pattern>              # Find supertypes
codix callers <pattern>             # Who calls this method
codix callees <pattern>             # What does this method call
codix symbols <file>                # List symbols in a file
codix package <pattern>             # List symbols in a package
```

### Flags

| Short | Long                 | Default        | Description                          |
|-------|----------------------|----------------|--------------------------------------|
| `-f`  | `--format`           | `text`         | Output format: `text` or `json`      |
| `-i`  | `--case-insensitive` | case-sensitive | Case-insensitive pattern matching    |
| `-k`  | `--kind`             | all            | Filter by symbol kind (language-dependent) |

### Patterns

Glob-based by default. Patterns match against both simple `name` and `qualified_name` — a match on either counts. This means `save` finds all methods named `save` without needing `*.save*`. Examples:
- `UserService` — matches by simple name
- `com.foo.UserService` — matches by qualified name
- `User*` — glob on simple name
- `com.foo.*Service` — glob on qualified name

### Output

**Text** (default): compact, one result per line, paths relative to CWD.
```
src/main/java/com/foo/UserService.java:42  public class UserService
src/main/java/com/foo/UserService.java:58  public void save(Person)
```

**JSON**: structured output with `--format json` / `-f json`.

### Self-discovery

Every command/subcommand, when called without required arguments, prints a concise help message: what it does, expected arguments, available flags, and an example. `codix` alone lists all subcommands. This allows AI agents to discover the tool by exploration.

## Project Root Detection

- `codix init` creates `.codix/` in the current directory. This is the project root.
- All other commands walk up from CWD looking for `.codix/`. Error if not found.
- Paths stored internally relative to codix root.
- Paths displayed relative to user's CWD.

## Project Structure

```
codix/
├── Cargo.toml
└── src/
    ├── main.rs              # CLI entry point (clap)
    ├── cli/
    │   └── mod.rs           # Command parsing, output formatting
    ├── engine/
    │   ├── mod.rs           # Indexing engine (discovery, staleness, reindex)
    │   └── project.rs       # Project root detection, path resolution
    ├── store/
    │   ├── mod.rs           # Store trait
    │   └── sqlite.rs        # SQLite implementation
    └── plugin/
        ├── mod.rs           # LanguagePlugin trait, plugin registry
        └── java/
            └── mod.rs       # Java plugin (tree-sitter extraction)
```

## Key Dependencies

- `clap` — CLI argument parsing
- `tree-sitter` + `tree-sitter-java` — parsing
- `rusqlite` — SQLite bindings
- `glob` — pattern matching
