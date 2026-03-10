# Inner/Nested Classes and Annotation Usage Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract inner/nested classes as symbols with proper parent linkage and qualified names, and track annotation usage on symbols as `AnnotatedBy` relationships.

**Architecture:** The Java plugin's `extract_members` becomes recursive to handle nested type declarations. A new `extract_annotations` helper emits `AnnotatedBy` relationships for `marker_annotation` and `annotation` tree-sitter nodes. The core adds `AnnotatedBy` to `RelationshipKind` — no schema changes needed.

**Tech Stack:** Rust, tree-sitter (Java grammar), SQLite (rusqlite)

---

## File Structure

- Modify: `src/model.rs` — add `AnnotatedBy` to `RelationshipKind`
- Modify: `src/plugin/java/mod.rs` — recursive `extract_members`, annotation extraction, recursive relationship extraction
- Modify: `tests/integration.rs` — integration tests for inner classes and annotations
- Modify: `TODO.md` — check off completed items

---

## Chunk 1: Core + Inner Classes

### Task 1: Add `AnnotatedBy` to `RelationshipKind`

**Files:**
- Modify: `src/model.rs:78-96`

- [ ] **Step 1: Add `AnnotatedBy` variant and its `as_str()` arm**

In `src/model.rs`, add `AnnotatedBy` after `FieldType` in the enum, and add the match arm in `as_str()`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelationshipKind {
    Extends,
    Implements,
    Calls,
    FieldType,
    AnnotatedBy,
}

impl RelationshipKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RelationshipKind::Extends => "extends",
            RelationshipKind::Implements => "implements",
            RelationshipKind::Calls => "calls",
            RelationshipKind::FieldType => "field-type",
            RelationshipKind::AnnotatedBy => "annotated-by",
        }
    }
}
```

- [ ] **Step 2: Run tests to verify nothing breaks**

Run: `cargo test`
Expected: All 88 tests pass. `AnnotatedBy` is just a new variant, no existing code uses it yet.

---

### Task 2: Recursive `extract_members` for inner classes

**Files:**
- Modify: `src/plugin/java/mod.rs:195-230` (extract_members)

- [ ] **Step 1: Write failing tests for inner class extraction**

Add to `src/plugin/java/mod.rs` `mod tests`:

```rust
#[test]
fn test_inner_class_extracted() {
    let source = "package com.foo;\npublic class Outer {\n  public static class Inner {}\n}";
    let result = parse_java(source);
    assert_eq!(result.symbols.len(), 2);
    let inner = result.symbols.iter().find(|s| s.name == "Inner").unwrap();
    assert_eq!(inner.kind, SymbolKind::Class);
    assert_eq!(inner.qualified_name, "com.foo.Outer.Inner");
    assert_eq!(inner.parent_local_id, Some(0));
}

#[test]
fn test_deeply_nested_class() {
    let source = "package com.foo;\npublic class A {\n  class B {\n    class C {}\n  }\n}";
    let result = parse_java(source);
    assert_eq!(result.symbols.len(), 3);
    let b = result.symbols.iter().find(|s| s.name == "B").unwrap();
    assert_eq!(b.qualified_name, "com.foo.A.B");
    let c = result.symbols.iter().find(|s| s.name == "C").unwrap();
    assert_eq!(c.qualified_name, "com.foo.A.B.C");
}

#[test]
fn test_inner_class_members() {
    let source = "package com.foo;\npublic class Outer {\n  public static class Inner {\n    private String name;\n    public void doWork() {}\n  }\n}";
    let result = parse_java(source);
    let inner = result.symbols.iter().find(|s| s.name == "Inner").unwrap();
    let inner_id = inner.local_id;
    let field = result.symbols.iter().find(|s| s.name == "name").unwrap();
    assert_eq!(field.parent_local_id, Some(inner_id));
    let method = result.symbols.iter().find(|s| s.name == "doWork").unwrap();
    assert_eq!(method.parent_local_id, Some(inner_id));
    assert_eq!(method.qualified_name, "com.foo.Outer.Inner.doWork()");
}

#[test]
fn test_inner_interface_extracted() {
    let source = "package com.foo;\npublic class Outer {\n  public interface Callback {\n    void onComplete();\n  }\n}";
    let result = parse_java(source);
    let cb = result.symbols.iter().find(|s| s.name == "Callback").unwrap();
    assert_eq!(cb.kind, SymbolKind::Interface);
    assert_eq!(cb.qualified_name, "com.foo.Outer.Callback");
    let method = result.symbols.iter().find(|s| s.name == "onComplete").unwrap();
    assert_eq!(method.parent_local_id, Some(cb.local_id));
}

#[test]
fn test_inner_enum_extracted() {
    let source = "package com.foo;\npublic class Outer {\n  public enum Status { ACTIVE, INACTIVE }\n}";
    let result = parse_java(source);
    let status = result.symbols.iter().find(|s| s.name == "Status").unwrap();
    assert_eq!(status.kind, SymbolKind::Enum);
    assert_eq!(status.qualified_name, "com.foo.Outer.Status");
    assert_eq!(status.parent_local_id, Some(0));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_inner_class_extracted test_deeply_nested_class test_inner_class_members test_inner_interface_extracted test_inner_enum_extracted -- --nocapture`
Expected: FAIL — inner classes are not extracted (symbol counts wrong).

- [ ] **Step 3: Make `extract_members` recursive**

Replace `extract_members` in `src/plugin/java/mod.rs:195-230`:

```rust
fn extract_members(
    type_node: tree_sitter::Node,
    source: &[u8],
    parent_qualified_name: &str,
    package: &str,
    parent_local_id: usize,
    import_map: &HashMap<String, String>,
    symbols: &mut Vec<ExtractedSymbol>,
) {
    let body_kind = match type_node.kind() {
        "class_declaration" | "record_declaration" => "class_body",
        "interface_declaration" => "interface_body",
        "enum_declaration" => "enum_body",
        "annotation_type_declaration" => "annotation_type_body",
        _ => return,
    };

    let mut cursor = type_node.walk();
    for child in type_node.children(&mut cursor) {
        if child.kind() == body_kind {
            let mut body_cursor = child.walk();
            for member in child.children(&mut body_cursor) {
                let local_id = symbols.len();
                match member.kind() {
                    "method_declaration" => {
                        if let Some(symbol) = extract_method(member, source, parent_qualified_name, package, parent_local_id, local_id, SymbolKind::Method, import_map) {
                            symbols.push(symbol);
                        }
                    }
                    "constructor_declaration" => {
                        if let Some(symbol) = extract_method(member, source, parent_qualified_name, package, parent_local_id, local_id, SymbolKind::Constructor, import_map) {
                            symbols.push(symbol);
                        }
                    }
                    "field_declaration" => {
                        if let Some(symbol) = extract_field(member, source, parent_qualified_name, package, parent_local_id, local_id, import_map) {
                            symbols.push(symbol);
                        }
                    }
                    "class_declaration" | "interface_declaration" | "enum_declaration"
                    | "record_declaration" | "annotation_type_declaration" => {
                        // Nested type declaration — extract as symbol and recurse
                        if let Some(mut nested_symbol) = extract_type_declaration(member, source, package, local_id) {
                            // Override qualified_name to nest under parent
                            let name = nested_symbol.name.clone();
                            nested_symbol.qualified_name = format!("{}.{}", parent_qualified_name, name);
                            nested_symbol.parent_local_id = Some(parent_local_id);
                            let nested_local_id = nested_symbol.local_id;
                            let nested_qn = nested_symbol.qualified_name.clone();
                            symbols.push(nested_symbol);
                            // Recurse into the nested type's members
                            extract_members(member, source, &nested_qn, package, nested_local_id, import_map, symbols);
                        }
                    }
                    _ => {}
                }
            }
            break;
        }
    }
}
```

- [ ] **Step 4: Run tests to verify inner class tests pass**

Run: `cargo test`
Expected: All tests pass, including the 5 new inner class tests.

---

### Task 3: Recursive relationship extraction for inner classes

**Files:**
- Modify: `src/plugin/java/mod.rs:51-69` (relationship extraction loop in `extract_symbols`)
- Modify: `src/plugin/java/mod.rs:361-401` (extract_type_relationships)
- Modify: `src/plugin/java/mod.rs:460-517` (extract_body_relationships)

- [ ] **Step 1: Write failing test for inner class relationships**

Add to `src/plugin/java/mod.rs` `mod tests`:

```rust
#[test]
fn test_inner_class_extends_relationship() {
    let source = "package com.foo;\nimport com.bar.Base;\npublic class Outer {\n  public static class Inner extends Base {\n    private String name;\n  }\n}";
    let result = parse_java(source);
    let extends: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::Extends).collect();
    assert_eq!(extends.len(), 1);
    assert_eq!(extends[0].target_qualified_name, "com.bar.Base");
    let inner = result.symbols.iter().find(|s| s.name == "Inner").unwrap();
    assert_eq!(extends[0].source_local_id, inner.local_id);
}

#[test]
fn test_inner_class_field_type_relationship() {
    let source = "package com.foo;\nimport com.bar.Repo;\npublic class Outer {\n  public static class Inner {\n    private Repo repo;\n  }\n}";
    let result = parse_java(source);
    let field_types: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::FieldType).collect();
    assert_eq!(field_types.len(), 1);
    assert_eq!(field_types[0].target_qualified_name, "com.bar.Repo");
}

#[test]
fn test_inner_class_method_calls() {
    let source = "package com.foo;\nimport com.bar.Repo;\npublic class Outer {\n  public static class Inner {\n    private Repo repo;\n    void work() { repo.save(); }\n  }\n}";
    let result = parse_java(source);
    let calls: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::Calls).collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].target_qualified_name, "com.bar.Repo.save");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_inner_class_extends_relationship test_inner_class_field_type_relationship test_inner_class_method_calls -- --nocapture`
Expected: FAIL — relationships for inner classes are not extracted.

- [ ] **Step 3: Make relationship extraction recursive**

The top-level relationship extraction loop in `extract_symbols` (lines 51-69) only processes root-level type declarations. To handle inner classes, make `extract_type_relationships` recurse into nested types found inside class/interface bodies.

Add handling for nested type declarations in `extract_body_relationships` (line 514, the `_ => {}` catch-all). When a body member is a type declaration, look up its local_id from `symbols` and call `extract_type_relationships` recursively:

In `extract_body_relationships`, replace the `_ => {}` catch-all (line 514) with:

```rust
            "class_declaration" | "interface_declaration" | "enum_declaration"
            | "record_declaration" | "annotation_type_declaration" => {
                // Recurse into nested type declarations for their relationships
                let nested_local_id = member.child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source).ok())
                    .and_then(|name| {
                        symbols.iter().find(|s| s.name == name && s.parent_local_id == Some(type_local_id)).map(|s| s.local_id)
                    });
                if let Some(nested_id) = nested_local_id {
                    extract_type_relationships(member, source, nested_id, symbols, relationships, import_map, package);
                }
            }
            _ => {}
```

Also add `"enum_body" | "annotation_type_body"` to the body kinds handled in `extract_type_relationships` (line 392):

```rust
            "class_body" | "interface_body" | "enum_body" | "annotation_type_body" => {
```

- [ ] **Step 4: Run tests to verify all pass**

Run: `cargo test`
Expected: All tests pass, including the 3 new inner class relationship tests.

---

## Chunk 2: Annotation Extraction

### Task 4: Extract annotation usage as `AnnotatedBy` relationships

**Files:**
- Modify: `src/plugin/java/mod.rs`

- [ ] **Step 1: Write failing tests for annotation extraction**

Add to `src/plugin/java/mod.rs` `mod tests`:

```rust
#[test]
fn test_annotation_on_class() {
    let source = "package com.foo;\nimport com.bar.Deprecated;\n@Deprecated\npublic class Svc {}";
    let result = parse_java(source);
    let annots: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::AnnotatedBy).collect();
    assert_eq!(annots.len(), 1);
    assert_eq!(annots[0].source_local_id, 0);
    assert_eq!(annots[0].target_qualified_name, "com.bar.Deprecated");
}

#[test]
fn test_annotation_on_method() {
    let source = "package com.foo;\npublic class Svc {\n  @Override\n  public void save() {}\n}";
    let result = parse_java(source);
    let annots: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::AnnotatedBy).collect();
    assert_eq!(annots.len(), 1);
    assert_eq!(annots[0].target_qualified_name, "com.foo.Override");
}

#[test]
fn test_annotation_on_constructor() {
    let source = "package com.foo;\nimport com.bar.Inject;\npublic class Svc {\n  @Inject\n  public Svc() {}\n}";
    let result = parse_java(source);
    let annots: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::AnnotatedBy).collect();
    assert_eq!(annots.len(), 1);
    assert_eq!(annots[0].target_qualified_name, "com.bar.Inject");
}

#[test]
fn test_annotation_on_field() {
    let source = "package com.foo;\nimport com.bar.Inject;\npublic class Svc {\n  @Inject\n  private String name;\n}";
    let result = parse_java(source);
    let annots: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::AnnotatedBy).collect();
    assert_eq!(annots.len(), 1);
    assert_eq!(annots[0].target_qualified_name, "com.bar.Inject");
}

#[test]
fn test_annotation_with_args() {
    let source = "package com.foo;\nimport com.bar.SuppressWarnings;\npublic class Svc {\n  @SuppressWarnings(\"unchecked\")\n  public void save() {}\n}";
    let result = parse_java(source);
    let annots: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::AnnotatedBy).collect();
    assert_eq!(annots.len(), 1);
    assert_eq!(annots[0].target_qualified_name, "com.bar.SuppressWarnings");
}

#[test]
fn test_multiple_annotations() {
    let source = "package com.foo;\npublic class Svc {\n  @Override\n  @Deprecated\n  public void save() {}\n}";
    let result = parse_java(source);
    let annots: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::AnnotatedBy).collect();
    assert_eq!(annots.len(), 2);
}

#[test]
fn test_annotation_on_param_attached_to_method() {
    let source = "package com.foo;\nimport com.bar.NotNull;\npublic class Svc {\n  public void save(@NotNull String s) {}\n}";
    let result = parse_java(source);
    let annots: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::AnnotatedBy).collect();
    assert_eq!(annots.len(), 1);
    let method = result.symbols.iter().find(|s| s.kind == SymbolKind::Method).unwrap();
    assert_eq!(annots[0].source_local_id, method.local_id);
    assert_eq!(annots[0].target_qualified_name, "com.bar.NotNull");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_annotation_on_class test_annotation_on_method test_annotation_on_constructor test_annotation_on_field test_annotation_with_args test_multiple_annotations test_annotation_on_param_attached_to_method -- --nocapture`
Expected: FAIL — no `AnnotatedBy` relationships are emitted.

- [ ] **Step 3: Add `extract_annotations` helper**

Add a new function to `src/plugin/java/mod.rs`:

```rust
fn extract_annotations(
    node: tree_sitter::Node,
    source: &[u8],
    source_local_id: usize,
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
    package: &str,
) {
    // Annotations can be inside a "modifiers" node or directly on the declaration
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "modifiers" {
            let mut mod_cursor = child.walk();
            for mod_child in child.children(&mut mod_cursor) {
                if mod_child.kind() == "marker_annotation" || mod_child.kind() == "annotation" {
                    if let Some(name) = annotation_name(mod_child, source) {
                        let resolved = resolve_type_name(&name, import_map, package);
                        relationships.push(ExtractedRelationship {
                            source_local_id,
                            target_qualified_name: resolved,
                            kind: RelationshipKind::AnnotatedBy,
                        });
                    }
                }
            }
        }
    }
}

fn annotation_name(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    // The annotation name is an "identifier" or "scoped_identifier" child
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" || child.kind() == "scoped_identifier" {
            return child.utf8_text(source).ok().map(|s| s.to_string());
        }
    }
    None
}
```

- [ ] **Step 4: Call `extract_annotations` for type declarations**

In `extract_type_relationships` (line 361), add annotation extraction at the start of the function, before the child loop:

```rust
fn extract_type_relationships(
    type_node: tree_sitter::Node,
    source: &[u8],
    type_local_id: usize,
    symbols: &[ExtractedSymbol],
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
    package: &str,
) {
    // Extract annotations on the type declaration itself
    extract_annotations(type_node, source, type_local_id, relationships, import_map, package);

    let mut cursor = type_node.walk();
    // ... rest unchanged ...
```

- [ ] **Step 5: Call `extract_annotations` for methods, constructors, and fields**

In `extract_body_relationships` (lines 480-516), add annotation extraction for each member. Also extract parameter annotations for methods/constructors.

In the `"field_declaration"` arm (after finding `field_local_id`), add:

```rust
                extract_annotations(member, source, field_local_id, relationships, import_map, package);
```

In the `"method_declaration" | "constructor_declaration"` arm (after finding `method_local_id`), add:

```rust
                extract_annotations(member, source, method_local_id, relationships, import_map, package);
                // Extract annotations on parameters, attached to the method
                extract_param_annotations(member, source, method_local_id, relationships, import_map, package);
```

Add the `extract_param_annotations` helper:

```rust
fn extract_param_annotations(
    method_node: tree_sitter::Node,
    source: &[u8],
    method_local_id: usize,
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
    package: &str,
) {
    let mut cursor = method_node.walk();
    for child in method_node.children(&mut cursor) {
        if child.kind() == "formal_parameters" {
            let mut param_cursor = child.walk();
            for param in child.children(&mut param_cursor) {
                if param.kind() == "formal_parameter" || param.kind() == "spread_parameter" {
                    extract_annotations(param, source, method_local_id, relationships, import_map, package);
                }
            }
        }
    }
}
```

- [ ] **Step 6: Run all tests**

Run: `cargo test`
Expected: All tests pass, including the 7 new annotation tests.

---

## Chunk 3: Integration Tests and Cleanup

### Task 5: Integration tests

**Files:**
- Modify: `tests/integration.rs`

- [ ] **Step 1: Add integration test for inner classes**

Add to `tests/integration.rs`:

```rust
#[test]
fn test_inner_class_symbols() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("src/Outer.java"),
        r#"package com.foo;
public class Outer {
    public static class Inner {
        public void doWork() {}
    }
    public interface Callback {
        void onComplete();
    }
}
"#,
    ).unwrap();
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["symbols", "src/Outer.java"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Outer"), "should contain Outer class");
    assert!(stdout.contains("Inner"), "should contain Inner class");
    assert!(stdout.contains("Callback"), "should contain Callback interface");
    assert!(stdout.contains("doWork"), "should contain Inner's method");
    assert!(stdout.contains("onComplete"), "should contain Callback's method");
}
```

- [ ] **Step 2: Add integration test for annotation refs**

```rust
#[test]
fn test_annotation_refs() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("src/MyAnnotation.java"),
        r#"package com.foo;
public @interface MyAnnotation {}
"#,
    ).unwrap();
    fs::write(
        tmp.path().join("src/Service.java"),
        r#"package com.foo;
public class Service {
    @MyAnnotation
    public void save() {}
}
"#,
    ).unwrap();
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["refs", "com.foo.MyAnnotation"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("save"), "save should reference MyAnnotation via @MyAnnotation");
}
```

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: All tests pass.

---

### Task 6: Update TODO.md and commit

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Check off completed items**

In `TODO.md`, mark as done:

```markdown
- [x] Handle inner/nested classes (currently only top-level types are parents)
- [x] Track annotation usage as relationships
```

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Commit all changes**

```bash
git add src/model.rs src/plugin/java/mod.rs tests/integration.rs TODO.md
git commit -m "feat: inner/nested class extraction and annotation usage tracking

- Make extract_members recursive for nested type declarations
- Add AnnotatedBy relationship kind for @annotation usage
- Extract annotations on types, methods, constructors, fields, parameters
- Recurse relationship extraction into nested types"
```
