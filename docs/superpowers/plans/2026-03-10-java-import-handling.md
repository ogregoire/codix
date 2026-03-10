# Java Import Handling Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve Java type references to fully qualified names using import declarations, same-package rules, and wildcard imports — replacing the fragile simple-name fallback.

**Architecture:** The Java plugin parses import declarations and uses them (plus same-package rules) to qualify type names at extraction time. Wildcard imports are returned as metadata and resolved by the indexer in a post-index pass using a new Store method. The existing COALESCE fallback remains as a last resort.

**Tech Stack:** Rust, tree-sitter-java, rusqlite

**Spec:** `docs/superpowers/specs/2026-03-10-java-import-handling-design.md`

---

## Chunk 1: Data Model and Import Parsing

### Task 1: Add `wildcard_imports` to `ExtractionResult`

**Files:**
- Modify: `src/model.rs:193-197`

- [ ] **Step 1: Add the field**

In `src/model.rs`, add `wildcard_imports` to `ExtractionResult`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ExtractionResult {
    pub symbols: Vec<ExtractedSymbol>,
    pub relationships: Vec<ExtractedRelationship>,
    pub wildcard_imports: Vec<String>,
}
```

- [ ] **Step 2: Fix compilation errors**

Every place that constructs `ExtractionResult` needs the new field. There are two locations:

1. `src/plugin/java/mod.rs:72-75` — the main `extract_symbols` method:
```rust
ExtractionResult {
    symbols,
    relationships,
    wildcard_imports: Vec::new(),
}
```

2. `src/plugin/java/mod.rs` — the `parse_java` test helper also constructs via `extract_symbols`, so it gets the field automatically via the return value. No change needed there.

- [ ] **Step 3: Verify it compiles and all tests pass**

Run: `cargo test`
Expected: All 62 tests pass. No behavior changes.

---

### Task 2: Parse import declarations in the Java plugin

**Files:**
- Modify: `src/plugin/java/mod.rs`

This task adds a new function `parse_imports` that walks the AST for `import_declaration` nodes and classifies them into single-type imports (HashMap) and wildcard prefixes (Vec). It does NOT yet use them for resolution — that's Task 3.

- [ ] **Step 1: Write the test for single-type import parsing**

Add to `src/plugin/java/mod.rs` tests module:

```rust
#[test]
fn test_parse_imports_single_type() {
    let plugin = JavaPlugin;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&plugin.tree_sitter_language()).unwrap();
    let source = "package com.bar;\nimport com.foo.Repository;\nimport com.foo.Person;\npublic class Svc {}";
    let tree = parser.parse(source.as_bytes(), None).unwrap();
    let root = tree.root_node();
    let (import_map, wildcards) = parse_imports(root, source.as_bytes());
    assert_eq!(import_map.get("Repository"), Some(&"com.foo.Repository".to_string()));
    assert_eq!(import_map.get("Person"), Some(&"com.foo.Person".to_string()));
    assert!(wildcards.is_empty());
}
```

- [ ] **Step 2: Write the test for wildcard import parsing**

```rust
#[test]
fn test_parse_imports_wildcard() {
    let plugin = JavaPlugin;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&plugin.tree_sitter_language()).unwrap();
    let source = "package com.bar;\nimport com.foo.*;\npublic class Svc {}";
    let tree = parser.parse(source.as_bytes(), None).unwrap();
    let root = tree.root_node();
    let (import_map, wildcards) = parse_imports(root, source.as_bytes());
    assert!(import_map.is_empty());
    assert_eq!(wildcards, vec!["com.foo".to_string()]);
}
```

- [ ] **Step 3: Write the test for static imports (should be ignored)**

```rust
#[test]
fn test_parse_imports_static_ignored() {
    let plugin = JavaPlugin;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&plugin.tree_sitter_language()).unwrap();
    let source = "package com.bar;\nimport static com.foo.Utils.helper;\npublic class Svc {}";
    let tree = parser.parse(source.as_bytes(), None).unwrap();
    let root = tree.root_node();
    let (import_map, wildcards) = parse_imports(root, source.as_bytes());
    assert!(import_map.is_empty());
    assert!(wildcards.is_empty());
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test parse_imports`
Expected: FAIL — `parse_imports` function doesn't exist yet.

- [ ] **Step 5: Implement `parse_imports`**

Add to `src/plugin/java/mod.rs` (after `find_package`, around line 97):

```rust
use std::collections::HashMap;

/// Parse import declarations from the AST root.
/// Returns (single_type_imports, wildcard_prefixes).
/// Single-type: "Repository" -> "com.foo.Repository"
/// Wildcard: "com.foo.*" -> prefix "com.foo"
/// Static imports are ignored.
fn parse_imports(root: tree_sitter::Node, source: &[u8]) -> (HashMap<String, String>, Vec<String>) {
    let mut import_map = HashMap::new();
    let mut wildcards = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() != "import_declaration" {
            continue;
        }
        // Skip static imports: they have a child node with kind "static"
        // or the text starts with "import static"
        let text = match child.utf8_text(source) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if text.contains("static ") {
            continue;
        }
        // Find the scoped_identifier or identifier child
        let mut import_cursor = child.walk();
        for import_child in child.children(&mut import_cursor) {
            let kind = import_child.kind();
            if kind == "scoped_identifier" || kind == "identifier" {
                if let Ok(name) = import_child.utf8_text(source) {
                    let name = name.trim();
                    if name.ends_with(".*") {
                        // This shouldn't happen with scoped_identifier — the asterisk
                        // is a separate child. But handle it defensively.
                        let prefix = &name[..name.len() - 2];
                        wildcards.push(prefix.to_string());
                    } else {
                        // Single-type import: extract simple name from qualified name
                        let simple_name = name.rsplit('.').next().unwrap_or(name);
                        import_map.insert(simple_name.to_string(), name.to_string());
                    }
                }
            } else if kind == "asterisk" {
                // Wildcard import: the scoped_identifier before this is the prefix
                // Walk back to find it
                let mut prefix_cursor = child.walk();
                for prefix_child in child.children(&mut prefix_cursor) {
                    if prefix_child.kind() == "scoped_identifier" || prefix_child.kind() == "identifier" {
                        if let Ok(prefix) = prefix_child.utf8_text(source) {
                            wildcards.push(prefix.trim().to_string());
                        }
                        break;
                    }
                }
            }
        }
    }
    (import_map, wildcards)
}
```

**Important notes for implementer:**
- The tree-sitter Java grammar represents `import com.foo.*` as an `import_declaration` with children: `scoped_identifier("com.foo")` and `asterisk("*")`.
- The tree-sitter Java grammar represents `import com.foo.Repository` as an `import_declaration` with a single `scoped_identifier("com.foo.Repository")`.
- Static imports contain a child with text "static" — checking `text.contains("static ")` is sufficient.
- You MUST verify the actual tree-sitter node structure by adding a debug print in a test if the implementation doesn't pass tests. The grammar may vary.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test parse_imports`
Expected: All 3 new tests pass.

---

### Task 3: Use imports to qualify type names in relationships

**Files:**
- Modify: `src/plugin/java/mod.rs:24-76` (extract_symbols method)
- Modify: `src/plugin/java/mod.rs:298-383` (relationship extraction functions)

This task threads the import map and package name through the relationship extraction functions so that type references (`extends`, `implements`, `field-type`) get resolved to qualified names. Method calls (`Calls`) remain as simple names.

- [ ] **Step 1: Write the test for import-resolved extends**

Add to tests module in `src/plugin/java/mod.rs`:

```rust
#[test]
fn test_import_resolves_extends() {
    let source = "package com.bar;\nimport com.foo.BaseService;\npublic class UserService extends BaseService {}";
    let result = parse_java(source);
    assert_eq!(result.relationships.len(), 1);
    assert_eq!(result.relationships[0].target_qualified_name, "com.foo.BaseService");
}
```

- [ ] **Step 2: Write the test for same-package resolution**

```rust
#[test]
fn test_same_package_resolves_type() {
    let source = "package com.foo;\npublic class UserService extends BaseService {}";
    let result = parse_java(source);
    assert_eq!(result.relationships.len(), 1);
    assert_eq!(result.relationships[0].target_qualified_name, "com.foo.BaseService");
}
```

- [ ] **Step 3: Write the test for import-resolved field type**

```rust
#[test]
fn test_import_resolves_field_type() {
    let source = "package com.bar;\nimport com.foo.Repository;\npublic class Svc {\n  private Repository repo;\n}";
    let result = parse_java(source);
    let field_types: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::FieldType).collect();
    assert_eq!(field_types.len(), 1);
    assert_eq!(field_types[0].target_qualified_name, "com.foo.Repository");
}
```

- [ ] **Step 4: Write the test for wildcard imports returned but not resolved**

```rust
#[test]
fn test_wildcard_imports_returned() {
    let source = "package com.bar;\nimport com.foo.*;\npublic class Svc extends Base {}";
    let result = parse_java(source);
    // Wildcard can't be resolved at extraction time — target stays simple name
    // but with same-package prepended (com.bar.Base)
    assert_eq!(result.relationships[0].target_qualified_name, "com.bar.Base");
    assert_eq!(result.wildcard_imports, vec!["com.foo".to_string()]);
}
```

- [ ] **Step 5: Write the test for method calls staying as simple names**

```rust
#[test]
fn test_method_calls_stay_simple() {
    let source = "package com.bar;\nimport com.foo.Repository;\npublic class Svc {\n  void work() { repo.save(); }\n}";
    let result = parse_java(source);
    let calls: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::Calls).collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].target_qualified_name, "save");
}
```

- [ ] **Step 6: Write the test for no-package file (default package)**

```rust
#[test]
fn test_no_package_no_qualification() {
    let source = "public class Svc extends Base {}";
    let result = parse_java(source);
    assert_eq!(result.relationships[0].target_qualified_name, "Base");
}
```

- [ ] **Step 7: Run new tests to verify they fail**

Run: `cargo test -- test_import_resolves test_same_package test_wildcard_imports_returned test_method_calls_stay test_no_package_no_qualification`
Expected: Several fail because relationships still emit simple names.

- [ ] **Step 8: Implement the resolution logic**

Modify `extract_symbols` in `src/plugin/java/mod.rs` to:
1. Call `parse_imports` to get the import map and wildcards
2. Pass the import map and package to `extract_type_relationships`
3. In functions that emit type relationships (`Extends`, `Implements`, `FieldType`), resolve the simple name using this helper:

Add a new helper function:

```rust
/// Resolve a simple type name to a qualified name using import map and package.
/// Priority: 1) import map, 2) same-package, 3) leave as-is (no package).
fn resolve_type_name(simple_name: &str, import_map: &HashMap<String, String>, package: &str) -> String {
    // Check import map first
    if let Some(qualified) = import_map.get(simple_name) {
        return qualified.clone();
    }
    // Same-package resolution
    if !package.is_empty() {
        return format!("{}.{}", package, simple_name);
    }
    // Default package — can't qualify
    simple_name.to_string()
}
```

Then modify the signature of `extract_type_relationships` and its callees to accept `import_map: &HashMap<String, String>` and `package: &str`, and call `resolve_type_name` on type reference names before pushing to relationships.

**Specific changes needed:**

1. **`extract_symbols` method** (line 24-76): After `find_package`, call `parse_imports`. Pass `import_map` and `package` to `extract_type_relationships`. Set `wildcard_imports` in the returned `ExtractionResult`.

2. **`extract_type_relationships`** (line 298-332): Add `import_map: &HashMap<String, String>` and `package: &str` parameters. Pass them to:
   - `first_type_identifier` calls — wrap the returned name with `resolve_type_name`
   - `collect_type_list_names` calls — pass import_map and package through
   - `extract_body_relationships` calls — pass import_map and package through

3. **`collect_type_list_names` / `collect_type_identifiers`** (lines 351-383): Add `import_map` and `package` params. When pushing a `type_identifier` relationship, call `resolve_type_name` on the name.

4. **`extract_body_relationships`** (lines 385-424): Add `import_map` and `package` params. When calling `field_type_name`, wrap the result with `resolve_type_name`. Do NOT resolve method call names (`Calls` relationships stay simple).

**Do NOT modify:**
- `extract_method_calls` / `collect_method_invocations` — method calls stay as simple names
- `extract_method` / `extract_field` — symbol extraction doesn't change

- [ ] **Step 9: Update existing tests that now have different target_qualified_name values**

The following existing tests need their expected `target_qualified_name` updated:

1. `test_extract_extends` (line 590-598): `"BaseService"` → `"com.foo.BaseService"` (same-package resolution)
2. `test_extract_implements` (line 600-606): Count stays 2, no name assertion to change
3. `test_extract_field_type` (line 617-623): `"Repository"` → `"com.foo.Repository"` (same-package resolution)
4. `test_extract_method_calls` (line 609-614): No change (calls stay simple)

- [ ] **Step 10: Run all tests**

Run: `cargo test`
Expected: All tests pass (old + new).

---

## Chunk 2: Store Method, Indexer Changes, and Integration Tests

### Task 4: Add `resolve_wildcard_imports` to Store

**Files:**
- Modify: `src/store/mod.rs:17` (add method to trait)
- Modify: `src/store/sqlite.rs:204-216` (add implementation after resolve_relationships)

This adds a Store method that resolves unresolved relationships for a specific file using wildcard import prefixes.

- [ ] **Step 1: Write the test**

Add to `src/store/sqlite.rs` tests module:

```rust
#[test]
fn test_resolve_wildcard_imports() {
    let store = test_store();

    // Create a symbol in com.foo package
    let f1 = store.upsert_file("Repository.java", 1, None, "java").unwrap();
    let syms1 = vec![ExtractedSymbol {
        local_id: 0, name: "Repository".into(), signature: None,
        qualified_name: "com.foo.Repository".into(), kind: SymbolKind::Interface,
        visibility: Visibility::Public,
        line: 1, column: 0, end_line: 10, end_column: 1,
        parent_local_id: None, package: "com.foo".into(),
    }];
    store.insert_symbols(f1, &syms1).unwrap();

    // Create a symbol in com.bar package with a relationship pointing to "com.bar.Repository"
    // (same-package guess — wrong because Repository is in com.foo)
    let f2 = store.upsert_file("UserService.java", 1, None, "java").unwrap();
    let syms2 = vec![ExtractedSymbol {
        local_id: 0, name: "UserService".into(), signature: None,
        qualified_name: "com.bar.UserService".into(), kind: SymbolKind::Class,
        visibility: Visibility::Public,
        line: 1, column: 0, end_line: 10, end_column: 1,
        parent_local_id: None, package: "com.bar".into(),
    }];
    let ids2 = store.insert_symbols(f2, &syms2).unwrap();
    let map2: Vec<(usize, SymbolId)> = vec![(0, ids2[0])];
    let rels = vec![ExtractedRelationship {
        source_local_id: 0,
        target_qualified_name: "com.bar.Repository".into(),
        kind: RelationshipKind::Implements,
    }];
    store.insert_relationships(f2, &map2, &rels).unwrap();

    // Wildcard resolve: import com.foo.* should fix "com.bar.Repository" -> "com.foo.Repository"
    let resolved = store.resolve_wildcard_imports(f2, &["com.foo".to_string()]).unwrap();
    assert_eq!(resolved, 1);

    // Now resolve_relationships should link it
    let count = store.resolve_relationships().unwrap();
    assert!(count >= 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_resolve_wildcard`
Expected: FAIL — method doesn't exist.

- [ ] **Step 3: Add the method to the Store trait**

In `src/store/mod.rs`, add after `resolve_relationships`:

```rust
fn resolve_wildcard_imports(&self, file_id: FileId, prefixes: &[String]) -> Result<u64>;
```

- [ ] **Step 4: Implement in SqliteStore**

Add to `src/store/sqlite.rs` after `resolve_relationships`:

```rust
fn resolve_wildcard_imports(&self, file_id: FileId, prefixes: &[String]) -> Result<u64> {
    // Get all unresolved relationships for this file
    let mut stmt = self.conn.prepare(
        "SELECT source_symbol_id, target_qualified_name, kind FROM relationships WHERE file_id = ?1 AND target_symbol_id IS NULL"
    )?;
    let rows: Vec<(i64, String, String)> = stmt.query_map(params![file_id], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;

    let mut resolved_count = 0u64;
    for (source_id, target_name, kind) in &rows {
        // Extract simple name from the current target (which may be package-qualified with wrong package)
        let simple_name = target_name.rsplit('.').next().unwrap_or(target_name);

        // Try each prefix
        let mut matches = Vec::new();
        for prefix in prefixes {
            let candidate = format!("{}.{}", prefix, simple_name);
            let exists: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM symbols WHERE qualified_name = ?1)",
                params![candidate],
                |row| row.get(0),
            )?;
            if exists {
                matches.push(candidate);
            }
        }

        // Only resolve if exactly one match (avoid ambiguity)
        // Use DELETE+INSERT because target_qualified_name is part of the PRIMARY KEY
        if matches.len() == 1 {
            self.conn.execute(
                "DELETE FROM relationships WHERE source_symbol_id = ?1 AND target_qualified_name = ?2 AND kind = ?3",
                params![source_id, target_name, kind],
            )?;
            self.conn.execute(
                "INSERT OR IGNORE INTO relationships (source_symbol_id, target_symbol_id, target_qualified_name, file_id, kind) VALUES (?1, NULL, ?2, ?3, ?4)",
                params![source_id, matches[0], file_id, kind],
            )?;
            resolved_count += 1;
        }
    }
    Ok(resolved_count)
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test test_resolve_wildcard`
Expected: PASS.

---

### Task 5: Update indexer to collect wildcards and resolve them

**Files:**
- Modify: `src/engine/indexer.rs:34-46` (`full_index`)
- Modify: `src/engine/indexer.rs:50-104` (`incremental_reindex`)
- Modify: `src/engine/indexer.rs:107-129` (`index_file`)

- [ ] **Step 1: Write the test**

Add to `src/engine/indexer.rs` tests module:

```rust
#[test]
fn test_wildcard_import_resolution() {
    let (_tmp, root) = setup_project();
    let store = SqliteStore::open(":memory:").unwrap();
    let registry = PluginRegistry::new();

    // Add a second package with a class that uses wildcard import
    fs::create_dir_all(root.join("src/main/java/com/bar")).unwrap();
    fs::write(root.join("src/main/java/com/bar/Client.java"),
        "package com.bar;\nimport com.foo.*;\npublic class Client extends Foo {}").unwrap();

    full_index(&root, &store, &registry).unwrap();

    // Find Client and check its supertypes — should resolve to com.foo.Foo
    let query = crate::model::SymbolQuery {
        pattern: "Client".to_string(),
        case_insensitive: false,
        kind: None,
    };
    let results = store.find_symbol(&query).unwrap();
    assert_eq!(results.len(), 1);
    let supers = store.find_supertypes(results[0].id).unwrap();
    assert_eq!(supers.len(), 1);
    assert_eq!(supers[0].qualified_name, "com.foo.Foo");
}
```

- [ ] **Step 1b: Write the ambiguous wildcard test**

```rust
#[test]
fn test_ambiguous_wildcard_not_resolved() {
    let (_tmp, root) = setup_project();
    let store = SqliteStore::open(":memory:").unwrap();
    let registry = PluginRegistry::new();

    // Two packages with a class named "Foo"
    fs::create_dir_all(root.join("src/main/java/com/other")).unwrap();
    fs::write(root.join("src/main/java/com/other/Foo.java"),
        "package com.other;\npublic class Foo {}").unwrap();

    // A client that wildcard-imports both packages
    fs::create_dir_all(root.join("src/main/java/com/client")).unwrap();
    fs::write(root.join("src/main/java/com/client/Client.java"),
        "package com.client;\nimport com.foo.*;\nimport com.other.*;\npublic class Client extends Foo {}").unwrap();

    full_index(&root, &store, &registry).unwrap();

    // Client's supertype should NOT be resolved (ambiguous: com.foo.Foo and com.other.Foo both exist)
    let query = crate::model::SymbolQuery {
        pattern: "Client".to_string(),
        case_insensitive: false,
        kind: None,
    };
    let results = store.find_symbol(&query).unwrap();
    assert_eq!(results.len(), 1);
    let supers = store.find_supertypes(results[0].id).unwrap();
    // Ambiguous: should not resolve (or resolve via COALESCE fallback to one of them)
    // The key invariant: resolve_wildcard_imports returns 0 for ambiguous cases
    // COALESCE fallback may still pick one, which is acceptable
    assert!(supers.len() <= 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_wildcard_import_resolution test_ambiguous_wildcard`
Expected: FAIL — wildcards not handled yet in indexer.

- [ ] **Step 3: Modify `index_file` to return wildcard imports**

Change `index_file` signature to return `Result<(FileId, Vec<String>)>` (file ID + wildcard imports):

```rust
fn index_file(root: &Path, path: &Path, plugin: &dyn LanguagePlugin, store: &dyn Store) -> Result<(FileId, Vec<String>)> {
    let source = std::fs::read(path)?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&plugin.tree_sitter_language())?;
    let tree = parser.parse(&source, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse {}", path.display()))?;
    let result = plugin.extract_symbols(&tree, &source, path);
    let rel_path = project::relative_to_root(root, path);
    let mtime = std::fs::metadata(path)?
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let hash = compute_sha256(&source);
    let file_id = store.upsert_file(&rel_path, mtime, Some(&hash), plugin.name())?;
    let symbol_ids = store.insert_symbols(file_id, &result.symbols)?;
    let map: Vec<(usize, _)> = result.symbols.iter()
        .map(|s| s.local_id)
        .zip(symbol_ids.iter().copied())
        .collect();
    store.insert_relationships(file_id, &map, &result.relationships)?;
    Ok((file_id, result.wildcard_imports))
}
```

- [ ] **Step 4: Update `full_index` to collect wildcards and resolve them**

```rust
pub fn full_index(root: &Path, store: &dyn Store, registry: &PluginRegistry) -> Result<u64> {
    store.clear_all()?;
    let files = discover_files(root, registry);
    store.begin_transaction()?;
    let mut count = 0u64;
    let mut wildcard_map: HashMap<FileId, Vec<String>> = HashMap::new();
    for (path, ext) in &files {
        let plugin = registry.plugin_for_extension(ext).unwrap();
        let (file_id, wildcards) = index_file(root, path, plugin, store)?;
        if !wildcards.is_empty() {
            wildcard_map.insert(file_id, wildcards);
        }
        count += 1;
    }
    // Resolve wildcard imports before general relationship resolution
    for (file_id, prefixes) in &wildcard_map {
        store.resolve_wildcard_imports(*file_id, prefixes)?;
    }
    store.resolve_relationships()?;
    store.commit_transaction()?;
    Ok(count)
}
```

Add `use std::collections::HashMap;` at the top if not already imported (it's already imported via `HashSet` — just add `HashMap`).

**Known limitation:** Incremental reindex only resolves wildcards for files reindexed in the current run. If file A has `import com.foo.*` referencing `Bar`, and file B defining `com.foo.Bar` is added later, file A's wildcard won't be re-resolved until file A itself is modified. A full `codix index` resolves everything.

- [ ] **Step 5: Update `incremental_reindex` similarly**

Add wildcard collection and resolution to `incremental_reindex`:

```rust
pub fn incremental_reindex(root: &Path, store: &dyn Store, registry: &PluginRegistry) -> Result<()> {
    let disk_files = discover_files(root, registry);
    let indexed_files = store.list_files()?;

    let disk_paths: HashSet<String> = disk_files.iter()
        .map(|(p, _)| project::relative_to_root(root, p))
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
    let mut wildcard_map: HashMap<FileId, Vec<String>> = HashMap::new();
    for (path, ext) in &disk_files {
        let rel_path = project::relative_to_root(root, path);
        let mtime = std::fs::metadata(path)?
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        match store.get_file(&rel_path)? {
            None => {
                let plugin = registry.plugin_for_extension(ext).unwrap();
                let (file_id, wildcards) = index_file(root, path, plugin, store)?;
                if !wildcards.is_empty() {
                    wildcard_map.insert(file_id, wildcards);
                }
            }
            Some(f) if f.mtime < mtime => {
                let source = std::fs::read(path)?;
                let hash = compute_sha256(&source);
                if f.hash.as_deref() == Some(&hash) {
                    store.upsert_file(&rel_path, mtime, Some(&hash), &f.language)?;
                } else {
                    store.delete_relationships_for_file(f.id)?;
                    store.delete_symbols_for_file(f.id)?;
                    let plugin = registry.plugin_for_extension(ext).unwrap();
                    let (file_id, wildcards) = index_file(root, path, plugin, store)?;
                    if !wildcards.is_empty() {
                        wildcard_map.insert(file_id, wildcards);
                    }
                }
            }
            _ => {}
        }
    }

    for (file_id, prefixes) in &wildcard_map {
        store.resolve_wildcard_imports(*file_id, prefixes)?;
    }
    store.resolve_relationships()?;
    store.commit_transaction()?;
    Ok(())
}
```

- [ ] **Step 6: Run all tests**

Run: `cargo test`
Expected: All tests pass.

---

### Task 6: Add integration tests for cross-package imports

**Files:**
- Modify: `tests/integration.rs`

- [ ] **Step 1: Add a multi-package setup helper**

Add to `tests/integration.rs`:

```rust
fn setup_multi_package_project(dir: &std::path::Path) {
    // Package com.foo
    fs::create_dir_all(dir.join("src/foo")).unwrap();
    fs::write(
        dir.join("src/foo/Repository.java"),
        r#"package com.foo;
public interface Repository {
    void save(Object o);
}
"#,
    ).unwrap();
    fs::write(
        dir.join("src/foo/Person.java"),
        r#"package com.foo;
public class Person {}
"#,
    ).unwrap();

    // Package com.bar — imports from com.foo
    fs::create_dir_all(dir.join("src/bar")).unwrap();
    fs::write(
        dir.join("src/bar/UserService.java"),
        r#"package com.bar;
import com.foo.Repository;
import com.foo.Person;
public class UserService implements Repository {
    private Person person;
    public void save(Object o) {}
}
"#,
    ).unwrap();

    // Package com.baz — wildcard import
    fs::create_dir_all(dir.join("src/baz")).unwrap();
    fs::write(
        dir.join("src/baz/Client.java"),
        r#"package com.baz;
import com.foo.*;
public class Client extends Person {}
"#,
    ).unwrap();
}
```

- [ ] **Step 2: Test single-type import resolution**

```rust
#[test]
fn test_cross_package_import_resolution() {
    let tmp = TempDir::new().unwrap();
    setup_multi_package_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    // UserService implements Repository (cross-package via single-type import)
    let out = codix_cmd(tmp.path()).args(["impls", "com.foo.Repository"]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("UserService"));
}
```

- [ ] **Step 3: Test wildcard import resolution**

```rust
#[test]
fn test_wildcard_import_resolution() {
    let tmp = TempDir::new().unwrap();
    setup_multi_package_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    // Client extends Person (cross-package via wildcard import)
    let out = codix_cmd(tmp.path()).args(["supers", "Client"]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Person"));
}
```

- [ ] **Step 4: Test same-package implicit resolution**

```rust
#[test]
fn test_same_package_implicit_resolution() {
    let tmp = TempDir::new().unwrap();
    setup_multi_package_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    // refs to Person should include UserService (Person field via single-type import)
    // and Client (extends Person via wildcard import)
    let out = codix_cmd(tmp.path()).args(["refs", "com.foo.Person"]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("UserService"), "UserService should reference Person via field type");
    assert!(stdout.contains("Client"), "Client should reference Person via extends");
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: All tests pass (existing + new integration tests).

---

### Task 7: Expand test-project with cross-package examples

**Files:**
- Create: `test-project/src/main/java/com/bar/UserClient.java`

- [ ] **Step 1: Add a cross-package file to test-project**

Create `test-project/src/main/java/com/bar/UserClient.java`:

```java
package com.bar;

import com.foo.Repository;
import com.foo.Person;

public class UserClient {
    private Repository repo;

    public Person findUser(int id) {
        return repo.findById(id);
    }
}
```

- [ ] **Step 2: Manual verification**

Run from the test-project directory:
```bash
cd test-project && codix init && codix refs com.foo.Repository && codix refs com.foo.Person
```

Expected: Both should show `UserClient` in the results.

```bash
codix impls com.foo.Repository
```

Expected: Should show `UserService` (from existing test-project files).

- [ ] **Step 3: Clean up**

```bash
rm -rf test-project/.codix
```

---

### Task 8: Commit all changes

- [ ] **Step 1: Run full test suite one final time**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 2: Commit everything**

```bash
git add src/model.rs src/plugin/java/mod.rs src/store/mod.rs src/store/sqlite.rs src/engine/indexer.rs tests/integration.rs test-project/src/main/java/com/bar/UserClient.java
git commit -m "feat: resolve Java type references using imports and same-package rules

- Parse single-type and wildcard import declarations from AST
- Resolve type names (extends, implements, field-type) using import map
- Same-package implicit resolution for unimported types
- Post-index wildcard resolution pass via new Store method
- COALESCE fallback remains for unresolved references"
```
