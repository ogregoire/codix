# Method Call Resolution Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve method call targets to qualified `ClassName.methodName` keys using receiver type inference from local scope data, replacing simple-name-only matching.

**Architecture:** The Java plugin builds a per-method scope map (fields, parameters, `this`) to resolve receiver types at extraction time. The core stores an additional `type_text` column on symbols and uses method-key-based matching in `find_callers`/`find_callees` at query time. Single-pass indexing, no cross-file lookups. Core stays language-agnostic.

**Tech Stack:** Rust, tree-sitter (Java grammar), SQLite (rusqlite)

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `src/model.rs` | Modify | Add `type_text: Option<String>` to `ExtractedSymbol` |
| `src/store/sqlite.rs` | Modify | Schema migration, `insert_symbols` update, new `find_callers`/`find_callees` queries, `resolve_relationships` skip Calls, new index |
| `src/plugin/java/mod.rs` | Modify | `type_text` extraction (field types, return types), scope map construction, receiver-resolved method call targets |
| `tests/integration.rs` | Modify | Add cross-package caller/callee integration tests |

---

## Chunk 1: Core Schema and Model

### Task 1: Add `type_text` to model and schema

**Files:**
- Modify: `src/model.rs:170-184` (ExtractedSymbol struct)
- Modify: `src/store/sqlite.rs:46-89` (schema, insert_symbols)

- [ ] **Step 1: Add `type_text` field to `ExtractedSymbol`**

In `src/model.rs`, add `type_text: Option<String>` to the `ExtractedSymbol` struct after the `package` field:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ExtractedSymbol {
    pub local_id: usize,
    pub name: String,
    pub signature: Option<String>,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub visibility: Visibility,
    pub line: i64,
    pub column: i64,
    pub end_line: i64,
    pub end_column: i64,
    pub parent_local_id: Option<usize>,
    pub package: String,
    pub type_text: Option<String>,
}
```

- [ ] **Step 2: Fix all compilation errors from the new field**

Every place that constructs an `ExtractedSymbol` now needs `type_text: None` (or a value). These are all in `src/plugin/java/mod.rs`:

- `extract_type_declaration` (line ~182): add `type_text: None,`
- `extract_method` (line ~292): add `type_text: None,` (we'll populate this in Task 3)
- `extract_field` (line ~334): add `type_text: None,` (we'll populate this in Task 3)
- All test helper constructors in `src/store/sqlite.rs` tests that create `ExtractedSymbol`: add `type_text: None,`

- [ ] **Step 3: Add `type_text` column to the schema**

In `src/store/sqlite.rs` `open()`, update the `CREATE TABLE symbols` statement to include the new column:

```sql
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
    package TEXT NOT NULL,
    type_text TEXT
);
```

Also add the new index for the query pattern:

```sql
CREATE INDEX IF NOT EXISTS idx_relationships_target_qname_kind ON relationships(target_qualified_name, kind);
```

- [ ] **Step 4: Update `insert_symbols` to include `type_text`**

In `src/store/sqlite.rs` `insert_symbols()` (line ~146), update the INSERT statement:

```rust
self.conn.execute(
    "INSERT INTO symbols (name, signature, qualified_name, kind, visibility, file_id, line, column_, end_line, end_column, parent_symbol_id, package, type_text)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11, ?12)",
    params![
        sym.name, sym.signature, sym.qualified_name,
        sym.kind.as_str(), sym.visibility.as_str(),
        file_id, sym.line, sym.column, sym.end_line, sym.end_column,
        sym.package, sym.type_text
    ],
)?;
```

- [ ] **Step 5: Run all tests to verify nothing broke**

Run: `cargo test`
Expected: All existing tests pass. The new `type_text` field defaults to `None` everywhere, so behavior is unchanged.

---

### Task 2: Rewrite `find_callers`/`find_callees` with method key matching

**Files:**
- Modify: `src/store/sqlite.rs:348-374` (find_callers, find_callees)

- [ ] **Step 1: Write failing test for method-key-based `find_callers`**

In `src/store/sqlite.rs` `mod tests`, add:

```rust
#[test]
fn test_find_callers_via_method_key() {
    let store = test_store();
    // Class: com.foo.Repository with method save(Object)
    let f1 = store.upsert_file("Repository.java", 1, None, "java").unwrap();
    let syms1 = vec![
        ExtractedSymbol {
            local_id: 0, name: "Repository".into(), signature: None,
            qualified_name: "com.foo.Repository".into(), kind: SymbolKind::Interface,
            visibility: Visibility::Public,
            line: 1, column: 0, end_line: 5, end_column: 1,
            parent_local_id: None, package: "com.foo".into(), type_text: None,
        },
        ExtractedSymbol {
            local_id: 1, name: "save".into(), signature: Some("save(Object)".into()),
            qualified_name: "com.foo.Repository.save(Object)".into(), kind: SymbolKind::Method,
            visibility: Visibility::Public,
            line: 3, column: 4, end_line: 3, end_column: 30,
            parent_local_id: Some(0), package: "com.foo".into(), type_text: None,
        },
    ];
    let ids1 = store.insert_symbols(f1, &syms1).unwrap();
    let save_id = ids1[1];

    // Class: com.bar.Service with method doWork() that calls com.foo.Repository.save
    let f2 = store.upsert_file("Service.java", 1, None, "java").unwrap();
    let syms2 = vec![
        ExtractedSymbol {
            local_id: 0, name: "Service".into(), signature: None,
            qualified_name: "com.bar.Service".into(), kind: SymbolKind::Class,
            visibility: Visibility::Public,
            line: 1, column: 0, end_line: 10, end_column: 1,
            parent_local_id: None, package: "com.bar".into(), type_text: None,
        },
        ExtractedSymbol {
            local_id: 1, name: "doWork".into(), signature: Some("doWork()".into()),
            qualified_name: "com.bar.Service.doWork()".into(), kind: SymbolKind::Method,
            visibility: Visibility::Public,
            line: 5, column: 4, end_line: 8, end_column: 5,
            parent_local_id: Some(0), package: "com.bar".into(), type_text: None,
        },
    ];
    let ids2 = store.insert_symbols(f2, &syms2).unwrap();
    let map2: Vec<(usize, SymbolId)> = vec![(0, ids2[0]), (1, ids2[1])];

    // Relationship: doWork calls com.foo.Repository.save (receiver-resolved target)
    let rels = vec![ExtractedRelationship {
        source_local_id: 1,
        target_qualified_name: "com.foo.Repository.save".into(),
        kind: RelationshipKind::Calls,
    }];
    store.insert_relationships(f2, &map2, &rels).unwrap();
    store.resolve_relationships().unwrap();

    // find_callers should find doWork as a caller of save(Object)
    let callers = store.find_callers(save_id).unwrap();
    assert_eq!(callers.len(), 1);
    assert_eq!(callers[0].name, "doWork");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_find_callers_via_method_key -- --nocapture`
Expected: FAIL — the current `find_callers` only matches on `target_symbol_id`, which is NULL for the `com.foo.Repository.save` target (COALESCE can't match it to any symbol's `qualified_name` because the symbol is `com.foo.Repository.save(Object)` with the signature).

- [ ] **Step 3: Write failing test for method-key-based `find_callees`**

```rust
#[test]
fn test_find_callees_via_method_key() {
    let store = test_store();
    // Same setup as test_find_callers_via_method_key
    let f1 = store.upsert_file("Repository.java", 1, None, "java").unwrap();
    let syms1 = vec![
        ExtractedSymbol {
            local_id: 0, name: "Repository".into(), signature: None,
            qualified_name: "com.foo.Repository".into(), kind: SymbolKind::Interface,
            visibility: Visibility::Public,
            line: 1, column: 0, end_line: 5, end_column: 1,
            parent_local_id: None, package: "com.foo".into(), type_text: None,
        },
        ExtractedSymbol {
            local_id: 1, name: "save".into(), signature: Some("save(Object)".into()),
            qualified_name: "com.foo.Repository.save(Object)".into(), kind: SymbolKind::Method,
            visibility: Visibility::Public,
            line: 3, column: 4, end_line: 3, end_column: 30,
            parent_local_id: Some(0), package: "com.foo".into(), type_text: None,
        },
    ];
    store.insert_symbols(f1, &syms1).unwrap();

    let f2 = store.upsert_file("Service.java", 1, None, "java").unwrap();
    let syms2 = vec![
        ExtractedSymbol {
            local_id: 0, name: "Service".into(), signature: None,
            qualified_name: "com.bar.Service".into(), kind: SymbolKind::Class,
            visibility: Visibility::Public,
            line: 1, column: 0, end_line: 10, end_column: 1,
            parent_local_id: None, package: "com.bar".into(), type_text: None,
        },
        ExtractedSymbol {
            local_id: 1, name: "doWork".into(), signature: Some("doWork()".into()),
            qualified_name: "com.bar.Service.doWork()".into(), kind: SymbolKind::Method,
            visibility: Visibility::Public,
            line: 5, column: 4, end_line: 8, end_column: 5,
            parent_local_id: Some(0), package: "com.bar".into(), type_text: None,
        },
    ];
    let ids2 = store.insert_symbols(f2, &syms2).unwrap();
    let dowork_id = ids2[1];
    let map2: Vec<(usize, SymbolId)> = vec![(0, ids2[0]), (1, ids2[1])];

    let rels = vec![ExtractedRelationship {
        source_local_id: 1,
        target_qualified_name: "com.foo.Repository.save".into(),
        kind: RelationshipKind::Calls,
    }];
    store.insert_relationships(f2, &map2, &rels).unwrap();
    store.resolve_relationships().unwrap();

    // find_callees should find save(Object) as a callee of doWork
    let callees = store.find_callees(dowork_id).unwrap();
    assert_eq!(callees.len(), 1);
    assert_eq!(callees[0].name, "save");
}
```

**Note:** `resolve_relationships` stays unchanged. It continues resolving ALL relationship kinds via COALESCE, including Calls. For Calls with method-key targets like `"com.foo.Repository.save"`, COALESCE won't find a match (the symbol's qualified_name includes the signature), so `target_symbol_id` stays NULL for those. For simple-name calls like `"save"`, COALESCE's name fallback still works. The new `find_callers`/`find_callees` queries handle both cases.

- [ ] **Step 4: Rewrite `find_callers`**

Replace the existing `find_callers` in `src/store/sqlite.rs`:

```rust
fn find_callers(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>> {
    let mut stmt = self.conn.prepare(
        "SELECT s.id, s.name, s.signature, s.kind, s.qualified_name, s.visibility, \
         s.file_id, s.line, s.column_, s.end_line, s.end_column, s.parent_symbol_id, s.package, f.path as file_path \
         FROM relationships r \
         JOIN symbols s ON s.id = r.source_symbol_id \
         JOIN files f ON s.file_id = f.id \
         WHERE r.kind = 'calls' \
         AND (r.target_symbol_id = ?1 \
              OR r.target_qualified_name = ( \
                  SELECT parent.qualified_name || '.' || target.name \
                  FROM symbols target \
                  JOIN symbols parent ON parent.id = target.parent_symbol_id \
                  WHERE target.id = ?1 \
              ))"
    )?;
    let symbols = stmt.query_map(params![symbol_id], Self::symbol_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(symbols)
}
```

This uses UNION logic via OR: matches either via `target_symbol_id` (legacy/fallback) or via the method key (`parent.qualified_name || '.' || target.name`).

- [ ] **Step 5: Rewrite `find_callees`**

Replace the existing `find_callees` in `src/store/sqlite.rs`:

```rust
fn find_callees(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>> {
    let mut stmt = self.conn.prepare(
        "SELECT DISTINCT callee.id, callee.name, callee.signature, callee.kind, callee.qualified_name, callee.visibility, \
         callee.file_id, callee.line, callee.column_, callee.end_line, callee.end_column, callee.parent_symbol_id, callee.package, f.path as file_path \
         FROM relationships r \
         JOIN symbols callee ON callee.kind IN ('method', 'constructor') \
         JOIN symbols parent ON parent.id = callee.parent_symbol_id \
             AND parent.qualified_name || '.' || callee.name = r.target_qualified_name \
         JOIN files f ON callee.file_id = f.id \
         WHERE r.source_symbol_id = ?1 AND r.kind = 'calls' \
         UNION \
         SELECT s.id, s.name, s.signature, s.kind, s.qualified_name, s.visibility, \
         s.file_id, s.line, s.column_, s.end_line, s.end_column, s.parent_symbol_id, s.package, f2.path as file_path \
         FROM relationships r2 \
         JOIN symbols s ON s.id = r2.target_symbol_id \
         JOIN files f2 ON s.file_id = f2.id \
         WHERE r2.source_symbol_id = ?1 AND r2.kind = 'calls' AND r2.target_symbol_id IS NOT NULL"
    )?;
    let symbols = stmt.query_map(params![symbol_id], Self::symbol_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(symbols)
}
```

First branch: method-key matching for resolved calls. Second branch: `target_symbol_id` fallback for legacy/unresolved.

- [ ] **Step 6: Run tests to verify both new tests pass and old tests still pass**

Run: `cargo test`
Expected: ALL tests pass, including `test_find_callers_via_method_key` and `test_find_callees_via_method_key`. The existing `seed_store` tests (`test_find_callers`, `test_find_callees`) still pass because their Calls relationships use exact qualified names like `"com.foo.PersonRepo.findAll()"` which COALESCE resolves to `target_symbol_id`, and the new queries match on `target_symbol_id` via the OR/UNION fallback.

---

## Chunk 2: Java Plugin — type_text and Receiver Resolution

### Task 3: Extract `type_text` for fields and methods

**Files:**
- Modify: `src/plugin/java/mod.rs:259-306` (extract_method)
- Modify: `src/plugin/java/mod.rs:308-348` (extract_field)

- [ ] **Step 1: Write failing tests for `type_text` extraction**

Add to `src/plugin/java/mod.rs` `mod tests`:

```rust
#[test]
fn test_field_type_text() {
    let source = "package com.foo;\nimport com.bar.Repository;\npublic class Svc {\n  private Repository repo;\n}";
    let result = parse_java(source);
    let field = result.symbols.iter().find(|s| s.kind == SymbolKind::Field).unwrap();
    assert_eq!(field.type_text, Some("com.bar.Repository".to_string()));
}

#[test]
fn test_method_return_type_text() {
    let source = "package com.foo;\nimport com.bar.Person;\npublic class Svc {\n  public Person findById(int id) { return null; }\n}";
    let result = parse_java(source);
    let method = result.symbols.iter().find(|s| s.kind == SymbolKind::Method).unwrap();
    assert_eq!(method.type_text, Some("com.bar.Person".to_string()));
}

#[test]
fn test_void_method_type_text_is_none() {
    let source = "package com.foo;\npublic class Svc {\n  public void save() {}\n}";
    let result = parse_java(source);
    let method = result.symbols.iter().find(|s| s.kind == SymbolKind::Method).unwrap();
    assert_eq!(method.type_text, None);
}

#[test]
fn test_constructor_type_text_is_none() {
    let source = "package com.foo;\npublic class Svc {\n  public Svc() {}\n}";
    let result = parse_java(source);
    let ctor = result.symbols.iter().find(|s| s.kind == SymbolKind::Constructor).unwrap();
    assert_eq!(ctor.type_text, None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_field_type_text test_method_return_type_text test_void_method_type_text_is_none test_constructor_type_text_is_none -- --nocapture`
Expected: FAIL — `type_text` is always `None`.

- [ ] **Step 3: Implement field `type_text` extraction**

The field type is already extracted by `field_type_name()` and resolved via `resolve_type_name()` in `extract_body_relationships()`. We need to also set it on the symbol.

Modify `extract_field` to accept `import_map` and `package`, and populate `type_text`:

```rust
fn extract_field(
    node: tree_sitter::Node,
    source: &[u8],
    parent_qualified_name: &str,
    package: &str,
    parent_local_id: usize,
    local_id: usize,
    import_map: &HashMap<String, String>,
) -> Option<ExtractedSymbol> {
    let mut name = None;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            name = child
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
                .map(|s| s.to_string());
            break;
        }
    }
    let name = name?;

    let type_text = field_type_name(node, source)
        .map(|t| resolve_type_name(&t, import_map, package));

    let visibility = extract_visibility(node, source);
    let start = node.start_position();
    let end = node.end_position();
    let qualified_name = format!("{}.{}", parent_qualified_name, name);

    Some(ExtractedSymbol {
        local_id,
        name,
        signature: None,
        qualified_name,
        kind: SymbolKind::Field,
        visibility,
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: Some(parent_local_id),
        package: package.to_string(),
        type_text,
    })
}
```

- [ ] **Step 4: Implement method return type extraction**

Add a helper function to extract the return type from a method declaration:

```rust
fn method_return_type(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    // method_declaration has a "type" field for the return type
    // For void methods, the type is "void_type"
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "void_type" => return None,
            "type_identifier" | "generic_type" | "array_type" => {
                return child.utf8_text(source).ok().map(|s| {
                    let base = s.split('<').next().unwrap_or(s);
                    base.to_string()
                });
            }
            // Primitive types (int, boolean, etc.) — not useful for resolution
            "integral_type" | "floating_point_type" | "boolean_type" => return None,
            _ => continue,
        }
    }
    None
}
```

Modify `extract_method` to accept `import_map` and `package`, and populate `type_text`:

```rust
fn extract_method(
    node: tree_sitter::Node,
    source: &[u8],
    parent_qualified_name: &str,
    package: &str,
    parent_local_id: usize,
    local_id: usize,
    kind: SymbolKind,
    import_map: &HashMap<String, String>,
) -> Option<ExtractedSymbol> {
    let name = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())?;

    let visibility = extract_visibility(node, source);
    let start = node.start_position();
    let end = node.end_position();

    let param_types = {
        let mut result = String::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "formal_parameters" {
                result = extract_formal_params(child, source);
                break;
            }
        }
        result
    };

    let type_text = if kind == SymbolKind::Constructor {
        None
    } else {
        method_return_type(node, source)
            .map(|t| resolve_type_name(&t, import_map, package))
    };

    let signature = format!("{}({})", name, param_types);
    let qualified_name = format!("{}.{}", parent_qualified_name, signature);

    Some(ExtractedSymbol {
        local_id,
        name,
        signature: Some(signature),
        qualified_name,
        kind,
        visibility,
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: Some(parent_local_id),
        package: package.to_string(),
        type_text,
    })
}
```

- [ ] **Step 5: Update `extract_members` to pass `import_map` and `package` to `extract_method` and `extract_field`**

`extract_members` (line ~198) needs to accept and forward `import_map` and `package`:

```rust
fn extract_members(
    type_node: tree_sitter::Node,
    source: &[u8],
    parent_qualified_name: &str,
    package: &str,
    parent_local_id: usize,
    symbols: &mut Vec<ExtractedSymbol>,
    import_map: &HashMap<String, String>,
) {
    // ... existing body_kind logic ...
    let maybe_symbol = match member.kind() {
        "method_declaration" => extract_method(member, source, parent_qualified_name, package, parent_local_id, local_id, SymbolKind::Method, import_map),
        "constructor_declaration" => extract_method(member, source, parent_qualified_name, package, parent_local_id, local_id, SymbolKind::Constructor, import_map),
        "field_declaration" => extract_field(member, source, parent_qualified_name, package, parent_local_id, local_id, import_map),
        _ => None,
    };
    // ...
}
```

- [ ] **Step 6: Update `extract_symbols` to parse imports before extracting members**

In `extract_symbols` (line ~25), move `parse_imports` before the member extraction loop so `import_map` is available:

```rust
fn extract_symbols(
    &self,
    tree: &tree_sitter::Tree,
    source: &[u8],
    _file_path: &Path,
) -> ExtractionResult {
    let root = tree.root_node();
    let mut symbols = Vec::new();
    let package = find_package(root, source);
    let (import_map, wildcard_imports) = parse_imports(root, source);

    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if let Some(type_symbol) = extract_type_declaration(child, source, &package, symbols.len()) {
            let type_local_id = type_symbol.local_id;
            let type_qualified_name = type_symbol.qualified_name.clone();
            let type_package = type_symbol.package.clone();
            symbols.push(type_symbol);
            extract_members(child, source, &type_qualified_name, &type_package, type_local_id, &mut symbols, &import_map);
        }
    }

    // ... rest of relationship extraction unchanged ...
}
```

- [ ] **Step 7: Run tests to verify `type_text` tests pass**

Run: `cargo test`
Expected: All tests pass, including the 4 new `type_text` tests.

---

### Task 4: Scope map construction and receiver-resolved method calls

**Files:**
- Modify: `src/plugin/java/mod.rs:445-539` (extract_body_relationships, extract_method_calls, collect_method_invocations)

- [ ] **Step 1: Write failing tests for receiver-resolved method calls**

Add to `src/plugin/java/mod.rs` `mod tests`:

```rust
#[test]
fn test_field_receiver_resolution() {
    let source = "package com.foo;\nimport com.bar.Repository;\npublic class Svc {\n  private Repository repo;\n  void work() { repo.save(); }\n}";
    let result = parse_java(source);
    let calls: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::Calls).collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].target_qualified_name, "com.bar.Repository.save");
}

#[test]
fn test_param_receiver_resolution() {
    let source = "package com.foo;\nimport com.bar.Repository;\npublic class Svc {\n  void work(Repository r) { r.save(); }\n}";
    let result = parse_java(source);
    let calls: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::Calls).collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].target_qualified_name, "com.bar.Repository.save");
}

#[test]
fn test_this_receiver_resolution() {
    let source = "package com.foo;\npublic class Svc {\n  void work() { this.save(); }\n  void save() {}\n}";
    let result = parse_java(source);
    let calls: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::Calls).collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].target_qualified_name, "com.foo.Svc.save");
}

#[test]
fn test_unqualified_call_resolution() {
    let source = "package com.foo;\npublic class Svc {\n  void work() { save(); }\n  void save() {}\n}";
    let result = parse_java(source);
    let calls: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::Calls).collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].target_qualified_name, "com.foo.Svc.save");
}

#[test]
fn test_unresolved_receiver_stays_simple() {
    let source = "package com.foo;\npublic class Svc {\n  void work() { unknown.save(); }\n}";
    let result = parse_java(source);
    let calls: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::Calls).collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].target_qualified_name, "save");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_field_receiver_resolution test_param_receiver_resolution test_this_receiver_resolution test_unqualified_call_resolution test_unresolved_receiver_stays_simple -- --nocapture`
Expected: FAIL — all currently emit simple name `"save"`.

- [ ] **Step 3: Build scope map and resolve receivers**

Replace `extract_method_calls` and `collect_method_invocations` with scope-aware versions. Also update `extract_body_relationships` to build the scope map and pass it through.

First, add a helper to extract parameter names and types from a method node:

```rust
fn extract_method_params(method_node: tree_sitter::Node, source: &[u8], import_map: &HashMap<String, String>, package: &str) -> Vec<(String, String)> {
    let mut params = Vec::new();
    let mut cursor = method_node.walk();
    for child in method_node.children(&mut cursor) {
        if child.kind() == "formal_parameters" {
            let mut param_cursor = child.walk();
            for param in child.children(&mut param_cursor) {
                if param.kind() == "formal_parameter" || param.kind() == "spread_parameter" {
                    let mut type_name = None;
                    let mut param_name = None;
                    let mut inner = param.walk();
                    for pc in param.children(&mut inner) {
                        match pc.kind() {
                            "modifiers" => continue,
                            "identifier" => {
                                param_name = pc.utf8_text(source).ok().map(|s| s.to_string());
                            }
                            _ if pc.is_named() && pc.kind() != "identifier" => {
                                if type_name.is_none() {
                                    type_name = pc.utf8_text(source).ok().map(|s| {
                                        let base = s.split('<').next().unwrap_or(s);
                                        base.to_string()
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                    if let (Some(t), Some(n)) = (type_name, param_name) {
                        let resolved = resolve_type_name(&t, import_map, package);
                        params.push((n, resolved));
                    }
                }
            }
        }
    }
    params
}
```

Now update `extract_body_relationships` to build a scope map per method and pass it to `collect_method_invocations`:

```rust
fn extract_body_relationships(
    body_node: tree_sitter::Node,
    source: &[u8],
    type_local_id: usize,
    type_qualified_name: &str,
    symbols: &[ExtractedSymbol],
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
    package: &str,
) {
    // Build field scope: name -> qualified type
    let mut field_scope: HashMap<String, String> = HashMap::new();
    for sym in symbols.iter().filter(|s| s.kind == SymbolKind::Field && s.parent_local_id == Some(type_local_id)) {
        if let Some(ref tt) = sym.type_text {
            field_scope.insert(sym.name.clone(), tt.clone());
        }
    }

    let mut cursor = body_node.walk();
    for member in body_node.children(&mut cursor) {
        match member.kind() {
            "field_declaration" => {
                let field_local_id = symbols.iter()
                    .find(|s| s.kind == SymbolKind::Field && s.parent_local_id == Some(type_local_id)
                        && s.line == (member.start_position().row + 1) as i64)
                    .map(|s| s.local_id)
                    .unwrap_or(type_local_id);

                if let Some(type_name) = field_type_name(member, source) {
                    let resolved = resolve_type_name(&type_name, import_map, package);
                    relationships.push(ExtractedRelationship {
                        source_local_id: field_local_id,
                        target_qualified_name: resolved,
                        kind: RelationshipKind::FieldType,
                    });
                }
            }
            "method_declaration" | "constructor_declaration" => {
                let method_local_id = symbols.iter()
                    .find(|s| (s.kind == SymbolKind::Method || s.kind == SymbolKind::Constructor)
                        && s.parent_local_id == Some(type_local_id)
                        && s.line == (member.start_position().row + 1) as i64)
                    .map(|s| s.local_id)
                    .unwrap_or(type_local_id);

                // Build scope for this method: fields + params + this
                let mut scope = field_scope.clone();
                for (name, qtype) in extract_method_params(member, source, import_map, package) {
                    scope.insert(name, qtype);
                }
                scope.insert("this".to_string(), type_qualified_name.to_string());

                collect_method_invocations(member, source, method_local_id, relationships, &scope, type_qualified_name);
            }
            _ => {}
        }
    }
}
```

- [ ] **Step 4: Rewrite `collect_method_invocations` with receiver resolution**

```rust
fn collect_method_invocations(
    node: tree_sitter::Node,
    source: &[u8],
    method_local_id: usize,
    relationships: &mut Vec<ExtractedRelationship>,
    scope: &HashMap<String, String>,
    enclosing_class: &str,
) {
    if node.kind() == "method_invocation" {
        if let Some(name_node) = node.child_by_field_name("name") {
            if let Ok(method_name) = name_node.utf8_text(source) {
                // Check for receiver (the "object" field in tree-sitter Java grammar)
                let target = if let Some(obj_node) = node.child_by_field_name("object") {
                    if let Ok(receiver) = obj_node.utf8_text(source) {
                        if receiver == "this" {
                            // this.method() -> EnclosingClass.method
                            format!("{}.{}", enclosing_class, method_name)
                        } else if let Some(receiver_type) = scope.get(receiver) {
                            // receiver is a known field/param -> ReceiverType.method
                            format!("{}.{}", receiver_type, method_name)
                        } else {
                            // Unknown receiver -> simple name fallback
                            method_name.to_string()
                        }
                    } else {
                        method_name.to_string()
                    }
                } else {
                    // No receiver (unqualified call) -> EnclosingClass.method
                    format!("{}.{}", enclosing_class, method_name)
                };

                relationships.push(ExtractedRelationship {
                    source_local_id: method_local_id,
                    target_qualified_name: target,
                    kind: RelationshipKind::Calls,
                });
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_method_invocations(child, source, method_local_id, relationships, scope, enclosing_class);
    }
}
```

- [ ] **Step 5: Update `extract_type_relationships` to pass `type_qualified_name` to `extract_body_relationships`**

The `extract_type_relationships` function (line ~350) needs to pass the type's qualified name:

```rust
"class_body" | "interface_body" => {
    let type_qn = symbols.iter()
        .find(|s| s.local_id == type_local_id)
        .map(|s| s.qualified_name.as_str())
        .unwrap_or("");
    extract_body_relationships(child, source, type_local_id, type_qn, symbols, relationships, import_map, package);
}
```

- [ ] **Step 6: Remove the now-unused `extract_method_calls` function**

Delete the `extract_method_calls` wrapper function (line ~509-516) since `collect_method_invocations` is now called directly from `extract_body_relationships`.

- [ ] **Step 7: Update existing test `test_method_calls_stay_simple`**

The test `test_method_calls_stay_simple` (line ~773) was asserting that method calls stay as simple names. Now with receiver resolution, calls on fields/params with known types will be resolved. Update this test:

```rust
#[test]
fn test_method_calls_with_import_resolved_receiver() {
    let source = "package com.bar;\nimport com.foo.Repository;\npublic class Svc {\n  private Repository repo;\n  void work() { repo.save(); }\n}";
    let result = parse_java(source);
    let calls: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::Calls).collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].target_qualified_name, "com.foo.Repository.save");
}
```

- [ ] **Step 8: Run all tests**

Run: `cargo test`
Expected: All tests pass. The existing `test_extract_method_calls` test (line ~672) uses `repo.save(entity)` and `helper.process()` — `repo` and `helper` are not fields of the class in that test, so they'll remain as simple names `"save"` and `"process"`. Verify this test still passes.

---

## Chunk 3: Integration Tests and Cleanup

### Task 5: Integration tests for receiver-resolved callers/callees

**Files:**
- Modify: `tests/integration.rs`

- [ ] **Step 1: Add test helper with cross-package method calls**

Add to `tests/integration.rs`:

```rust
fn setup_method_call_project(dir: &std::path::Path) {
    fs::create_dir_all(dir.join("src/foo")).unwrap();
    fs::write(
        dir.join("src/foo/Repository.java"),
        r#"package com.foo;
public interface Repository {
    void save(Object o);
    Object findById(int id);
}
"#,
    ).unwrap();

    fs::create_dir_all(dir.join("src/bar")).unwrap();
    fs::write(
        dir.join("src/bar/Service.java"),
        r#"package com.bar;
import com.foo.Repository;
public class Service {
    private Repository repo;
    public void doWork() {
        repo.save(null);
    }
}
"#,
    ).unwrap();
}
```

- [ ] **Step 2: Add integration test for `codix callers` with receiver resolution**

```rust
#[test]
fn test_callers_with_receiver_resolution() {
    let tmp = TempDir::new().unwrap();
    setup_method_call_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["callers", "com.foo.Repository.save*"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("doWork"), "doWork should be a caller of Repository.save");
}
```

- [ ] **Step 3: Add integration test for `codix callees` with receiver resolution**

```rust
#[test]
fn test_callees_with_receiver_resolution() {
    let tmp = TempDir::new().unwrap();
    setup_method_call_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["callees", "com.bar.Service.doWork*"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("save"), "save should be a callee of doWork");
}
```

- [ ] **Step 4: Run all tests (unit + integration)**

Run: `cargo test`
Expected: All tests pass.

---

### Task 6: Update TODO.md

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Check off completed items and update**

Mark these items as done in `TODO.md`:

```markdown
- [x] Improve method call resolution beyond simple name matching — use field types and imports to narrow down which method is being called
- [x] Track receiver type for method invocations (e.g. `repo.save()` → resolve `repo` field type to find `Repository.save()`)
```

Under "Index Quality", check off:
```markdown
- [x] Extract return types and parameter types as relationships
```

(We're extracting return types as `type_text` on symbols, and parameter types are used in the scope map during extraction.)

---

### Task 7: Final commit

- [ ] **Step 1: Run full test suite one final time**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 2: Commit all changes**

```bash
git add src/model.rs src/store/sqlite.rs src/plugin/java/mod.rs tests/integration.rs TODO.md docs/superpowers/specs/2026-03-10-method-call-resolution-design.md docs/superpowers/plans/2026-03-10-method-call-resolution.md
git commit -m "feat: method call resolution with receiver type inference

Resolve method call targets to ClassName.methodName keys using
scope-based receiver type inference (fields, params, this).
Resolution happens at query time via method key matching.

- Add type_text column to symbols (field types, return types)
- Build per-method scope map during extraction
- Resolve receivers via scope map + imports
- Rewrite find_callers/find_callees with method key matching
- Add idx_relationships_target_qname_kind index"
```
