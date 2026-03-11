# codix Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust CLI that indexes code symbols and relationships for fast querying by AI agents.

**Architecture:** Monolithic index-then-query. Tree-sitter parses source files, symbols and relationships stored in SQLite behind an abstract Store trait, compile-time language plugins via a LanguagePlugin trait. Starting with Java.

**Tech Stack:** Rust, clap (CLI), tree-sitter + tree-sitter-java (parsing), rusqlite (storage), glob (pattern matching), serde/serde_json (JSON output), walkdir (directory traversal), anyhow (errors).

---

## Chunk 1: Project Scaffold and Core Types

### Task 1: Initialize Rust project and dependencies

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`

- [ ] **Step 1: Create the Rust project**

Run: `cargo init --name codix`

- [ ] **Step 2: Add dependencies to Cargo.toml**

```toml
[package]
name = "codix"
version = "0.1.0"
edition = "2021"

[dependencies]
clap = { version = "4", features = ["derive"] }
tree-sitter = "0.24"
tree-sitter-java = "0.23"
rusqlite = { version = "0.31", features = ["bundled"] }
glob-match = "0.2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
walkdir = "2"
anyhow = "1"
pathdiff = "0.2"

[dev-dependencies]
tempfile = "3"
```

**Note:** Verify tree-sitter and tree-sitter-java version compatibility before proceeding. If tree-sitter-java 0.23 is incompatible with tree-sitter 0.24, use compatible versions (e.g. both at 0.23 or both at 0.24). Check crates.io for the latest compatible pairing.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs
git commit -m "feat: initialize Rust project with dependencies"
```

### Task 2: Define core data model types

**Files:**
- Create: `src/model.rs`
- Modify: `src/main.rs` (add module declaration)

- [ ] **Step 1: Write tests for model types**

In `src/model.rs`, add tests that verify type construction and Display impls:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_kind_display() {
        assert_eq!(SymbolKind::Class.as_str(), "class");
        assert_eq!(SymbolKind::Method.as_str(), "method");
    }

    #[test]
    fn test_relationship_kind_display() {
        assert_eq!(RelationshipKind::Extends.as_str(), "extends");
        assert_eq!(RelationshipKind::Implements.as_str(), "implements");
    }

    #[test]
    fn test_visibility_display() {
        assert_eq!(Visibility::Public.as_str(), "public");
        assert_eq!(Visibility::PackagePrivate.as_str(), "package-private");
    }

    #[test]
    fn test_symbol_display_text_class() {
        let sym = Symbol {
            id: 1,
            name: "UserService".into(),
            signature: None,
            qualified_name: "com.foo.UserService".into(),
            kind: SymbolKind::Class,
            visibility: Visibility::Public,
            file_id: 1,
            file_path: "src/main/java/com/foo/UserService.java".into(),
            line: 42,
            column: 0,
            end_line: 100,
            end_column: 1,
            parent_symbol_id: None,
            package: "com.foo".into(),
        };
        assert_eq!(
            sym.display_text(),
            "src/main/java/com/foo/UserService.java:42  public class UserService"
        );
    }

    #[test]
    fn test_symbol_display_text_method_with_signature() {
        let sym = Symbol {
            id: 2,
            name: "save".into(),
            signature: Some("save(Person)".into()),
            qualified_name: "com.foo.UserService.save(Person)".into(),
            kind: SymbolKind::Method,
            visibility: Visibility::Public,
            file_id: 1,
            file_path: "src/main/java/com/foo/UserService.java".into(),
            line: 58,
            column: 4,
            end_line: 65,
            end_column: 5,
            parent_symbol_id: Some(1),
            package: "com.foo".into(),
        };
        assert_eq!(
            sym.display_text(),
            "src/main/java/com/foo/UserService.java:58  public method save(Person)"
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test`
Expected: compilation errors (types don't exist yet)

- [ ] **Step 3: Implement the model types**

```rust
use serde::Serialize;

pub type FileId = i64;
pub type SymbolId = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SymbolKind {
    Class,
    Interface,
    Enum,
    Record,
    Annotation,
    Method,
    Field,
    Constructor,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Enum => "enum",
            Self::Record => "record",
            Self::Annotation => "annotation",
            Self::Method => "method",
            Self::Field => "field",
            Self::Constructor => "constructor",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "class" => Some(Self::Class),
            "interface" => Some(Self::Interface),
            "enum" => Some(Self::Enum),
            "record" => Some(Self::Record),
            "annotation" => Some(Self::Annotation),
            "method" => Some(Self::Method),
            "field" => Some(Self::Field),
            "constructor" => Some(Self::Constructor),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Visibility {
    Public,
    Protected,
    PackagePrivate,
    Private,
}

impl Visibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Protected => "protected",
            Self::PackagePrivate => "package-private",
            Self::Private => "private",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "public" => Some(Self::Public),
            "protected" => Some(Self::Protected),
            "package-private" => Some(Self::PackagePrivate),
            "private" => Some(Self::Private),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelationshipKind {
    Extends,
    Implements,
    Calls,
    FieldType,
    ParameterType,
    ReturnType,
    Throws,
    Overrides,
}

impl RelationshipKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Extends => "extends",
            Self::Implements => "implements",
            Self::Calls => "calls",
            Self::FieldType => "field_type",
            Self::ParameterType => "parameter_type",
            Self::ReturnType => "return_type",
            Self::Throws => "throws",
            Self::Overrides => "overrides",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "extends" => Some(Self::Extends),
            "implements" => Some(Self::Implements),
            "calls" => Some(Self::Calls),
            "field_type" => Some(Self::FieldType),
            "parameter_type" => Some(Self::ParameterType),
            "return_type" => Some(Self::ReturnType),
            "throws" => Some(Self::Throws),
            "overrides" => Some(Self::Overrides),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FileRecord {
    pub id: FileId,
    pub path: String,
    pub mtime: i64,
    pub hash: Option<String>,
    pub language: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub signature: Option<String>,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub visibility: Visibility,
    pub file_id: FileId,
    pub file_path: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub parent_symbol_id: Option<SymbolId>,
    pub package: String,
}

impl Symbol {
    pub fn display_text(&self) -> String {
        let label = self.signature.as_deref().unwrap_or(&self.name);
        format!(
            "{}:{}  {} {} {}",
            self.file_path,
            self.line,
            self.visibility.as_str(),
            self.kind.as_str(),
            label,
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Relationship {
    pub source_symbol_id: SymbolId,
    pub target_symbol_id: Option<SymbolId>,
    pub target_qualified_name: String,
    pub file_id: FileId,
    pub kind: RelationshipKind,
}

/// Produced by a LanguagePlugin when extracting from a single file.
#[derive(Debug, Clone)]
pub struct ExtractedSymbol {
    pub local_id: usize,
    pub name: String,
    pub signature: Option<String>,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub visibility: Visibility,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub parent_local_id: Option<usize>,
    pub package: String,
}

#[derive(Debug, Clone)]
pub struct ExtractedRelationship {
    pub source_local_id: usize,
    pub target_qualified_name: String,
    pub kind: RelationshipKind,
}

#[derive(Debug, Clone)]
pub struct ExtractionResult {
    pub symbols: Vec<ExtractedSymbol>,
    pub relationships: Vec<ExtractedRelationship>,
}

/// Query parameters for finding symbols.
#[derive(Debug, Clone, Default)]
pub struct SymbolQuery {
    pub pattern: String,
    pub case_insensitive: bool,
    pub kind: Option<SymbolKind>,
}
```

- [ ] **Step 4: Add module to main.rs**

```rust
mod model;

fn main() {
    println!("codix");
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add src/model.rs src/main.rs
git commit -m "feat: add core data model types"
```

### Task 3: Define Store trait

**Files:**
- Create: `src/store/mod.rs`
- Modify: `src/main.rs` (add module declaration)

- [ ] **Step 1: Write the Store trait**

```rust
use std::path::Path;
use anyhow::Result;
use crate::model::*;

pub mod sqlite;

pub trait Store {
    // File operations
    fn upsert_file(&self, path: &str, mtime: i64, hash: Option<&str>, language: &str) -> Result<FileId>;
    fn get_file(&self, path: &str) -> Result<Option<FileRecord>>;
    fn list_files(&self) -> Result<Vec<FileRecord>>;
    fn delete_file(&self, file_id: FileId) -> Result<()>;

    // Symbol operations
    fn insert_symbols(&self, file_id: FileId, symbols: &[ExtractedSymbol]) -> Result<Vec<SymbolId>>;
    fn delete_symbols_for_file(&self, file_id: FileId) -> Result<()>;

    // Relationship operations
    fn insert_relationships(&self, file_id: FileId, symbol_id_map: &[(usize, SymbolId)], relationships: &[ExtractedRelationship]) -> Result<()>;
    fn delete_relationships_for_file(&self, file_id: FileId) -> Result<()>;
    fn resolve_relationships(&self) -> Result<u64>;

    // Query operations
    fn find_symbol(&self, query: &SymbolQuery) -> Result<Vec<Symbol>>;
    fn find_references(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>>;
    fn find_implementations(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>>;
    fn find_supertypes(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>>;
    fn find_callers(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>>;
    fn find_callees(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>>;
    fn symbols_in_file(&self, file_path: &str) -> Result<Vec<Symbol>>;
    fn symbols_in_package(&self, package: &str, query: &SymbolQuery) -> Result<Vec<Symbol>>;

    // Lifecycle
    fn begin_transaction(&self) -> Result<()>;
    fn commit_transaction(&self) -> Result<()>;
    fn rollback_transaction(&self) -> Result<()>;
    fn clear_all(&self) -> Result<()>;
}
```

- [ ] **Step 2: Add module to main.rs**

Add `mod store;` to `src/main.rs`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles (sqlite module can be empty initially — just `// TODO`)

- [ ] **Step 4: Commit**

```bash
git add src/store/mod.rs src/main.rs
git commit -m "feat: define Store trait"
```

### Task 4: Implement SQLite Store — schema and file operations

**Files:**
- Create: `src/store/sqlite.rs`

- [ ] **Step 1: Write tests for schema creation and file operations**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> SqliteStore {
        SqliteStore::open(":memory:").unwrap()
    }

    #[test]
    fn test_upsert_and_get_file() {
        let store = test_store();
        let id = store.upsert_file("src/Foo.java", 1000, None, "java").unwrap();
        let file = store.get_file("src/Foo.java").unwrap().unwrap();
        assert_eq!(file.id, id);
        assert_eq!(file.path, "src/Foo.java");
        assert_eq!(file.mtime, 1000);
        assert_eq!(file.language, "java");
    }

    #[test]
    fn test_upsert_file_updates_mtime() {
        let store = test_store();
        let id1 = store.upsert_file("src/Foo.java", 1000, None, "java").unwrap();
        let id2 = store.upsert_file("src/Foo.java", 2000, None, "java").unwrap();
        assert_eq!(id1, id2);
        let file = store.get_file("src/Foo.java").unwrap().unwrap();
        assert_eq!(file.mtime, 2000);
    }

    #[test]
    fn test_list_files() {
        let store = test_store();
        store.upsert_file("a.java", 1, None, "java").unwrap();
        store.upsert_file("b.java", 2, None, "java").unwrap();
        let files = store.list_files().unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_delete_file() {
        let store = test_store();
        let id = store.upsert_file("a.java", 1, None, "java").unwrap();
        store.delete_file(id).unwrap();
        assert!(store.get_file("a.java").unwrap().is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test`
Expected: compilation errors

- [ ] **Step 3: Implement SqliteStore with schema and file operations**

```rust
use rusqlite::{Connection, params};
use anyhow::Result;
use crate::model::*;
use super::Store;

pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let store = Self { conn };
        store.create_schema()?;
        Ok(store)
    }

    fn create_schema(&self) -> Result<()> {
        self.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                mtime INTEGER NOT NULL,
                hash TEXT,
                language TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS symbols (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                signature TEXT,
                qualified_name TEXT NOT NULL,
                kind TEXT NOT NULL,
                visibility TEXT NOT NULL,
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                line INTEGER NOT NULL,
                column_ INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                end_column INTEGER NOT NULL,
                parent_symbol_id INTEGER REFERENCES symbols(id) ON DELETE SET NULL,
                package TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS relationships (
                source_symbol_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
                target_symbol_id INTEGER REFERENCES symbols(id) ON DELETE CASCADE,
                target_qualified_name TEXT NOT NULL,
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                kind TEXT NOT NULL,
                PRIMARY KEY (source_symbol_id, target_qualified_name, kind)
            );
            CREATE INDEX IF NOT EXISTS idx_symbols_file_id ON symbols(file_id);
            CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_qualified_name ON symbols(qualified_name);
            CREATE INDEX IF NOT EXISTS idx_symbols_package ON symbols(package);
            CREATE INDEX IF NOT EXISTS idx_relationships_file_id ON relationships(file_id);
            CREATE INDEX IF NOT EXISTS idx_relationships_target ON relationships(target_symbol_id);
            CREATE INDEX IF NOT EXISTS idx_relationships_source ON relationships(source_symbol_id);
        ")?;
        Ok(())
    }
}

impl Store for SqliteStore {
    fn upsert_file(&self, path: &str, mtime: i64, hash: Option<&str>, language: &str) -> Result<FileId> {
        self.conn.execute(
            "INSERT INTO files (path, mtime, hash, language) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET mtime=?2, hash=?3, language=?4",
            params![path, mtime, hash, language],
        )?;
        let id = self.conn.query_row(
            "SELECT id FROM files WHERE path = ?1", params![path], |row| row.get(0),
        )?;
        Ok(id)
    }

    fn get_file(&self, path: &str) -> Result<Option<FileRecord>> {
        let mut stmt = self.conn.prepare("SELECT id, path, mtime, hash, language FROM files WHERE path = ?1")?;
        let mut rows = stmt.query_map(params![path], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                mtime: row.get(2)?,
                hash: row.get(3)?,
                language: row.get(4)?,
            })
        })?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    fn list_files(&self) -> Result<Vec<FileRecord>> {
        let mut stmt = self.conn.prepare("SELECT id, path, mtime, hash, language FROM files")?;
        let rows = stmt.query_map([], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                mtime: row.get(2)?,
                hash: row.get(3)?,
                language: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn delete_file(&self, file_id: FileId) -> Result<()> {
        self.conn.execute("DELETE FROM files WHERE id = ?1", params![file_id])?;
        Ok(())
    }

    // Remaining methods stubbed — implemented in subsequent tasks
    fn insert_symbols(&self, _file_id: FileId, _symbols: &[ExtractedSymbol]) -> Result<Vec<SymbolId>> { todo!() }
    fn delete_symbols_for_file(&self, _file_id: FileId) -> Result<()> { todo!() }
    fn insert_relationships(&self, _file_id: FileId, _map: &[(usize, SymbolId)], _rels: &[ExtractedRelationship]) -> Result<()> { todo!() }
    fn delete_relationships_for_file(&self, _file_id: FileId) -> Result<()> { todo!() }
    fn resolve_relationships(&self) -> Result<u64> { todo!() }
    fn find_symbol(&self, _query: &SymbolQuery) -> Result<Vec<Symbol>> { todo!() }
    fn find_references(&self, _symbol_id: SymbolId) -> Result<Vec<Symbol>> { todo!() }
    fn find_implementations(&self, _symbol_id: SymbolId) -> Result<Vec<Symbol>> { todo!() }
    fn find_supertypes(&self, _symbol_id: SymbolId) -> Result<Vec<Symbol>> { todo!() }
    fn find_callers(&self, _symbol_id: SymbolId) -> Result<Vec<Symbol>> { todo!() }
    fn find_callees(&self, _symbol_id: SymbolId) -> Result<Vec<Symbol>> { todo!() }
    fn symbols_in_file(&self, _file_path: &str) -> Result<Vec<Symbol>> { todo!() }
    fn symbols_in_package(&self, _package: &str, _query: &SymbolQuery) -> Result<Vec<Symbol>> { todo!() }
    fn begin_transaction(&self) -> Result<()> { todo!() }
    fn commit_transaction(&self) -> Result<()> { todo!() }
    fn rollback_transaction(&self) -> Result<()> { todo!() }
    fn clear_all(&self) -> Result<()> { todo!() }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add src/store/sqlite.rs
git commit -m "feat: implement SQLite store schema and file operations"
```

### Task 5: SQLite Store — symbol and relationship CRUD

**Files:**
- Modify: `src/store/sqlite.rs`

- [ ] **Step 1: Write tests for symbol insert/delete and relationship insert/delete**

```rust
#[test]
fn test_insert_and_delete_symbols() {
    let store = test_store();
    let fid = store.upsert_file("Foo.java", 1, None, "java").unwrap();
    let syms = vec![ExtractedSymbol {
        local_id: 0,
        name: "Foo".into(),
        signature: None,
        qualified_name: "com.foo.Foo".into(),
        kind: SymbolKind::Class,
        visibility: Visibility::Public,
        line: 1, column: 0, end_line: 10, end_column: 1,
        parent_local_id: None,
        package: "com.foo".into(),
    }];
    let ids = store.insert_symbols(fid, &syms).unwrap();
    assert_eq!(ids.len(), 1);

    store.delete_symbols_for_file(fid).unwrap();
    // Verify deletion by trying to re-insert (no constraint violations = clean)
    let ids2 = store.insert_symbols(fid, &syms).unwrap();
    assert_eq!(ids2.len(), 1);
}

#[test]
fn test_insert_symbols_with_parent() {
    let store = test_store();
    let fid = store.upsert_file("Foo.java", 1, None, "java").unwrap();
    let syms = vec![
        ExtractedSymbol {
            local_id: 0, name: "Foo".into(), signature: None,
            qualified_name: "com.foo.Foo".into(), kind: SymbolKind::Class,
            visibility: Visibility::Public,
            line: 1, column: 0, end_line: 10, end_column: 1,
            parent_local_id: None, package: "com.foo".into(),
        },
        ExtractedSymbol {
            local_id: 1, name: "bar".into(), signature: Some("bar(String)".into()),
            qualified_name: "com.foo.Foo.bar(String)".into(), kind: SymbolKind::Method,
            visibility: Visibility::Public,
            line: 3, column: 4, end_line: 5, end_column: 5,
            parent_local_id: Some(0), package: "com.foo".into(),
        },
    ];
    let ids = store.insert_symbols(fid, &syms).unwrap();
    assert_eq!(ids.len(), 2);
}

#[test]
fn test_insert_and_delete_relationships() {
    let store = test_store();
    let fid = store.upsert_file("Foo.java", 1, None, "java").unwrap();
    let syms = vec![ExtractedSymbol {
        local_id: 0, name: "Foo".into(), signature: None,
        qualified_name: "com.foo.Foo".into(), kind: SymbolKind::Class,
        visibility: Visibility::Public,
        line: 1, column: 0, end_line: 10, end_column: 1,
        parent_local_id: None, package: "com.foo".into(),
    }];
    let ids = store.insert_symbols(fid, &syms).unwrap();
    let map: Vec<(usize, SymbolId)> = vec![(0, ids[0])];
    let rels = vec![ExtractedRelationship {
        source_local_id: 0,
        target_qualified_name: "com.foo.Bar".into(),
        kind: RelationshipKind::Extends,
    }];
    store.insert_relationships(fid, &map, &rels).unwrap();
    store.delete_relationships_for_file(fid).unwrap();
}

#[test]
fn test_transaction_and_clear() {
    let store = test_store();
    store.begin_transaction().unwrap();
    store.upsert_file("a.java", 1, None, "java").unwrap();
    store.commit_transaction().unwrap();
    assert_eq!(store.list_files().unwrap().len(), 1);
    store.clear_all().unwrap();
    assert_eq!(store.list_files().unwrap().len(), 0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test`
Expected: panics from `todo!()`

- [ ] **Step 3: Implement the remaining CRUD methods**

Replace the `todo!()` stubs for: `insert_symbols`, `delete_symbols_for_file`, `insert_relationships`, `delete_relationships_for_file`, `resolve_relationships`, `begin_transaction`, `commit_transaction`, `rollback_transaction`, `clear_all`.

Key implementation notes:
- `insert_symbols`: Insert in local_id order. First pass inserts symbols without parent_symbol_id, collect the mapping `local_id → real SymbolId`. Second pass updates `parent_symbol_id` for symbols that have `parent_local_id`.
- `insert_relationships`: Use the `symbol_id_map` to translate `source_local_id` → `source_symbol_id`. Store `target_qualified_name` as-is; `target_symbol_id` stays NULL until `resolve_relationships` runs.
- `resolve_relationships`: `UPDATE relationships SET target_symbol_id = (SELECT id FROM symbols WHERE qualified_name = relationships.target_qualified_name) WHERE target_symbol_id IS NULL`.
- `clear_all`: `DELETE FROM relationships; DELETE FROM symbols; DELETE FROM files;`

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add src/store/sqlite.rs
git commit -m "feat: implement SQLite store symbol and relationship CRUD"
```

### Task 6: SQLite Store — query operations

**Files:**
- Modify: `src/store/sqlite.rs`

- [ ] **Step 1: Write tests for query operations**

```rust
fn seed_store() -> SqliteStore {
    let store = test_store();
    let f1 = store.upsert_file("UserService.java", 1, None, "java").unwrap();
    let f2 = store.upsert_file("PersonRepo.java", 1, None, "java").unwrap();

    let syms1 = vec![
        ExtractedSymbol {
            local_id: 0, name: "UserService".into(), signature: None,
            qualified_name: "com.foo.UserService".into(), kind: SymbolKind::Class,
            visibility: Visibility::Public,
            line: 1, column: 0, end_line: 50, end_column: 1,
            parent_local_id: None, package: "com.foo".into(),
        },
        ExtractedSymbol {
            local_id: 1, name: "save".into(), signature: Some("save(Person)".into()),
            qualified_name: "com.foo.UserService.save(Person)".into(), kind: SymbolKind::Method,
            visibility: Visibility::Public,
            line: 10, column: 4, end_line: 20, end_column: 5,
            parent_local_id: Some(0), package: "com.foo".into(),
        },
    ];
    let ids1 = store.insert_symbols(f1, &syms1).unwrap();
    let map1: Vec<(usize, SymbolId)> = syms1.iter().enumerate().map(|(i, s)| (s.local_id, ids1[i])).collect();

    let syms2 = vec![
        ExtractedSymbol {
            local_id: 0, name: "PersonRepo".into(), signature: None,
            qualified_name: "com.foo.PersonRepo".into(), kind: SymbolKind::Interface,
            visibility: Visibility::Public,
            line: 1, column: 0, end_line: 30, end_column: 1,
            parent_local_id: None, package: "com.foo".into(),
        },
        ExtractedSymbol {
            local_id: 1, name: "findAll".into(), signature: Some("findAll()".into()),
            qualified_name: "com.foo.PersonRepo.findAll()".into(), kind: SymbolKind::Method,
            visibility: Visibility::Public,
            line: 5, column: 4, end_line: 5, end_column: 40,
            parent_local_id: Some(0), package: "com.foo".into(),
        },
    ];
    let ids2 = store.insert_symbols(f2, &syms2).unwrap();
    let map2: Vec<(usize, SymbolId)> = syms2.iter().enumerate().map(|(i, s)| (s.local_id, ids2[i])).collect();

    // UserService implements PersonRepo
    let rels1 = vec![ExtractedRelationship {
        source_local_id: 0,
        target_qualified_name: "com.foo.PersonRepo".into(),
        kind: RelationshipKind::Implements,
    }];
    store.insert_relationships(f1, &map1, &rels1).unwrap();

    // UserService.save calls PersonRepo.findAll
    let rels_call = vec![ExtractedRelationship {
        source_local_id: 1,
        target_qualified_name: "com.foo.PersonRepo.findAll()".into(),
        kind: RelationshipKind::Calls,
    }];
    store.insert_relationships(f1, &map1, &rels_call).unwrap();

    store.resolve_relationships().unwrap();
    store
}

#[test]
fn test_find_symbol_exact() {
    let store = seed_store();
    let results = store.find_symbol(&SymbolQuery { pattern: "UserService".into(), ..Default::default() }).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "UserService");
}

#[test]
fn test_find_symbol_glob() {
    let store = seed_store();
    let results = store.find_symbol(&SymbolQuery { pattern: "*Service".into(), ..Default::default() }).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn test_find_symbol_by_kind() {
    let store = seed_store();
    let results = store.find_symbol(&SymbolQuery {
        pattern: "*".into(), kind: Some(SymbolKind::Interface), ..Default::default()
    }).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "PersonRepo");
}

#[test]
fn test_find_implementations() {
    let store = seed_store();
    // Find what implements PersonRepo
    let repo = store.find_symbol(&SymbolQuery { pattern: "PersonRepo".into(), ..Default::default() }).unwrap();
    let impls = store.find_implementations(repo[0].id).unwrap();
    assert_eq!(impls.len(), 1);
    assert_eq!(impls[0].name, "UserService");
}

#[test]
fn test_find_supertypes() {
    let store = seed_store();
    let svc = store.find_symbol(&SymbolQuery { pattern: "UserService".into(), ..Default::default() }).unwrap();
    let supers = store.find_supertypes(svc[0].id).unwrap();
    assert_eq!(supers.len(), 1);
    assert_eq!(supers[0].name, "PersonRepo");
}

#[test]
fn test_find_callers() {
    let store = seed_store();
    let find_all = store.find_symbol(&SymbolQuery { pattern: "com.foo.PersonRepo.findAll()".into(), ..Default::default() }).unwrap();
    let callers = store.find_callers(find_all[0].id).unwrap();
    assert_eq!(callers.len(), 1);
    assert_eq!(callers[0].name, "save");
}

#[test]
fn test_find_callees() {
    let store = seed_store();
    let save = store.find_symbol(&SymbolQuery { pattern: "com.foo.UserService.save(Person)".into(), ..Default::default() }).unwrap();
    let callees = store.find_callees(save[0].id).unwrap();
    assert_eq!(callees.len(), 1);
    assert_eq!(callees[0].name, "findAll");
}

#[test]
fn test_find_references() {
    let store = seed_store();
    let repo = store.find_symbol(&SymbolQuery { pattern: "PersonRepo".into(), ..Default::default() }).unwrap();
    let refs = store.find_references(repo[0].id).unwrap();
    // UserService implements PersonRepo → should show up
    assert!(!refs.is_empty());
}

#[test]
fn test_symbols_in_file() {
    let store = seed_store();
    let syms = store.symbols_in_file("UserService.java").unwrap();
    assert_eq!(syms.len(), 2); // class + method
}

#[test]
fn test_symbols_in_package() {
    let store = seed_store();
    let syms = store.symbols_in_package("com.foo", &SymbolQuery { pattern: "*".into(), ..Default::default() }).unwrap();
    assert_eq!(syms.len(), 4); // 2 classes + 2 methods
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test`
Expected: panics from `todo!()`

- [ ] **Step 3: Implement query methods**

Key implementation notes:
- `find_symbol`: Convert glob pattern to SQL LIKE pattern (`*` → `%`, `?` → `_`) for an initial SQL filter on both `name` and `qualified_name`. Then apply `glob_match::glob_match` in Rust on the results for precise matching (SQL LIKE can over-match). If `case_insensitive`, use `LIKE` with `COLLATE NOCASE`. If `kind` is set, add a SQL WHERE clause for it. This avoids loading all symbols into memory.
- `find_references`: `SELECT symbols.* FROM relationships JOIN symbols ON symbols.id = relationships.source_symbol_id WHERE relationships.target_symbol_id = ?`
- `find_implementations`: Same as `find_references` but filtered to `kind IN ('implements', 'extends')`
- `find_supertypes`: `SELECT symbols.* FROM relationships JOIN symbols ON symbols.id = relationships.target_symbol_id WHERE relationships.source_symbol_id = ? AND kind IN ('extends', 'implements')`
- `find_callers`: Like `find_references` but `kind = 'calls'`
- `find_callees`: Like `find_supertypes` but `kind = 'calls'`
- `symbols_in_file`: Join symbols with files on `file_id` where `files.path = ?`
- `symbols_in_package`: `WHERE package = ?` with optional glob and kind filtering

All query methods must join with `files` table to populate `file_path` on the returned `Symbol`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add src/store/sqlite.rs
git commit -m "feat: implement SQLite store query operations"
```

---

## Chunk 2: Language Plugin System and Java Plugin

### Task 7: Define LanguagePlugin trait and registry

**Files:**
- Create: `src/plugin/mod.rs`
- Modify: `src/main.rs` (add module declaration)

- [ ] **Step 1: Write the LanguagePlugin trait and registry**

```rust
use std::path::Path;
use crate::model::*;

pub mod java;

pub trait LanguagePlugin {
    fn name(&self) -> &str;
    fn file_extensions(&self) -> &[&str];
    fn symbol_kinds(&self) -> &[&str];
    fn tree_sitter_language(&self) -> tree_sitter::Language;
    fn extract_symbols(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &Path,
    ) -> ExtractionResult;
}

pub struct PluginRegistry {
    plugins: Vec<Box<dyn LanguagePlugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        let mut registry = Self { plugins: Vec::new() };
        registry.register(Box::new(java::JavaPlugin));
        registry
    }

    pub fn register(&mut self, plugin: Box<dyn LanguagePlugin>) {
        self.plugins.push(plugin);
    }

    pub fn plugin_for_extension(&self, ext: &str) -> Option<&dyn LanguagePlugin> {
        self.plugins.iter()
            .find(|p| p.file_extensions().contains(&ext))
            .map(|p| p.as_ref())
    }

    pub fn all_extensions(&self) -> Vec<&str> {
        self.plugins.iter().flat_map(|p| p.file_extensions()).copied().collect()
    }
}
```

- [ ] **Step 2: Add module to main.rs and verify compilation**

Run: `cargo build`
Expected: compiles (java module can be a stub initially)

- [ ] **Step 3: Commit**

```bash
git add src/plugin/mod.rs src/main.rs
git commit -m "feat: define LanguagePlugin trait and plugin registry"
```

### Task 8: Java plugin — type-level symbol extraction

**Files:**
- Create: `src/plugin/java/mod.rs`

- [ ] **Step 1: Create test Java files in a temp directory for testing**

Write tests that parse small Java snippets and verify extracted symbols:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn parse_java(source: &str) -> ExtractionResult {
        let plugin = JavaPlugin;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&plugin.tree_sitter_language()).unwrap();
        let tree = parser.parse(source.as_bytes(), None).unwrap();
        plugin.extract_symbols(&tree, source.as_bytes(), &PathBuf::from("Test.java"))
    }

    #[test]
    fn test_extract_class() {
        let result = parse_java("package com.foo;\n\npublic class UserService {}");
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "UserService");
        assert_eq!(result.symbols[0].kind, SymbolKind::Class);
        assert_eq!(result.symbols[0].visibility, Visibility::Public);
        assert_eq!(result.symbols[0].package, "com.foo");
        assert_eq!(result.symbols[0].qualified_name, "com.foo.UserService");
    }

    #[test]
    fn test_extract_interface() {
        let result = parse_java("package com.foo;\n\npublic interface Repository {}");
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].kind, SymbolKind::Interface);
    }

    #[test]
    fn test_extract_enum() {
        let result = parse_java("package com.foo;\n\npublic enum Status { ACTIVE, INACTIVE }");
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].kind, SymbolKind::Enum);
    }

    #[test]
    fn test_extract_multiple_classes_in_file() {
        let source = "package com.foo;\n\npublic class Main {}\nclass Helper {}";
        let result = parse_java(source);
        assert_eq!(result.symbols.len(), 2);
        assert_eq!(result.symbols[0].name, "Main");
        assert_eq!(result.symbols[0].visibility, Visibility::Public);
        assert_eq!(result.symbols[1].name, "Helper");
        assert_eq!(result.symbols[1].visibility, Visibility::PackagePrivate);
    }

    #[test]
    fn test_no_package() {
        let result = parse_java("public class NoPackage {}");
        assert_eq!(result.symbols[0].package, "");
        assert_eq!(result.symbols[0].qualified_name, "NoPackage");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test`
Expected: compilation errors

- [ ] **Step 3: Implement JavaPlugin type-level extraction**

Implement `JavaPlugin` struct with the `LanguagePlugin` trait. Use tree-sitter to walk the CST:
- Find `package_declaration` node to extract package name.
- Find `class_declaration`, `interface_declaration`, `enum_declaration`, `record_declaration`, `annotation_type_declaration` nodes.
- For each, extract: name (from `identifier` child), visibility (look for `modifiers` child containing `public`, `protected`, `private` — absence = package-private), line/column positions.
- Build qualified_name as `{package}.{name}` (or just `{name}` if no package).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add src/plugin/java/mod.rs
git commit -m "feat: Java plugin type-level symbol extraction"
```

### Task 9: Java plugin — member extraction (methods, fields, constructors)

**Files:**
- Modify: `src/plugin/java/mod.rs`

- [ ] **Step 1: Write tests for member extraction**

```rust
#[test]
fn test_extract_methods() {
    let source = "package com.foo;\npublic class Svc {\n  public void save(Person p) {}\n  private int count() { return 0; }\n}";
    let result = parse_java(source);
    // class + 2 methods
    assert_eq!(result.symbols.len(), 3);
    let save = &result.symbols[1];
    assert_eq!(save.name, "save");
    assert_eq!(save.signature, Some("save(Person)".to_string()));
    assert_eq!(save.kind, SymbolKind::Method);
    assert_eq!(save.parent_local_id, Some(0));
    let count = &result.symbols[2];
    assert_eq!(count.signature, Some("count()".to_string()));
    assert_eq!(count.visibility, Visibility::Private);
}

#[test]
fn test_extract_constructor() {
    let source = "package com.foo;\npublic class Svc {\n  public Svc(String name) {}\n}";
    let result = parse_java(source);
    assert_eq!(result.symbols.len(), 2);
    let ctor = &result.symbols[1];
    assert_eq!(ctor.name, "Svc");
    assert_eq!(ctor.kind, SymbolKind::Constructor);
    assert_eq!(ctor.signature, Some("Svc(String)".to_string()));
}

#[test]
fn test_extract_fields() {
    let source = "package com.foo;\npublic class Svc {\n  private String name;\n  protected int age;\n}";
    let result = parse_java(source);
    assert_eq!(result.symbols.len(), 3); // class + 2 fields
    let name_field = &result.symbols[1];
    assert_eq!(name_field.name, "name");
    assert_eq!(name_field.kind, SymbolKind::Field);
    assert_eq!(name_field.visibility, Visibility::Private);
}

#[test]
fn test_method_overloads() {
    let source = "package com.foo;\npublic class Svc {\n  void save(Person p) {}\n  void save(String s, int i) {}\n}";
    let result = parse_java(source);
    let methods: Vec<_> = result.symbols.iter().filter(|s| s.kind == SymbolKind::Method).collect();
    assert_eq!(methods.len(), 2);
    assert_eq!(methods[0].signature, Some("save(Person)".to_string()));
    assert_eq!(methods[1].signature, Some("save(String,int)".to_string()));
    assert_ne!(methods[0].qualified_name, methods[1].qualified_name);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test`
Expected: failures (methods/fields not extracted yet)

- [ ] **Step 3: Implement member extraction**

Extend the tree-sitter walker:
- Inside each type declaration, find `method_declaration`, `constructor_declaration`, `field_declaration` nodes.
- For methods: extract name, build signature from `formal_parameters` (extract type names only, comma-separated), set `parent_local_id` to the enclosing type's `local_id`.
- For constructors: same as methods but kind = Constructor.
- For fields: extract name from `variable_declarator`, no signature.
- Qualified name for members: `{type_qualified_name}.{signature_or_name}`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add src/plugin/java/mod.rs
git commit -m "feat: Java plugin member extraction (methods, fields, constructors)"
```

### Task 10: Java plugin — relationship extraction

**Files:**
- Modify: `src/plugin/java/mod.rs`

- [ ] **Step 1: Write tests for relationship extraction**

```rust
#[test]
fn test_extract_extends() {
    let source = "package com.foo;\npublic class UserService extends BaseService {}";
    let result = parse_java(source);
    assert_eq!(result.relationships.len(), 1);
    assert_eq!(result.relationships[0].kind, RelationshipKind::Extends);
    assert_eq!(result.relationships[0].target_qualified_name, "BaseService");
    assert_eq!(result.relationships[0].source_local_id, 0);
}

#[test]
fn test_extract_implements() {
    let source = "package com.foo;\npublic class UserService implements Repository, Serializable {}";
    let result = parse_java(source);
    let impls: Vec<_> = result.relationships.iter().filter(|r| r.kind == RelationshipKind::Implements).collect();
    assert_eq!(impls.len(), 2);
}

#[test]
fn test_extract_method_calls() {
    let source = "package com.foo;\npublic class Svc {\n  void doWork() {\n    repo.save(entity);\n    helper.process();\n  }\n}";
    let result = parse_java(source);
    let calls: Vec<_> = result.relationships.iter().filter(|r| r.kind == RelationshipKind::Calls).collect();
    assert!(calls.len() >= 2);
}

#[test]
fn test_extract_field_type() {
    let source = "package com.foo;\npublic class Svc {\n  private Repository repo;\n}";
    let result = parse_java(source);
    let field_types: Vec<_> = result.relationships.iter().filter(|r| r.kind == RelationshipKind::FieldType).collect();
    assert_eq!(field_types.len(), 1);
    assert_eq!(field_types[0].target_qualified_name, "Repository");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test`
Expected: failures (relationships not extracted yet)

- [ ] **Step 3: Implement relationship extraction**

Extend the tree-sitter walker:
- `extends`: In class/interface declarations, look for `superclass` / `super_interfaces` nodes. Extract the type name as `target_qualified_name`. For now, use the simple name (fully-qualified resolution happens in the resolve pass).
- `implements`: Same, from `interfaces` node.
- `calls`: In method bodies, find `method_invocation` nodes. Extract the method name as target. Note: full resolution (receiver type + method name) is complex — for v1, store the method name only. This means callers/callees queries may have false positives for common names, which is acceptable.
- `field_type`: For field declarations, extract the type name.
- `return_type`, `parameter_type`, `throws`: Extract from method declarations.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add src/plugin/java/mod.rs
git commit -m "feat: Java plugin relationship extraction"
```

---

## Chunk 3: Engine (Project Detection, Indexing, Staleness)

### Task 11: Project root detection and path utilities

**Files:**
- Create: `src/engine/mod.rs`
- Create: `src/engine/project.rs`
- Modify: `src/main.rs` (add module declaration)

- [ ] **Step 1: Write tests for project root detection**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_find_root_in_current_dir() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join(".codix")).unwrap();
        let root = find_project_root(tmp.path()).unwrap();
        assert_eq!(root, tmp.path());
    }

    #[test]
    fn test_find_root_in_parent() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join(".codix")).unwrap();
        let sub = tmp.path().join("src/main/java");
        fs::create_dir_all(&sub).unwrap();
        let root = find_project_root(&sub).unwrap();
        assert_eq!(root, tmp.path());
    }

    #[test]
    fn test_no_root_found() {
        let tmp = TempDir::new().unwrap();
        let result = find_project_root(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_relative_path_from_root() {
        let root = PathBuf::from("/project");
        let file = PathBuf::from("/project/src/Foo.java");
        assert_eq!(relative_to_root(&root, &file), "src/Foo.java");
    }

    #[test]
    fn test_display_path_relative_to_cwd() {
        let root = PathBuf::from("/project");
        let cwd = PathBuf::from("/project/src");
        let stored = "src/main/Foo.java";
        let display = display_path(&root, &cwd, stored);
        assert_eq!(display, "main/Foo.java");
    }
}
```

Note: Add `tempfile = "3"` to `[dev-dependencies]` in Cargo.toml.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test`
Expected: compilation errors

- [ ] **Step 3: Implement project root detection and path utilities**

```rust
use std::path::{Path, PathBuf};
use anyhow::{Result, bail};

pub fn find_project_root(start: &Path) -> Result<PathBuf> {
    let mut current = start.canonicalize()?;
    loop {
        if current.join(".codix").is_dir() {
            return Ok(current);
        }
        if !current.pop() {
            bail!("No codix project found. Run 'codix init' in your project root.");
        }
    }
}

pub fn init_project(dir: &Path) -> Result<PathBuf> {
    let codix_dir = dir.join(".codix");
    if codix_dir.exists() {
        bail!("codix project already initialized in this directory.");
    }
    std::fs::create_dir(&codix_dir)?;
    Ok(codix_dir)
}

pub fn db_path(root: &Path) -> PathBuf {
    root.join(".codix").join("index.db")
}

pub fn relative_to_root(root: &Path, file: &Path) -> String {
    file.strip_prefix(root).unwrap().to_string_lossy().into_owned()
}

pub fn display_path(root: &Path, cwd: &Path, stored_path: &str) -> String {
    let abs = root.join(stored_path);
    pathdiff::diff_paths(&abs, cwd)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| stored_path.to_string())
}
```

Note: Add `pathdiff = "0.2"` to `[dependencies]` in Cargo.toml.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/engine/mod.rs src/engine/project.rs src/main.rs
git commit -m "feat: project root detection and path utilities"
```

### Task 12: Indexing engine — file discovery and full index

**Files:**
- Create: `src/engine/indexer.rs`
- Modify: `src/engine/mod.rs` (add module)

- [ ] **Step 1: Write tests for file discovery**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    fn setup_project() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir(root.join(".codix")).unwrap();
        fs::create_dir_all(root.join("src/main/java/com/foo")).unwrap();
        fs::write(root.join("src/main/java/com/foo/Foo.java"), "package com.foo;\npublic class Foo {}").unwrap();
        fs::write(root.join("src/main/java/com/foo/Bar.java"), "package com.foo;\npublic class Bar {}").unwrap();
        fs::write(root.join("src/main/java/com/foo/readme.txt"), "not java").unwrap();
        (tmp, root)
    }

    #[test]
    fn test_discover_files() {
        let (_tmp, root) = setup_project();
        let registry = PluginRegistry::new();
        let files = discover_files(&root, &registry);
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|(_, ext)| ext == "java"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Implement file discovery and full index**

```rust
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use anyhow::Result;
use crate::plugin::PluginRegistry;
use crate::store::Store;
use crate::engine::project;

/// Discover all files the registry can handle.
/// Returns (absolute_path, extension) pairs.
pub fn discover_files(root: &Path, registry: &PluginRegistry) -> Vec<(PathBuf, String)> {
    let extensions = registry.all_extensions();
    let mut result = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.starts_with(root.join(".codix")) { continue; }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if extensions.contains(&ext) {
                result.push((path.to_path_buf(), ext.to_string()));
            }
        }
    }
    result
}

/// Full reindex: clear everything, re-parse all files.
pub fn full_index(root: &Path, store: &dyn Store, registry: &PluginRegistry) -> Result<()> {
    store.clear_all()?;
    let files = discover_files(root, registry);
    store.begin_transaction()?;
    for (path, ext) in &files {
        let plugin = registry.plugin_for_extension(ext).unwrap();
        index_file(root, path, plugin, store)?;
    }
    store.resolve_relationships()?;
    store.commit_transaction()?;
    Ok(())
}

fn index_file(root: &Path, path: &Path, plugin: &dyn crate::plugin::LanguagePlugin, store: &dyn Store) -> Result<()> {
    let source = std::fs::read(path)?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&plugin.tree_sitter_language())?;
    let tree = parser.parse(&source, None).ok_or_else(|| anyhow::anyhow!("Failed to parse {}", path.display()))?;
    let result = plugin.extract_symbols(&tree, &source, path);
    let rel_path = project::relative_to_root(root, path);
    let mtime = std::fs::metadata(path)?.modified()?.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let file_id = store.upsert_file(&rel_path, mtime, None, plugin.name())?;
    let symbol_ids = store.insert_symbols(file_id, &result.symbols)?;
    let map: Vec<(usize, _)> = result.symbols.iter().map(|s| s.local_id).zip(symbol_ids.iter().copied()).collect();
    store.insert_relationships(file_id, &map, &result.relationships)?;
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add src/engine/indexer.rs src/engine/mod.rs
git commit -m "feat: file discovery and full index"
```

### Task 13: Staleness check and incremental reindex

**Files:**
- Modify: `src/engine/indexer.rs`

- [ ] **Step 1: Write tests for staleness detection**

```rust
#[test]
fn test_incremental_reindex_detects_new_file() {
    let (_tmp, root) = setup_project();
    let store = SqliteStore::open(":memory:").unwrap();
    let registry = PluginRegistry::new();
    full_index(&root, &store, &registry).unwrap();
    assert_eq!(store.list_files().unwrap().len(), 2);

    // Add a new file
    fs::write(root.join("src/main/java/com/foo/Baz.java"), "package com.foo;\npublic class Baz {}").unwrap();
    incremental_reindex(&root, &store, &registry).unwrap();
    assert_eq!(store.list_files().unwrap().len(), 3);
}

#[test]
fn test_incremental_reindex_detects_deleted_file() {
    let (_tmp, root) = setup_project();
    let store = SqliteStore::open(":memory:").unwrap();
    let registry = PluginRegistry::new();
    full_index(&root, &store, &registry).unwrap();
    assert_eq!(store.list_files().unwrap().len(), 2);

    fs::remove_file(root.join("src/main/java/com/foo/Bar.java")).unwrap();
    incremental_reindex(&root, &store, &registry).unwrap();
    assert_eq!(store.list_files().unwrap().len(), 1);
}

#[test]
fn test_incremental_reindex_detects_modified_file() {
    let (_tmp, root) = setup_project();
    let store = SqliteStore::open(":memory:").unwrap();
    let registry = PluginRegistry::new();
    full_index(&root, &store, &registry).unwrap();

    // Modify file (need to change mtime — sleep 1.5s for HFS+ 1s granularity)
    std::thread::sleep(std::time::Duration::from_millis(1500));
    fs::write(root.join("src/main/java/com/foo/Foo.java"), "package com.foo;\npublic class Foo {\n  void newMethod() {}\n}").unwrap();
    incremental_reindex(&root, &store, &registry).unwrap();

    let syms = store.symbols_in_file("src/main/java/com/foo/Foo.java").unwrap();
    assert_eq!(syms.len(), 2); // class + new method
}
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Implement incremental reindex**

```rust
use std::collections::HashSet;

pub fn incremental_reindex(root: &Path, store: &dyn Store, registry: &PluginRegistry) -> Result<()> {
    let disk_files = discover_files(root, registry);
    let indexed_files = store.list_files()?;

    let disk_paths: HashSet<String> = disk_files.iter()
        .map(|(p, _)| project::relative_to_root(root, p))
        .collect();
    let indexed_paths: HashSet<String> = indexed_files.iter()
        .map(|f| f.path.clone())
        .collect();

    store.begin_transaction()?;

    // Delete removed files
    for indexed in &indexed_files {
        if !disk_paths.contains(&indexed.path) {
            store.delete_relationships_for_file(indexed.id)?;
            store.delete_symbols_for_file(indexed.id)?;
            store.delete_file(indexed.id)?;
        }
    }

    // Add new or modified files
    for (path, ext) in &disk_files {
        let rel_path = project::relative_to_root(root, path);
        let mtime = std::fs::metadata(path)?.modified()?.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;

        let needs_reindex = match store.get_file(&rel_path)? {
            None => true,
            Some(f) => f.mtime < mtime,
        };

        if needs_reindex {
            // Clean old data if file existed
            if let Some(f) = store.get_file(&rel_path)? {
                store.delete_relationships_for_file(f.id)?;
                store.delete_symbols_for_file(f.id)?;
            }
            let plugin = registry.plugin_for_extension(ext).unwrap();
            index_file(root, path, plugin, store)?;
        }
    }

    store.resolve_relationships()?;
    store.commit_transaction()?;
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add src/engine/indexer.rs
git commit -m "feat: incremental reindex with staleness detection"
```

---

## Chunk 4: CLI Commands and Output

### Task 14: CLI scaffolding with clap — init and index commands

**Files:**
- Create: `src/cli/mod.rs`
- Modify: `src/main.rs` (wire up CLI)

- [ ] **Step 1: Implement CLI argument parsing and init/index commands**

```rust
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
    /// Full reindex (drop and rebuild)
    Index,
    /// Find symbol definitions matching a pattern
    Find {
        /// Glob pattern to match (e.g. UserService, User*, *.save*)
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
    /// Find implementations of an interface or subclasses
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
        /// Path to the file
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
```

- [ ] **Step 2: Wire up main.rs**

```rust
mod cli;
mod engine;
mod model;
mod plugin;
mod store;

use clap::Parser;
use cli::{Cli, Commands};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init => cmd_init(),
        Commands::Index => cmd_index(),
        // ... remaining commands
    }
}
```

- [ ] **Step 3: Implement init and index command handlers**

`cmd_init()`: Call `engine::project::init_project`, print success.
`cmd_index()`: Find project root, open SQLite store, run `engine::indexer::full_index`, print summary.

- [ ] **Step 4: Build and test manually**

Run: `cargo build && ./target/debug/codix --help`
Expected: shows help with all subcommands

Run: `./target/debug/codix init` (in a temp directory)
Expected: creates `.codix/` directory

- [ ] **Step 5: Commit**

```bash
git add src/cli/mod.rs src/main.rs
git commit -m "feat: CLI scaffolding with init and index commands"
```

### Task 15: Query command handlers and output formatting

**Files:**
- Modify: `src/cli/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement output formatting**

Add to `src/cli/mod.rs`:

```rust
use crate::model::Symbol;
use crate::engine::project;
use std::path::Path;

pub fn print_symbols(symbols: &[Symbol], format: &Format, root: &Path, cwd: &Path) {
    match format {
        Format::Text => {
            for sym in symbols {
                let path = project::display_path(root, cwd, &sym.file_path);
                let label = sym.signature.as_deref().unwrap_or(&sym.name);
                println!("{}:{}  {} {} {}", path, sym.line, sym.visibility.as_str(), sym.kind.as_str(), label);
            }
        }
        Format::Json => {
            println!("{}", serde_json::to_string_pretty(symbols).unwrap());
        }
    }
}
```

- [ ] **Step 2: Implement all query command handlers**

Each query command follows the same pattern:
1. Find project root
2. Open store
3. Run incremental reindex
4. Execute query
5. Print results

For commands that take a pattern and resolve to a single symbol first (refs, impls, supers, callers, callees): first `find_symbol` to get the target, then run the specific query. If multiple symbols match, list them and ask the user to be more specific. If exactly one matches, proceed.

- [ ] **Step 3: Build and test manually against a sample Java project**

Create a small test Java project, run `codix init`, `codix index`, then test each command.

- [ ] **Step 4: Commit**

```bash
git add src/cli/mod.rs src/main.rs
git commit -m "feat: implement all query commands with text and JSON output"
```

### Task 16: Integration tests

**Files:**
- Create: `tests/integration.rs`

- [ ] **Step 1: Write end-to-end integration tests**

```rust
use std::process::Command;
use std::fs;
use tempfile::TempDir;

fn codix_cmd(dir: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_codix"));
    cmd.current_dir(dir);
    cmd
}

#[test]
fn test_init_creates_codix_dir() {
    let tmp = TempDir::new().unwrap();
    let out = codix_cmd(tmp.path()).arg("init").output().unwrap();
    assert!(out.status.success());
    assert!(tmp.path().join(".codix").exists());
}

#[test]
fn test_full_workflow() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Setup
    codix_cmd(root).arg("init").output().unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/Foo.java"), "package com.foo;\npublic class Foo {\n  public void bar() {}\n}").unwrap();
    fs::write(root.join("src/Baz.java"), "package com.foo;\npublic class Baz extends Foo {\n  public void bar() { super.bar(); }\n}").unwrap();

    // Index
    let out = codix_cmd(root).arg("index").output().unwrap();
    assert!(out.status.success());

    // Find
    let out = codix_cmd(root).args(["find", "Foo"]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Foo"));
    assert!(stdout.contains("class"));

    // Symbols in file
    let out = codix_cmd(root).args(["symbols", "src/Foo.java"]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Foo"));
    assert!(stdout.contains("bar"));

    // Impls
    let out = codix_cmd(root).args(["impls", "Foo"]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Baz"));

    // JSON format
    let out = codix_cmd(root).args(["find", "Foo", "-f", "json"]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&stdout).is_ok());
}

#[test]
fn test_no_init_error() {
    let tmp = TempDir::new().unwrap();
    let out = codix_cmd(tmp.path()).args(["find", "Foo"]).output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("codix init"));
}
```

- [ ] **Step 2: Run integration tests**

Run: `cargo test --test integration`
Expected: all pass

- [ ] **Step 3: Commit**

```bash
git add tests/integration.rs
git commit -m "feat: add integration tests for full CLI workflow"
```
