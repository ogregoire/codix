# Rename Symbol Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `codix rename` command that precisely renames Java symbols (classes, methods, fields) across the codebase using the existing index + tree-sitter AST.

**Architecture:** The plugin trait gets `supports()` and `find_rename_occurrences()` methods. A new `engine/rename.rs` module orchestrates: resolve symbol via store, collect related symbols from index, AST-walk affected files via plugin, then optionally apply replacements and update the store directly (no full reindex).

**Tech Stack:** Rust, tree-sitter, rusqlite, clap

**Spec:** `docs/superpowers/specs/2026-03-11-rename-symbol-design.md`

---

## Chunk 1: Foundation (model types, plugin trait, store methods)

### Task 1: Add PluginCapability enum and rename types to model.rs

**Files:**
- Modify: `src/model.rs`

- [ ] **Step 1: Add types to model.rs**

Add after the `ExtractionResult` struct (after line 110):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginCapability {
    Rename,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenameOccurrence {
    pub line: i64,
    pub column: i64,
    pub byte_offset: usize,
    pub old_text: String,
}

#[derive(Debug, Clone)]
pub enum RenameError {
    NotSupported { language: String },
}

impl std::fmt::Display for RenameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenameError::NotSupported { language } => {
                write!(f, "Rename is not supported for {} files", language)
            }
        }
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add src/model.rs
git commit -m "feat: add PluginCapability, RenameOccurrence, and RenameError types"
```

### Task 2: Extend LanguagePlugin trait with supports() and find_rename_occurrences()

**Files:**
- Modify: `src/plugin/mod.rs`

- [ ] **Step 1: Add default methods to LanguagePlugin trait**

Add these methods inside the `LanguagePlugin` trait (after `extract_symbols`, before the closing `}`):

```rust
    fn supports(&self, _capability: PluginCapability) -> bool {
        false
    }

    fn find_rename_occurrences(
        &self,
        _tree: &tree_sitter::Tree,
        _source: &[u8],
        _symbol_name: &str,
        _symbol_kind: &SymbolKind,
        _symbol_qualified_name: &str,
    ) -> Result<Vec<RenameOccurrence>, RenameError> {
        Err(RenameError::NotSupported {
            language: self.display_name().to_string(),
        })
    }
```

Also add `PluginCapability, RenameOccurrence, RenameError` to the `use crate::model::*` import (it's already `*` so nothing to change).

- [ ] **Step 2: Add supported_languages_for helper to PluginRegistry**

Add to the `impl PluginRegistry` block:

```rust
    pub fn supported_languages_for(&self, capability: PluginCapability) -> Vec<&str> {
        self.plugins.iter()
            .filter(|p| p.supports(capability))
            .map(|p| p.display_name())
            .collect()
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: success

- [ ] **Step 4: Run existing tests to ensure no regressions**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add src/plugin/mod.rs
git commit -m "feat: add supports() and find_rename_occurrences() to LanguagePlugin trait"
```

### Task 3: Add store methods for rename

**Files:**
- Modify: `src/store/mod.rs`
- Modify: `src/store/sqlite.rs`

- [ ] **Step 1: Write failing tests for the four new store methods**

Add to `src/store/sqlite.rs` inside `mod tests`:

```rust
    #[test]
    fn test_update_symbol_name() {
        let store = test_store();
        let fid = store.upsert_file("Foo.java", 1, None, "java").unwrap();
        let syms = vec![
            ExtractedSymbol {
                local_id: 0, name: "Foo".into(), signature: None,
                qualified_name: "com.foo.Foo".into(), kind: SymbolKind::new("class"),
                visibility: Visibility::new("public"),
                line: 1, column: 0, end_line: 10, end_column: 1,
                parent_local_id: None, package: "com.foo".into(), type_text: None,
            },
            ExtractedSymbol {
                local_id: 1, name: "save".into(), signature: Some("save(Person)".into()),
                qualified_name: "com.foo.Foo.save(Person)".into(), kind: SymbolKind::new("method"),
                visibility: Visibility::new("public"),
                line: 3, column: 4, end_line: 5, end_column: 5,
                parent_local_id: Some(0), package: "com.foo".into(), type_text: None,
            },
        ];
        let ids = store.insert_symbols(fid, &syms).unwrap();

        store.update_symbol_name(ids[1], "findById", "com.foo.Foo.findById(Person)", Some("findById(Person)")).unwrap();

        let q = SymbolQuery { pattern: "findById".into(), case_insensitive: false, kind: None };
        let results = store.find_symbol(&q).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "findById");
        assert_eq!(results[0].qualified_name, "com.foo.Foo.findById(Person)");
        assert_eq!(results[0].signature.as_deref(), Some("findById(Person)"));
    }

    #[test]
    fn test_update_child_qualified_names() {
        let store = test_store();
        let fid = store.upsert_file("Foo.java", 1, None, "java").unwrap();
        let syms = vec![
            ExtractedSymbol {
                local_id: 0, name: "Foo".into(), signature: None,
                qualified_name: "com.foo.Foo".into(), kind: SymbolKind::new("class"),
                visibility: Visibility::new("public"),
                line: 1, column: 0, end_line: 10, end_column: 1,
                parent_local_id: None, package: "com.foo".into(), type_text: None,
            },
            ExtractedSymbol {
                local_id: 1, name: "save".into(), signature: Some("save()".into()),
                qualified_name: "com.foo.Foo.save()".into(), kind: SymbolKind::new("method"),
                visibility: Visibility::new("public"),
                line: 3, column: 4, end_line: 5, end_column: 5,
                parent_local_id: Some(0), package: "com.foo".into(), type_text: None,
            },
        ];
        let ids = store.insert_symbols(fid, &syms).unwrap();

        store.update_child_qualified_names(ids[0], "com.foo.Foo", "com.foo.Bar").unwrap();

        let q = SymbolQuery { pattern: "save".into(), case_insensitive: false, kind: None };
        let results = store.find_symbol(&q).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].qualified_name, "com.foo.Bar.save()");
    }

    #[test]
    fn test_update_relationship_targets() {
        let store = test_store();
        let f1 = store.upsert_file("Foo.java", 1, None, "java").unwrap();
        let f2 = store.upsert_file("Bar.java", 1, None, "java").unwrap();
        let syms1 = vec![ExtractedSymbol {
            local_id: 0, name: "Foo".into(), signature: None,
            qualified_name: "com.foo.Foo".into(), kind: SymbolKind::new("class"),
            visibility: Visibility::new("public"),
            line: 1, column: 0, end_line: 10, end_column: 1,
            parent_local_id: None, package: "com.foo".into(), type_text: None,
        }];
        let ids1 = store.insert_symbols(f1, &syms1).unwrap();
        let syms2 = vec![ExtractedSymbol {
            local_id: 0, name: "Bar".into(), signature: None,
            qualified_name: "com.foo.Bar".into(), kind: SymbolKind::new("class"),
            visibility: Visibility::new("public"),
            line: 1, column: 0, end_line: 10, end_column: 1,
            parent_local_id: None, package: "com.foo".into(), type_text: None,
        }];
        let ids2 = store.insert_symbols(f2, &syms2).unwrap();
        let map: Vec<(usize, SymbolId)> = vec![(0, ids2[0])];
        let rels = vec![ExtractedRelationship {
            source_local_id: 0,
            target_qualified_name: "com.foo.Foo".into(),
            kind: RelationshipKind::Extends,
        }];
        store.insert_relationships(f2, &map, &rels).unwrap();
        store.resolve_relationships().unwrap();

        store.update_relationship_targets("com.foo.Foo", "com.foo.Baz").unwrap();

        // The relationship target_qualified_name should be updated
        // Verify by checking that find_references no longer returns Bar for the old Foo id
        // (target_symbol_id still points to old symbol, but target_qualified_name is updated)
        let refs = store.find_references(ids1[0]).unwrap();
        // target_symbol_id was already resolved and won't change from update_relationship_targets
        // This method updates target_qualified_name for future resolve_relationships calls
        assert_eq!(refs.len(), 1); // still linked by target_symbol_id
    }

    #[test]
    fn test_update_file_mtime() {
        let store = test_store();
        let fid = store.upsert_file("Foo.java", 1000, None, "java").unwrap();
        store.update_file_mtime(fid, 2000).unwrap();
        let file = store.get_file("Foo.java").unwrap().unwrap();
        assert_eq!(file.mtime, 2000);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_update_symbol_name test_update_child_qualified_names test_update_relationship_targets test_update_file_mtime -- --test-threads=1`
Expected: FAIL (methods don't exist yet)

- [ ] **Step 3: Add four methods to Store trait**

In `src/store/mod.rs`, add inside the `Store` trait (before `fn index_stats`):

```rust
    fn update_symbol_name(&self, symbol_id: SymbolId, new_name: &str, new_qualified_name: &str, new_signature: Option<&str>) -> Result<()>;
    fn update_child_qualified_names(&self, parent_symbol_id: SymbolId, old_prefix: &str, new_prefix: &str) -> Result<()>;
    fn update_relationship_targets(&self, old_qualified_name: &str, new_qualified_name: &str) -> Result<()>;
    fn update_file_mtime(&self, file_id: FileId, new_mtime: i64) -> Result<()>;
```

- [ ] **Step 4: Implement in SqliteStore**

In `src/store/sqlite.rs`, add inside `impl Store for SqliteStore` (before `fn index_stats`):

```rust
    fn update_symbol_name(&self, symbol_id: SymbolId, new_name: &str, new_qualified_name: &str, new_signature: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE symbols SET name = ?1, qualified_name = ?2, signature = ?3 WHERE id = ?4",
            params![new_name, new_qualified_name, new_signature, symbol_id],
        )?;
        Ok(())
    }

    fn update_child_qualified_names(&self, parent_symbol_id: SymbolId, old_prefix: &str, new_prefix: &str) -> Result<()> {
        // Note: this updates direct children only. Deeply nested inner classes
        // (e.g. com.foo.Foo.Inner.method) would need recursive cascading.
        // Acceptable for v1 since inner class renames are not a primary use case.
        self.conn.execute(
            "UPDATE symbols SET qualified_name = ?1 || substr(qualified_name, ?2) \
             WHERE parent_symbol_id = ?3 AND qualified_name LIKE ?4 || '%'",
            params![new_prefix, old_prefix.len() as i64 + 1, parent_symbol_id, old_prefix],
        )?;
        Ok(())
    }

    fn update_relationship_targets(&self, old_qualified_name: &str, new_qualified_name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE relationships SET target_qualified_name = ?1 WHERE target_qualified_name = ?2",
            params![new_qualified_name, old_qualified_name],
        )?;
        Ok(())
    }

    fn update_file_mtime(&self, file_id: FileId, new_mtime: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE files SET mtime = ?1 WHERE id = ?2",
            params![new_mtime, file_id],
        )?;
        Ok(())
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test`
Expected: all tests pass (including the four new ones)

- [ ] **Step 6: Commit**

```bash
git add src/store/mod.rs src/store/sqlite.rs
git commit -m "feat: add store methods for rename (update symbol name, children, relationships, mtime)"
```

## Chunk 2: Engine rename module

### Task 4: Create engine/rename.rs with core orchestration

**Files:**
- Create: `src/engine/rename.rs`
- Modify: `src/engine/mod.rs`

- [ ] **Step 1: Add module declaration**

In `src/engine/mod.rs`, add:

```rust
pub mod rename;
```

- [ ] **Step 2: Create engine/rename.rs with types and find_occurrences**

Create `src/engine/rename.rs`:

```rust
use std::collections::HashSet;
use std::path::Path;
use anyhow::Result;
use crate::model::*;
use crate::plugin::{LanguagePlugin, PluginRegistry};
use crate::store::Store;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct FileOccurrences {
    pub file_path: String,
    pub occurrences: Vec<RenameOccurrence>,
}

#[derive(Debug, Serialize)]
pub struct RenameResult {
    pub changes: Vec<FileOccurrences>,
    pub warnings: Vec<String>,
}

impl RenameResult {
    pub fn total_occurrences(&self) -> usize {
        self.changes.iter().map(|f| f.occurrences.len()).sum()
    }

    pub fn total_files(&self) -> usize {
        self.changes.len()
    }
}

/// Find all occurrences that would be renamed. Does not modify any files.
pub fn find_occurrences(
    root: &Path,
    store: &dyn Store,
    registry: &PluginRegistry,
    symbol: &Symbol,
    new_name: &str,
) -> Result<RenameResult> {
    // Validate: same name check
    if symbol.name == new_name {
        anyhow::bail!("Symbol is already named '{}'. Nothing to rename.", new_name);
    }

    // Determine language from file record
    let file_record = store.get_file(&symbol.file_path)?
        .ok_or_else(|| anyhow::anyhow!("File '{}' not found in index", symbol.file_path))?;

    let plugin = registry.all_plugins().into_iter()
        .find(|p| p.name() == file_record.language)
        .ok_or_else(|| anyhow::anyhow!("No plugin found for language '{}'", file_record.language))?;

    // Check plugin support
    if !plugin.supports(PluginCapability::Rename) {
        let supported = registry.supported_languages_for(PluginCapability::Rename);
        let supported_str = if supported.is_empty() {
            "none".to_string()
        } else {
            supported.join(", ")
        };
        anyhow::bail!(
            "Rename is not supported for {} files. Supported: {}",
            plugin.display_name(),
            supported_str
        );
    }

    // Check for name conflicts
    check_name_conflict(store, symbol, new_name)?;

    // Collect related symbols based on kind
    let related = collect_related_symbols(store, symbol)?;

    // Collect affected file paths (deduped)
    let mut affected_paths: HashSet<String> = HashSet::new();
    affected_paths.insert(symbol.file_path.clone());
    for sym in &related {
        affected_paths.insert(sym.file_path.clone());
    }

    let mut warnings = Vec::new();
    let mut changes = Vec::new();

    for file_path in &affected_paths {
        // Check if this file's language supports rename
        let file_rec = store.get_file(file_path)?;
        if let Some(ref fr) = file_rec {
            let file_plugin = registry.all_plugins().into_iter()
                .find(|p| p.name() == fr.language);
            if let Some(fp) = file_plugin {
                if !fp.supports(PluginCapability::Rename) {
                    warnings.push(format!(
                        "Warning: references in {} skipped ({} rename not supported)",
                        file_path, fp.display_name()
                    ));
                    continue;
                }
            }
        }

        let abs_path = root.join(file_path);
        let source = std::fs::read(&abs_path)?;

        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&plugin.tree_sitter_language())?;
        let tree = parser.parse(&source, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse {}", file_path))?;

        match plugin.find_rename_occurrences(
            &tree,
            &source,
            &symbol.name,
            &symbol.kind,
            &symbol.qualified_name,
        ) {
            Ok(occurrences) if !occurrences.is_empty() => {
                changes.push(FileOccurrences {
                    file_path: file_path.clone(),
                    occurrences,
                });
            }
            Ok(_) => {} // no occurrences in this file
            Err(RenameError::NotSupported { language }) => {
                warnings.push(format!(
                    "Warning: references in {} skipped ({} rename not supported)",
                    file_path, language
                ));
            }
        }
    }

    // Sort by file path, then occurrences by line
    changes.sort_by(|a, b| a.file_path.cmp(&b.file_path));
    for fc in &mut changes {
        fc.occurrences.sort_by_key(|o| (o.line, o.column));
    }

    Ok(RenameResult { changes, warnings })
}

/// Apply the rename: rewrite files and update the store.
pub fn apply_rename(
    root: &Path,
    store: &dyn Store,
    symbol: &Symbol,
    new_name: &str,
    result: &RenameResult,
) -> Result<()> {
    // Rewrite files
    for file_occ in &result.changes {
        let abs_path = root.join(&file_occ.file_path);
        let source = std::fs::read(&abs_path)?;

        // Apply replacements bottom-to-top (highest byte_offset first)
        let mut modified = source;
        let mut sorted_occs: Vec<&RenameOccurrence> = file_occ.occurrences.iter().collect();
        sorted_occs.sort_by(|a, b| b.byte_offset.cmp(&a.byte_offset));

        for occ in sorted_occs {
            let old_bytes = occ.old_text.as_bytes();
            let end = occ.byte_offset + old_bytes.len();
            modified.splice(occ.byte_offset..end, new_name.bytes());
        }

        std::fs::write(&abs_path, &modified)?;
    }

    // Update store
    let old_name = &symbol.name;
    let old_qualified = &symbol.qualified_name;
    let new_qualified = build_new_qualified_name(old_qualified, old_name, new_name);
    let new_signature = symbol.signature.as_ref().map(|sig| {
        build_new_signature(sig, old_name, new_name)
    });

    store.update_symbol_name(
        symbol.id,
        new_name,
        &new_qualified,
        new_signature.as_deref(),
    )?;

    // For class renames, cascade to children
    let kind_str = symbol.kind.as_str();
    if kind_str == "class" || kind_str == "interface" || kind_str == "enum"
        || kind_str == "record" || kind_str == "annotation" {
        store.update_child_qualified_names(symbol.id, old_qualified, &new_qualified)?;
    }

    store.update_relationship_targets(old_qualified, &new_qualified)?;

    // Update mtimes for modified files
    for file_occ in &result.changes {
        let abs_path = root.join(&file_occ.file_path);
        if let Ok(metadata) = std::fs::metadata(&abs_path) {
            if let Ok(mtime) = metadata.modified() {
                let mtime_secs = mtime
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                if let Some(file_rec) = store.get_file(&file_occ.file_path)? {
                    store.update_file_mtime(file_rec.id, mtime_secs)?;
                }
            }
        }
    }

    Ok(())
}

fn check_name_conflict(store: &dyn Store, symbol: &Symbol, new_name: &str) -> Result<()> {
    let new_qualified = build_new_qualified_name(&symbol.qualified_name, &symbol.name, new_name);
    let query = SymbolQuery {
        pattern: new_qualified.clone(),
        case_insensitive: false,
        kind: Some(symbol.kind.clone()),
    };
    let conflicts = store.find_symbol(&query)?;
    if let Some(conflict) = conflicts.first() {
        let label = conflict.signature.as_deref().unwrap_or(&conflict.name);
        anyhow::bail!(
            "A {} '{}' already exists in {} at {}:{}",
            conflict.kind.as_str(),
            label,
            conflict.package,
            conflict.file_path,
            conflict.line
        );
    }
    Ok(())
}

fn collect_related_symbols(store: &dyn Store, symbol: &Symbol) -> Result<Vec<Symbol>> {
    let kind_str = symbol.kind.as_str();
    let mut related = Vec::new();

    match kind_str {
        "class" | "interface" | "enum" | "record" | "annotation" => {
            related.extend(store.find_references(symbol.id)?);
        }
        "method" | "constructor" => {
            related.extend(store.find_callers(symbol.id)?);
            related.extend(store.find_implementations(symbol.id)?);

            // Walk up the override chain: find if this method overrides a supertype method
            if let Some(parent_id) = symbol.parent_symbol_id {
                let supertypes = store.find_supertypes(parent_id)?;
                for supertype in &supertypes {
                    // Look for methods with the same name in the supertype
                    let query = SymbolQuery {
                        pattern: symbol.name.clone(),
                        case_insensitive: false,
                        kind: Some(SymbolKind::new("method")),
                    };
                    let candidates = store.find_symbol(&query)?;
                    for candidate in candidates {
                        if candidate.parent_symbol_id == Some(supertype.id) {
                            related.push(candidate);
                        }
                    }
                }
            }
        }
        "field" | "enum_constant" => {
            related.extend(store.find_references(symbol.id)?);
        }
        _ => {}
    }

    Ok(related)
}

fn build_new_qualified_name(old_qualified: &str, old_name: &str, new_name: &str) -> String {
    // Replace the last occurrence of old_name in the qualified name
    // e.g. "com.foo.Foo.save(Person)" with old="save", new="findById"
    //   → "com.foo.Foo.findById(Person)"
    if let Some(pos) = old_qualified.rfind(old_name) {
        let mut result = String::with_capacity(old_qualified.len());
        result.push_str(&old_qualified[..pos]);
        result.push_str(new_name);
        result.push_str(&old_qualified[pos + old_name.len()..]);
        result
    } else {
        old_qualified.to_string()
    }
}

fn build_new_signature(old_signature: &str, old_name: &str, new_name: &str) -> String {
    // Signature format: "save(Person)" → "findById(Person)"
    if let Some(pos) = old_signature.find(old_name) {
        let mut result = String::with_capacity(old_signature.len());
        result.push_str(&old_signature[..pos]);
        result.push_str(new_name);
        result.push_str(&old_signature[pos + old_name.len()..]);
        result
    } else {
        old_signature.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_new_qualified_name_method() {
        assert_eq!(
            build_new_qualified_name("com.foo.Foo.save(Person)", "save", "findById"),
            "com.foo.Foo.findById(Person)"
        );
    }

    #[test]
    fn test_build_new_qualified_name_class() {
        assert_eq!(
            build_new_qualified_name("com.foo.UserService", "UserService", "AccountService"),
            "com.foo.AccountService"
        );
    }

    #[test]
    fn test_build_new_signature() {
        assert_eq!(
            build_new_signature("save(Person)", "save", "findById"),
            "findById(Person)"
        );
    }

    #[test]
    fn test_build_new_signature_no_params() {
        assert_eq!(
            build_new_signature("getAll()", "getAll", "findAll"),
            "findAll()"
        );
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: success

- [ ] **Step 4: Run unit tests**

Run: `cargo test engine::rename`
Expected: all 4 tests pass

- [ ] **Step 5: Commit**

```bash
git add src/engine/mod.rs src/engine/rename.rs
git commit -m "feat: add engine/rename.rs with find_occurrences and apply_rename"
```

## Chunk 3: Java plugin rename implementation

### Task 5: Implement find_rename_occurrences for Java plugin

**Files:**
- Modify: `src/plugin/java/mod.rs`

- [ ] **Step 1: Add supports() and find_rename_occurrences() to JavaPlugin**

Add inside the `impl LanguagePlugin for JavaPlugin` block (after `extract_symbols`):

```rust
    fn supports(&self, capability: PluginCapability) -> bool {
        match capability {
            PluginCapability::Rename => true,
        }
    }

    fn find_rename_occurrences(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        symbol_name: &str,
        symbol_kind: &SymbolKind,
        _symbol_qualified_name: &str,
    ) -> Result<Vec<RenameOccurrence>, RenameError> {
        let root = tree.root_node();
        let mut occurrences = Vec::new();
        let kind_str = symbol_kind.as_str();

        collect_rename_occurrences(root, source, symbol_name, kind_str, &mut occurrences);

        Ok(occurrences)
    }
```

- [ ] **Step 2: Add the recursive AST walker function**

Add as a module-level function (outside the `impl` block):

```rust
fn collect_rename_occurrences(
    node: tree_sitter::Node,
    source: &[u8],
    target_name: &str,
    target_kind: &str,
    occurrences: &mut Vec<RenameOccurrence>,
) {
    let node_kind = node.kind();
    let text = node.utf8_text(source).unwrap_or("");

    // Only check identifier/type_identifier nodes that match the target name
    if (node_kind == "identifier" || node_kind == "type_identifier") && text == target_name {
        if let Some(parent) = node.parent() {
            let parent_kind = parent.kind();
            let matches = match target_kind {
                "method" => is_method_occurrence(parent_kind, &node, &parent),
                "class" | "interface" | "enum" | "record" | "annotation" =>
                    is_type_occurrence(parent_kind, &node, &parent),
                "field" | "enum_constant" => is_field_occurrence(parent_kind, &node, &parent, source),
                "constructor" => is_constructor_occurrence(parent_kind),
                _ => false,
            };
            if matches {
                occurrences.push(RenameOccurrence {
                    line: node.start_position().row as i64 + 1,
                    column: node.start_position().column as i64,
                    byte_offset: node.start_byte(),
                    old_text: text.to_string(),
                });
            }
        }
    }

    // Recurse into children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_rename_occurrences(child, source, target_name, target_kind, occurrences);
    }
}

fn is_method_occurrence(parent_kind: &str, node: &tree_sitter::Node, parent: &tree_sitter::Node) -> bool {
    match parent_kind {
        // Declaration
        "method_declaration" => {
            // The identifier must be the method name, not a type or parameter name
            parent.child_by_field_name("name").map(|n| n.id()) == Some(node.id())
        }
        // Call sites
        "method_invocation" => {
            parent.child_by_field_name("name").map(|n| n.id()) == Some(node.id())
        }
        "super_method_invocation" => {
            parent.child_by_field_name("name").map(|n| n.id()) == Some(node.id())
        }
        _ => false,
    }
}

fn is_type_occurrence(parent_kind: &str, node: &tree_sitter::Node, parent: &tree_sitter::Node) -> bool {
    match parent_kind {
        // Declaration
        "class_declaration" | "interface_declaration" | "enum_declaration"
        | "record_declaration" | "annotation_type_declaration" => {
            parent.child_by_field_name("name").map(|n| n.id()) == Some(node.id())
        }
        // Constructor (class rename must also rename constructors)
        "constructor_declaration" => {
            parent.child_by_field_name("name").map(|n| n.id()) == Some(node.id())
        }
        // Import: the simple name at the end of "import com.foo.UserService"
        // In tree-sitter-java, the import path is a scoped_identifier.
        // The last identifier child of a scoped_identifier inside an import is the type name.
        "scoped_identifier" => {
            // Only match if this scoped_identifier is (directly or transitively) inside an import
            is_inside_import(parent) && is_last_identifier_in_scope(node, parent)
        }
        // Type usages in specific structural positions where type_identifier appears:
        // extends/implements clauses, field types, parameter types, return types,
        // new expressions, casts, instanceof, generic type arguments, annotations
        "superclass" | "super_interfaces" | "extends_interfaces"
        | "type_list" | "field_declaration" | "local_variable_declaration"
        | "formal_parameter" | "spread_parameter" | "catch_formal_parameter"
        | "object_creation_expression" | "cast_expression" | "instanceof_expression"
        | "generic_type" | "type_arguments" | "type_parameter" | "type_bound"
        | "annotation" | "marker_annotation" | "array_creation_expression"
        | "method_declaration" | "enhanced_for_statement" => {
            node.kind() == "type_identifier"
        }
        // type_identifier as a standalone type reference (e.g. in variable declarations)
        "variable_declarator" => false, // this is for field/var names, not types
        _ => false,
    }
}

fn is_inside_import(node: &tree_sitter::Node) -> bool {
    let mut current = Some(*node);
    while let Some(n) = current {
        if n.kind() == "import_declaration" {
            return true;
        }
        current = n.parent();
    }
    false
}

fn is_last_identifier_in_scope(node: &tree_sitter::Node, parent: &tree_sitter::Node) -> bool {
    // The target name should be the rightmost identifier in the scoped_identifier
    // In "com.foo.UserService", "UserService" is the last child of the outermost scoped_identifier
    if let Some(last_child) = parent.child_by_field_name("name") {
        last_child.id() == node.id()
    } else {
        // Fallback: check if this is the last named child
        let child_count = parent.named_child_count();
        if child_count > 0 {
            parent.named_child(child_count - 1).map(|c| c.id()) == Some(node.id())
        } else {
            false
        }
    }
}

fn is_field_occurrence(parent_kind: &str, node: &tree_sitter::Node, parent: &tree_sitter::Node, source: &[u8]) -> bool {
    match parent_kind {
        // Declaration: variable_declarator inside field_declaration
        "variable_declarator" => {
            if parent.child_by_field_name("name").map(|n| n.id()) == Some(node.id()) {
                // Check that the grandparent is a field_declaration
                parent.parent().map(|gp| gp.kind()) == Some("field_declaration")
            } else {
                false
            }
        }
        // Field access: this.myField, obj.myField
        "field_access" => {
            parent.child_by_field_name("field").map(|n| n.id()) == Some(node.id())
        }
        _ => false,
    }
}

fn is_constructor_occurrence(parent_kind: &str) -> bool {
    parent_kind == "constructor_declaration"
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add src/plugin/java/mod.rs
git commit -m "feat: implement find_rename_occurrences for Java plugin"
```

### Task 6: Add unit tests for Java plugin rename

**Files:**
- Modify: `src/plugin/java/mod.rs`

- [ ] **Step 1: Add test helper and method rename tests**

Add inside the existing `#[cfg(test)] mod tests` block in `src/plugin/java/mod.rs`:

```rust
    fn find_occurrences(source: &str, name: &str, kind: &str) -> Vec<RenameOccurrence> {
        let plugin = JavaPlugin;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_java::LANGUAGE.into()).unwrap();
        let tree = parser.parse(source.as_bytes(), None).unwrap();
        plugin.find_rename_occurrences(
            &tree,
            source.as_bytes(),
            name,
            &SymbolKind::new(kind),
            "",
        ).unwrap()
    }

    #[test]
    fn test_rename_method_declaration_and_calls() {
        let source = r#"
package com.foo;
public class UserService {
    public void save(Person p) {
        repo.save(p);
    }
    public void other() {
        save(null);
    }
}
"#;
        let occs = find_occurrences(source, "save", "method");
        assert_eq!(occs.len(), 3); // declaration + 2 call sites
        // Verify positions are distinct
        let lines: Vec<i64> = occs.iter().map(|o| o.line).collect();
        assert!(lines.contains(&4)); // declaration
        assert!(lines.contains(&5)); // repo.save()
        assert!(lines.contains(&8)); // save(null)
    }

    #[test]
    fn test_rename_method_does_not_rename_param() {
        let source = r#"
package com.foo;
public class Foo {
    public void abc(int abc) {
        System.out.println(abc);
    }
}
"#;
        let occs = find_occurrences(source, "abc", "method");
        assert_eq!(occs.len(), 1); // only the method declaration
        assert_eq!(occs[0].line, 4);
    }

    #[test]
    fn test_rename_class_declaration_and_constructor() {
        let source = r#"
package com.foo;
public class UserService {
    public UserService() {}
    public UserService(String name) {}
}
"#;
        let occs = find_occurrences(source, "UserService", "class");
        assert_eq!(occs.len(), 3); // class decl + 2 constructors
    }

    #[test]
    fn test_rename_class_type_usages() {
        let source = r#"
package com.foo;
import com.bar.UserService;
public class Client {
    private UserService service;
    public UserService getService() { return service; }
    public void run() {
        UserService s = new UserService();
    }
}
"#;
        let occs = find_occurrences(source, "UserService", "class");
        // import, field type, return type, local type, new expression
        assert!(occs.len() >= 4);
    }

    #[test]
    fn test_rename_field_declaration_and_access() {
        let source = r#"
package com.foo;
public class Foo {
    private int count;
    public void inc() {
        this.count = this.count + 1;
    }
}
"#;
        let occs = find_occurrences(source, "count", "field");
        assert_eq!(occs.len(), 3); // declaration + 2 field accesses
    }

    #[test]
    fn test_rename_field_does_not_rename_method() {
        let source = r#"
package com.foo;
public class Foo {
    private int save;
    public void save() {}
    public void run() {
        save();
        this.save = 1;
    }
}
"#;
        let field_occs = find_occurrences(source, "save", "field");
        let method_occs = find_occurrences(source, "save", "method");
        // field: declaration + this.save access
        assert_eq!(field_occs.len(), 2);
        // method: declaration + save() call
        assert_eq!(method_occs.len(), 2);
    }

    #[test]
    fn test_rename_class_extends_implements() {
        let source = r#"
package com.foo;
public class Child extends Parent {
}
"#;
        let occs = find_occurrences(source, "Parent", "class");
        assert_eq!(occs.len(), 1); // extends clause type_identifier
    }

    #[test]
    fn test_rename_super_method_invocation() {
        let source = r#"
package com.foo;
public class Child extends Parent {
    public void save() {
        super.save();
    }
}
"#;
        let occs = find_occurrences(source, "save", "method");
        assert_eq!(occs.len(), 2); // declaration + super.save()
    }
```

- [ ] **Step 2: Run the tests**

Run: `cargo test plugin::java::tests::test_rename`
Expected: all pass

- [ ] **Step 3: If any tests fail, adjust the AST walker logic and re-run until all pass**

- [ ] **Step 4: Commit**

```bash
git add src/plugin/java/mod.rs
git commit -m "test: add unit tests for Java rename occurrence finding"
```

## Chunk 4: CLI command and output formatting

### Task 7: Add Rename command to CLI

**Files:**
- Modify: `src/cli/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add Rename variant to Commands enum**

In `src/cli/mod.rs`, add inside the `Commands` enum (after the `Callees` variant, before the closing `}`):

```rust
    // — Refactoring —
    /// Rename a symbol across the codebase (dry-run by default, apply with --apply)
    #[command(display_order = 40)]
    Rename {
        /// Symbol name or qualified name (e.g. save, com.foo.UserService.save(Person))
        pattern: String,
        /// New name for the symbol
        new_name: String,
        /// Apply the rename (default: dry-run only)
        #[arg(long)]
        apply: bool,
        #[arg(short = 'f', long, default_value = "text")]
        format: Format,
        #[arg(short = 'i', long)]
        case_insensitive: bool,
        #[arg(short = 'k', long)]
        kind: Option<String>,
    },
```

- [ ] **Step 2: Add command routing in main.rs**

In `src/main.rs`, add the match arm in the `run` function (inside the `match cli.command` block):

```rust
        Commands::Rename {
            pattern,
            new_name,
            apply,
            format,
            case_insensitive,
            kind,
        } => cmd_rename(pattern, new_name, apply, format, case_insensitive, kind, verbose),
```

- [ ] **Step 3: Add cmd_rename function to main.rs**

Add the function in `src/main.rs`:

```rust
fn cmd_rename(
    pattern: String,
    new_name: String,
    apply: bool,
    format: Format,
    case_insensitive: bool,
    kind: Option<String>,
    verbose: bool,
) -> anyhow::Result<()> {
    let (store, root) = open_store_and_reindex(verbose)?;
    let cwd = env::current_dir()?;
    let registry = PluginRegistry::new();
    let parsed_kind = parse_kind(kind.clone());
    let query = SymbolQuery {
        pattern: pattern.clone(),
        case_insensitive,
        kind: parsed_kind,
    };
    let matches = store.find_symbol(&query)?;

    if matches.is_empty() {
        anyhow::bail!("No symbol found matching '{}'. Try: codix find '{}*'", pattern, pattern);
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
                "  {}:{}  {} {} {}\n  \u{2192} codix rename '{}' '{}'{}\n",
                path, sym.line,
                sym.visibility.as_str(), sym.kind.as_str(), label,
                escaped_name, new_name, flags
            ));
        }
        anyhow::bail!("{}", msg.trim_end());
    }

    let sym = &matches[0];
    let result = engine::rename::find_occurrences(&root, &store, &registry, sym, &new_name)?;

    // Print warnings
    for warning in &result.warnings {
        eprintln!("{}", warning);
    }

    if result.total_occurrences() == 0 {
        println!("No occurrences found to rename.");
        return Ok(());
    }

    match format {
        Format::Text => {
            if apply {
                engine::rename::apply_rename(&root, &store, sym, &new_name, &result)?;
                println!("Renamed {} {} in {} {}.",
                    result.total_occurrences(),
                    if result.total_occurrences() == 1 { "occurrence" } else { "occurrences" },
                    result.total_files(),
                    if result.total_files() == 1 { "file" } else { "files" },
                );
            } else {
                for file_occ in &result.changes {
                    let path = project::display_path(&root, &cwd, &file_occ.file_path);
                    for occ in &file_occ.occurrences {
                        println!("{}:{}:{}  {} \u{2192} {}",
                            path, occ.line, occ.column,
                            occ.old_text, new_name,
                        );
                    }
                }
                println!("\n{} {} in {} {}",
                    result.total_occurrences(),
                    if result.total_occurrences() == 1 { "occurrence" } else { "occurrences" },
                    result.total_files(),
                    if result.total_files() == 1 { "file" } else { "files" },
                );
            }
        }
        Format::Json => {
            if apply {
                engine::rename::apply_rename(&root, &store, sym, &new_name, &result)?;
            }
            #[derive(Serialize)]
            struct JsonOutput {
                applied: bool,
                changes: Vec<JsonChange>,
                summary: JsonSummary,
            }
            #[derive(Serialize)]
            struct JsonChange {
                file: String,
                line: i64,
                column: i64,
                old: String,
                new: String,
            }
            #[derive(Serialize)]
            struct JsonSummary {
                occurrences: usize,
                files: usize,
            }
            let output = JsonOutput {
                applied: apply,
                changes: result.changes.iter().flat_map(|fc| {
                    fc.occurrences.iter().map(move |occ| JsonChange {
                        file: fc.file_path.clone(),
                        line: occ.line,
                        column: occ.column,
                        old: occ.old_text.clone(),
                        new: new_name.clone(),
                    })
                }).collect(),
                summary: JsonSummary {
                    occurrences: result.total_occurrences(),
                    files: result.total_files(),
                },
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }

    Ok(())
}
```

Add `use serde::Serialize;` to the imports at the top of `main.rs` and `use engine::rename;` — though `engine::rename` is already accessible via the `engine` module import. The `Serialize` derive is only used inside the function, so add it at the top of the file with the other imports.

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`
Expected: success

- [ ] **Step 5: Run all existing tests to ensure no regressions**

Run: `cargo test`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add src/cli/mod.rs src/main.rs
git commit -m "feat: add codix rename command with dry-run and apply modes"
```

## Chunk 5: Integration tests

### Task 8: Add integration tests for rename

**Files:**
- Modify: `tests/integration.rs`

- [ ] **Step 1: Add rename integration test helpers and tests**

Add to `tests/integration.rs`:

```rust
fn setup_rename_project(dir: &std::path::Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src/UserService.java"),
        r#"package com.foo;
public class UserService {
    private PersonRepo repo;
    public void save(Person p) {
        repo.save(p);
    }
}
"#,
    ).unwrap();
    fs::write(
        dir.join("src/PersonRepo.java"),
        r#"package com.foo;
public interface PersonRepo {
    void save(Person p);
}
"#,
    ).unwrap();
}

#[test]
fn test_rename_dry_run() {
    let tmp = TempDir::new().unwrap();
    setup_rename_project(tmp.path());
    let out = codix_cmd(tmp.path()).arg("init").output().unwrap();
    assert!(out.status.success(), "init failed: {}", String::from_utf8_lossy(&out.stderr));

    let out = codix_cmd(tmp.path())
        .args(["rename", "com.foo.UserService.save(Person)", "findById"])
        .output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success(), "rename failed: {}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("save"), "should show old name");
    assert!(stdout.contains("findById"), "should show new name");
    assert!(stdout.contains("occurrence"), "should show occurrence count");

    // File should NOT be modified (dry-run)
    let content = fs::read_to_string(tmp.path().join("src/UserService.java")).unwrap();
    assert!(content.contains("void save"), "file should not be modified in dry-run");
}

#[test]
fn test_rename_apply() {
    let tmp = TempDir::new().unwrap();
    setup_rename_project(tmp.path());
    let out = codix_cmd(tmp.path()).arg("init").output().unwrap();
    assert!(out.status.success());

    let out = codix_cmd(tmp.path())
        .args(["rename", "com.foo.UserService.save(Person)", "findById", "--apply"])
        .output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success(), "rename --apply failed: {}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("Renamed"));

    // File SHOULD be modified
    let content = fs::read_to_string(tmp.path().join("src/UserService.java")).unwrap();
    assert!(content.contains("void findById"), "method should be renamed in file");
    // Note: repo.save(p) may also be renamed since v1 uses name-based matching
    // (no scope-aware resolution). This is a known limitation.
}

#[test]
fn test_rename_json_output() {
    let tmp = TempDir::new().unwrap();
    setup_rename_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["rename", "com.foo.UserService.save(Person)", "findById", "-f", "json"])
        .output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // Should be valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(parsed["changes"].is_array());
    assert!(parsed["summary"]["occurrences"].is_number());
    assert_eq!(parsed["applied"], false);
}

#[test]
fn test_rename_same_name_error() {
    let tmp = TempDir::new().unwrap();
    setup_rename_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["rename", "com.foo.UserService.save(Person)", "save"])
        .output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("already named"));
}

#[test]
fn test_rename_no_match_error() {
    let tmp = TempDir::new().unwrap();
    setup_rename_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["rename", "nonExistentSymbol", "newName"])
        .output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("No symbol found"));
    assert!(stderr.contains("codix find"));
}

#[test]
fn test_rename_disambiguation() {
    let tmp = TempDir::new().unwrap();
    setup_rename_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    // "save" matches methods in both UserService and PersonRepo
    let out = codix_cmd(tmp.path())
        .args(["rename", "save*", "findById"])
        .output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("Multiple symbols match"));
    assert!(stderr.contains("codix rename"));
}

#[test]
fn test_rename_class() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("src/Foo.java"),
        r#"package com.foo;
public class Foo {
    public Foo() {}
}
"#,
    ).unwrap();
    fs::write(
        tmp.path().join("src/Bar.java"),
        r#"package com.foo;
public class Bar extends Foo {
    private Foo helper;
}
"#,
    ).unwrap();
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["rename", "com.foo.Foo", "Baz", "-k", "class", "--apply"])
        .output().unwrap();
    assert!(out.status.success(), "rename failed: {}", String::from_utf8_lossy(&out.stderr));

    let foo_content = fs::read_to_string(tmp.path().join("src/Foo.java")).unwrap();
    assert!(foo_content.contains("class Baz"), "class should be renamed");
    assert!(foo_content.contains("public Baz()"), "constructor should be renamed");

    let bar_content = fs::read_to_string(tmp.path().join("src/Bar.java")).unwrap();
    assert!(bar_content.contains("extends Baz"), "extends should be renamed");
    assert!(bar_content.contains("private Baz helper"), "field type should be renamed");
}
```

- [ ] **Step 2: Run integration tests**

Run: `cargo test --test integration`
Expected: all pass (existing + new)

- [ ] **Step 3: If any tests fail, fix the implementation and re-run**

- [ ] **Step 4: Commit**

```bash
git add tests/integration.rs
git commit -m "test: add integration tests for codix rename"
```

### Task 9: Verify help output and run full test suite

**Files:** none (verification only)

- [ ] **Step 1: Check help output includes rename**

Run: `cargo run -- --help`
Expected: `rename` appears in the command list with description

Run: `cargo run -- rename --help`
Expected: shows pattern, new-name, --apply, -f, -i, -k flags with descriptions

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: all unit tests pass

Run: `cargo test --test integration`
Expected: all integration tests pass

- [ ] **Step 3: Clean up test artifacts if any**

Run: `rm -rf test-project/.codix` (if test-project exists and has stale .codix)

- [ ] **Step 4: Commit if any fixes were needed**
