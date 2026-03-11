# Python Plugin Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Python language plugin to codix that extracts classes, functions, methods, fields, and their relationships from `.py`/`.pyi` files.

**Architecture:** Single plugin file implementing `LanguagePlugin` trait, following the same pattern as the Go plugin. Tree-sitter-python parses Python source; the plugin walks the AST to extract symbols and relationships. Decorators are tracked via `AnnotatedBy`, inheritance via `Extends`, type hints via `FieldType`.

**Tech Stack:** Rust, tree-sitter, tree-sitter-python 0.23

**Spec:** `docs/superpowers/specs/2026-03-11-python-plugin-design.md`

---

## Chunk 1: Plugin scaffold and class extraction

### Task 1: Add tree-sitter-python dependency and plugin scaffold

**Files:**
- Modify: `Cargo.toml`
- Create: `src/plugin/python/mod.rs`
- Modify: `src/plugin/mod.rs`

- [ ] **Step 1: Add tree-sitter-python dependency**

In `Cargo.toml`, add to `[dependencies]`:
```toml
tree-sitter-python = "0.23"
```

- [ ] **Step 2: Create minimal plugin file**

Create `src/plugin/python/mod.rs`:
```rust
use std::path::Path;
use crate::model::*;

pub struct PythonPlugin;

impl super::LanguagePlugin for PythonPlugin {
    fn name(&self) -> &str { "python" }
    fn display_name(&self) -> &str { "Python" }

    fn can_handle(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e == "py" || e == "pyi")
            .unwrap_or(false)
    }

    fn tree_sitter_language(&self) -> tree_sitter::Language {
        tree_sitter_python::LANGUAGE.into()
    }

    fn extract_symbols(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        _file_path: &Path,
    ) -> ExtractionResult {
        let mut symbols = Vec::new();
        let mut relationships = Vec::new();

        let root = tree.root_node();
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            extract_top_level(child, source, &mut symbols, &mut relationships);
        }

        ExtractionResult {
            symbols,
            relationships,
            wildcard_imports: Vec::new(),
        }
    }
}

fn extract_top_level(
    node: tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    match node.kind() {
        "class_definition" => {
            extract_class(node, source, None, "", symbols, relationships);
        }
        "function_definition" => {
            extract_function(node, source, None, "", symbols, relationships);
        }
        "decorated_definition" => {
            extract_decorated(node, source, None, "", symbols, relationships);
        }
        _ => {}
    }
}

fn extract_decorated(
    node: tree_sitter::Node,
    source: &[u8],
    parent_local_id: Option<usize>,
    parent_name: &str,
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    // Collect decorator names
    let mut decorators = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "decorator" {
            if let Some(name) = extract_decorator_name(child, source) {
                decorators.push(name);
            }
        }
    }

    // Extract the underlying definition
    if let Some(def) = node.child_by_field_name("definition") {
        let pre_len = symbols.len();
        match def.kind() {
            "class_definition" => {
                extract_class(def, source, parent_local_id, parent_name, symbols, relationships);
            }
            "function_definition" => {
                extract_function(def, source, parent_local_id, parent_name, symbols, relationships);
            }
            _ => {}
        }
        // Add AnnotatedBy relationships for decorators
        if symbols.len() > pre_len {
            let symbol_local_id = symbols[pre_len].local_id;
            for decorator_name in decorators {
                relationships.push(ExtractedRelationship {
                    source_local_id: symbol_local_id,
                    target_qualified_name: decorator_name,
                    kind: RelationshipKind::AnnotatedBy,
                });
            }
        }
    }
}

fn extract_decorator_name(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    // Decorator children: "@" then an expression
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "identifier" => return child.utf8_text(source).ok().map(|s| s.to_string()),
            "attribute" => {
                // e.g. @app.route — extract the full dotted name
                return child.utf8_text(source).ok().map(|s| s.to_string());
            }
            "call" => {
                // e.g. @dataclass(frozen=True) — extract function name
                if let Some(func) = child.child_by_field_name("function") {
                    return func.utf8_text(source).ok().map(|s| s.to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn visibility_from_name(name: &str) -> Visibility {
    if name.starts_with("__") && !name.ends_with("__") {
        Visibility::new("private")
    } else if name.starts_with('_') {
        Visibility::new("protected")
    } else {
        Visibility::new("public")
    }
}

fn extract_class(
    node: tree_sitter::Node,
    source: &[u8],
    parent_local_id: Option<usize>,
    parent_name: &str,
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    let name = match node.child_by_field_name("name") {
        Some(n) => match n.utf8_text(source) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        },
        None => return,
    };

    let qualified_name = if parent_name.is_empty() {
        name.clone()
    } else {
        format!("{}.{}", parent_name, name)
    };

    let start = node.start_position();
    let end = node.end_position();
    let local_id = symbols.len();

    symbols.push(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: None,
        qualified_name: qualified_name.clone(),
        kind: SymbolKind::new("class"),
        visibility: visibility_from_name(&name),
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id,
        package: String::new(),
        type_text: None,
    });

    // Extract base classes → Extends relationships
    if let Some(superclasses) = node.child_by_field_name("superclasses") {
        let mut sc_cursor = superclasses.walk();
        for child in superclasses.children(&mut sc_cursor) {
            match child.kind() {
                "identifier" => {
                    if let Ok(base) = child.utf8_text(source) {
                        relationships.push(ExtractedRelationship {
                            source_local_id: local_id,
                            target_qualified_name: base.to_string(),
                            kind: RelationshipKind::Extends,
                        });
                    }
                }
                "attribute" => {
                    if let Ok(base) = child.utf8_text(source) {
                        relationships.push(ExtractedRelationship {
                            source_local_id: local_id,
                            target_qualified_name: base.to_string(),
                            kind: RelationshipKind::Extends,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    // Extract class body
    if let Some(body) = node.child_by_field_name("body") {
        let mut body_cursor = body.walk();
        for child in body.children(&mut body_cursor) {
            match child.kind() {
                "function_definition" => {
                    extract_function(child, source, Some(local_id), &qualified_name, symbols, relationships);
                }
                "decorated_definition" => {
                    extract_decorated(child, source, Some(local_id), &qualified_name, symbols, relationships);
                }
                "class_definition" => {
                    extract_class(child, source, Some(local_id), &qualified_name, symbols, relationships);
                }
                "expression_statement" => {
                    // Class-body field: `name: type` or `name = value`
                    let mut ec = child.walk();
                    for expr in child.children(&mut ec) {
                        if expr.kind() == "assignment" {
                            extract_class_field(expr, source, local_id, &qualified_name, symbols, relationships);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn extract_class_field(
    node: tree_sitter::Node,
    source: &[u8],
    parent_local_id: usize,
    parent_name: &str,
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    let left = match node.child_by_field_name("left") {
        Some(l) if l.kind() == "identifier" => match l.utf8_text(source) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        },
        _ => return,
    };

    let type_node = node.child_by_field_name("type");
    let type_text = type_node
        .and_then(|t| t.utf8_text(source).ok())
        .map(|s| s.to_string());

    let start = node.start_position();
    let end = node.end_position();
    let local_id = symbols.len();

    symbols.push(ExtractedSymbol {
        local_id,
        name: left.clone(),
        signature: None,
        qualified_name: format!("{}.{}", parent_name, left),
        kind: SymbolKind::new("field"),
        visibility: visibility_from_name(&left),
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: Some(parent_local_id),
        package: String::new(),
        type_text,
    });

    // FieldType relationship from type annotation
    if let Some(tn) = type_node {
        extract_field_type_relationships(tn, source, local_id, relationships);
    }
}

fn extract_function(
    node: tree_sitter::Node,
    source: &[u8],
    parent_local_id: Option<usize>,
    parent_name: &str,
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    let name = match node.child_by_field_name("name") {
        Some(n) => match n.utf8_text(source) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        },
        None => return,
    };

    let is_method = parent_local_id.is_some() && !parent_name.is_empty();

    let kind = if is_method && name == "__init__" {
        SymbolKind::new("constructor")
    } else if is_method {
        SymbolKind::new("method")
    } else {
        SymbolKind::new("function")
    };

    let qualified_name = if parent_name.is_empty() {
        name.clone()
    } else {
        format!("{}.{}", parent_name, name)
    };

    // Build signature: name(param1,param2) — excluding self and cls
    let signature = build_signature(&name, node, source);

    // Return type annotation for type_text
    let return_type = node.child_by_field_name("return_type");
    let type_text = return_type
        .and_then(|r| r.utf8_text(source).ok())
        .map(|s| s.to_string());

    let start = node.start_position();
    let end = node.end_position();
    let local_id = symbols.len();

    symbols.push(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: Some(signature),
        qualified_name,
        kind,
        visibility: visibility_from_name(&name),
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id,
        package: String::new(),
        type_text,
    });

    // Parameter type relationships
    if let Some(params) = node.child_by_field_name("parameters") {
        extract_param_type_relationships(params, source, local_id, relationships);
    }

    // Return type relationship
    if let Some(rt) = return_type {
        extract_field_type_relationships(rt, source, local_id, relationships);
    }

    // Call relationships
    if let Some(body) = node.child_by_field_name("body") {
        extract_call_relationships(body, source, local_id, relationships);
    }
}

fn build_signature(name: &str, node: tree_sitter::Node, source: &[u8]) -> String {
    let mut params = Vec::new();
    if let Some(param_list) = node.child_by_field_name("parameters") {
        let mut cursor = param_list.walk();
        for child in param_list.children(&mut cursor) {
            match child.kind() {
                "identifier" => {
                    if let Ok(pname) = child.utf8_text(source) {
                        if pname != "self" && pname != "cls" {
                            params.push(pname.to_string());
                        }
                    }
                }
                "typed_parameter" | "default_parameter" | "typed_default_parameter" => {
                    if let Some(pname_node) = child.child_by_field_name("name") {
                        if let Ok(pname) = pname_node.utf8_text(source) {
                            if pname != "self" && pname != "cls" {
                                params.push(pname.to_string());
                            }
                        }
                    }
                }
                "list_splat_pattern" | "dictionary_splat_pattern" => {
                    // *args, **kwargs
                    if let Ok(text) = child.utf8_text(source) {
                        params.push(text.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    format!("{}({})", name, params.join(","))
}

fn extract_param_type_relationships(
    params_node: tree_sitter::Node,
    source: &[u8],
    source_local_id: usize,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    let mut cursor = params_node.walk();
    for child in params_node.children(&mut cursor) {
        match child.kind() {
            "typed_parameter" | "typed_default_parameter" => {
                if let Some(pname_node) = child.child_by_field_name("name") {
                    if let Ok(pname) = pname_node.utf8_text(source) {
                        if pname == "self" || pname == "cls" {
                            continue;
                        }
                    }
                }
                if let Some(type_node) = child.child_by_field_name("type") {
                    extract_field_type_relationships(type_node, source, source_local_id, relationships);
                }
            }
            _ => {}
        }
    }
}

fn extract_field_type_relationships(
    type_node: tree_sitter::Node,
    source: &[u8],
    source_local_id: usize,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    // Walk the type expression to find user type references
    let mut type_names = Vec::new();
    collect_type_names(type_node, source, &mut type_names);
    for type_name in type_names {
        if is_user_type(&type_name) {
            relationships.push(ExtractedRelationship {
                source_local_id,
                target_qualified_name: type_name,
                kind: RelationshipKind::FieldType,
            });
        }
    }
}

fn collect_type_names(node: tree_sitter::Node, source: &[u8], out: &mut Vec<String>) {
    match node.kind() {
        "identifier" => {
            if let Ok(s) = node.utf8_text(source) {
                out.push(s.to_string());
            }
        }
        "attribute" => {
            // dotted name like module.Type
            if let Ok(s) = node.utf8_text(source) {
                out.push(s.to_string());
            }
        }
        "type" => {
            // Wrapper node — recurse into children
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_type_names(child, source, out);
            }
        }
        "generic_type" | "subscript" => {
            // List[int], Dict[str, int] — extract all type refs
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_type_names(child, source, out);
            }
        }
        "binary_operator" => {
            // Union type: X | Y (PEP 604)
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_type_names(child, source, out);
            }
        }
        _ => {}
    }
}

fn is_user_type(name: &str) -> bool {
    let base = name.split('.').last().unwrap_or(name);
    base.starts_with(|c: char| c.is_uppercase())
}

fn extract_call_relationships(
    node: tree_sitter::Node,
    source: &[u8],
    source_local_id: usize,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    collect_calls(node, source, source_local_id, relationships);
}

fn collect_calls(
    node: tree_sitter::Node,
    source: &[u8],
    source_local_id: usize,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    if node.kind() == "call" {
        if let Some(func) = node.child_by_field_name("function") {
            let call_name = match func.kind() {
                "identifier" => func.utf8_text(source).ok().map(|s| s.to_string()),
                "attribute" => {
                    // obj.method() — extract method name (last segment)
                    func.child_by_field_name("attribute")
                        .and_then(|a| a.utf8_text(source).ok())
                        .map(|s| s.to_string())
                }
                _ => None,
            };
            if let Some(name) = call_name {
                if !name.is_empty() {
                    relationships.push(ExtractedRelationship {
                        source_local_id,
                        target_qualified_name: name,
                        kind: RelationshipKind::Calls,
                    });
                }
            }
        }
    }
    // Recurse into children to find nested calls
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(child, source, source_local_id, relationships);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_extract(code: &str) -> ExtractionResult {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_python::LANGUAGE.into()).unwrap();
        let tree = parser.parse(code, None).unwrap();
        let plugin = PythonPlugin;
        plugin.extract_symbols(&tree, code.as_bytes(), std::path::Path::new("test.py"))
    }

    // Tests will be added per task
}
```

- [ ] **Step 3: Register plugin in mod.rs**

In `src/plugin/mod.rs`, add `pub mod python;` after the other module declarations, and register it in `PluginRegistry::new()`:
```rust
registry.register(Box::new(python::PythonPlugin));
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/plugin/python/mod.rs src/plugin/mod.rs
git commit -m "feat: add Python plugin with class, function, method, and field extraction"
```

---

### Task 2: Add unit tests for symbol extraction

**Files:**
- Modify: `src/plugin/python/mod.rs` (add tests in `mod tests`)

- [ ] **Step 1: Add class extraction test**

Add to `mod tests` in `src/plugin/python/mod.rs`:
```rust
#[test]
fn test_class_extraction() {
    let result = parse_and_extract("class Foo:\n    pass\n");
    assert_eq!(result.symbols.len(), 1);
    assert_eq!(result.symbols[0].name, "Foo");
    assert_eq!(result.symbols[0].kind, SymbolKind::new("class"));
    assert_eq!(result.symbols[0].visibility, Visibility::new("public"));
}

#[test]
fn test_nested_class() {
    let result = parse_and_extract("class Outer:\n    class Inner:\n        pass\n");
    assert_eq!(result.symbols.len(), 2);
    assert_eq!(result.symbols[0].name, "Outer");
    assert_eq!(result.symbols[1].name, "Inner");
    assert_eq!(result.symbols[1].qualified_name, "Outer.Inner");
    assert!(result.symbols[1].parent_local_id.is_some());
}
```

- [ ] **Step 2: Add function and method tests**

```rust
#[test]
fn test_function_extraction() {
    let result = parse_and_extract("def foo():\n    pass\n");
    assert_eq!(result.symbols.len(), 1);
    assert_eq!(result.symbols[0].name, "foo");
    assert_eq!(result.symbols[0].kind, SymbolKind::new("function"));
}

#[test]
fn test_method_extraction() {
    let result = parse_and_extract("class Foo:\n    def bar(self):\n        pass\n");
    assert_eq!(result.symbols.len(), 2); // class + method
    assert_eq!(result.symbols[1].name, "bar");
    assert_eq!(result.symbols[1].kind, SymbolKind::new("method"));
    assert_eq!(result.symbols[1].qualified_name, "Foo.bar");
}

#[test]
fn test_constructor() {
    let result = parse_and_extract("class Foo:\n    def __init__(self):\n        pass\n");
    assert_eq!(result.symbols[1].name, "__init__");
    assert_eq!(result.symbols[1].kind, SymbolKind::new("constructor"));
}

#[test]
fn test_signature_excludes_self() {
    let result = parse_and_extract("class Foo:\n    def bar(self, x, y):\n        pass\n");
    assert_eq!(result.symbols[1].signature.as_deref(), Some("bar(x,y)"));
}
```

- [ ] **Step 3: Add field and visibility tests**

```rust
#[test]
fn test_class_field() {
    let result = parse_and_extract("class Foo:\n    name: str\n");
    assert_eq!(result.symbols.len(), 2); // class + field
    assert_eq!(result.symbols[1].name, "name");
    assert_eq!(result.symbols[1].kind, SymbolKind::new("field"));
    assert_eq!(result.symbols[1].type_text.as_deref(), Some("str"));
}

#[test]
fn test_visibility() {
    let result = parse_and_extract("class Foo:\n    def public(self): pass\n    def _protected(self): pass\n    def __private(self): pass\n    def __dunder__(self): pass\n");
    assert_eq!(result.symbols[1].visibility, Visibility::new("public"));
    assert_eq!(result.symbols[2].visibility, Visibility::new("protected"));
    assert_eq!(result.symbols[3].visibility, Visibility::new("private"));
    assert_eq!(result.symbols[4].visibility, Visibility::new("public")); // dunder = public
}
```

- [ ] **Step 4: Add relationship tests**

```rust
#[test]
fn test_extends() {
    let result = parse_and_extract("class Foo(Bar, Baz):\n    pass\n");
    let extends: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::Extends)
        .collect();
    assert_eq!(extends.len(), 2);
    assert_eq!(extends[0].target_qualified_name, "Bar");
    assert_eq!(extends[1].target_qualified_name, "Baz");
}

#[test]
fn test_field_type_relationship() {
    let result = parse_and_extract("class Foo:\n    repo: Repository\n");
    let field_types: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::FieldType)
        .collect();
    assert_eq!(field_types.len(), 1);
    assert_eq!(field_types[0].target_qualified_name, "Repository");
}

#[test]
fn test_param_type_relationship() {
    let result = parse_and_extract("def process(item: Item) -> None:\n    pass\n");
    let field_types: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::FieldType)
        .collect();
    assert_eq!(field_types.len(), 1);
    assert_eq!(field_types[0].target_qualified_name, "Item");
}

#[test]
fn test_decorator_annotated_by() {
    let result = parse_and_extract("@dataclass\nclass Foo:\n    pass\n");
    let annotations: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::AnnotatedBy)
        .collect();
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].target_qualified_name, "dataclass");
}

#[test]
fn test_call_relationship() {
    let result = parse_and_extract("def main():\n    process()\n");
    let calls: Vec<_> = result.relationships.iter()
        .filter(|r| r.kind == RelationshipKind::Calls)
        .collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].target_qualified_name, "process");
}
```

- [ ] **Step 5: Run unit tests**

Run: `cargo test plugin::python -- --nocapture`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add src/plugin/python/mod.rs
git commit -m "test: add unit tests for Python plugin"
```

---

## Chunk 2: Integration tests and test fixture

### Task 3: Add test fixture and integration tests

**Files:**
- Create: `test-project/src/main/python/app.py`
- Modify: `tests/integration.rs`

- [ ] **Step 1: Create Python test fixture**

Create `test-project/src/main/python/app.py`:
```python
from typing import List


class Repository:
    def save(self, item: "Item") -> None:
        pass

    def find(self, id: int) -> "Item":
        pass


class Item:
    name: str
    id: int


class Service:
    repo: Repository

    def __init__(self, repo: Repository):
        self.repo = repo

    def process(self) -> None:
        item = self.repo.find(1)
        self.repo.save(item)


@dataclass
class Config:
    debug: bool
```

- [ ] **Step 2: Add setup helper and symbol test**

Add to `tests/integration.rs`:
```rust
fn setup_python_project(dir: &std::path::Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src/app.py"),
        r#"from typing import List


class Repository:
    def save(self, item: "Item") -> None:
        pass

    def find(self, id: int) -> "Item":
        pass


class Item:
    name: str
    id: int


class Service:
    repo: Repository

    def __init__(self, repo: Repository):
        self.repo = repo

    def process(self) -> None:
        item = self.repo.find(1)
        self.repo.save(item)


@dataclass
class Config:
    debug: bool
"#,
    )
    .unwrap();
}

#[test]
fn test_python_symbols() {
    let tmp = TempDir::new().unwrap();
    setup_python_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["symbols", "src/app.py"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Repository"), "should contain Repository class");
    assert!(stdout.contains("class"), "should show class kind");
    assert!(stdout.contains("Item"), "should contain Item class");
    assert!(stdout.contains("Service"), "should contain Service class");
    assert!(stdout.contains("__init__"), "should contain constructor");
    assert!(stdout.contains("constructor"), "should show constructor kind");
    assert!(stdout.contains("process"), "should contain process method");
    assert!(stdout.contains("Config"), "should contain Config class");
}
```

- [ ] **Step 3: Add field type refs test**

```rust
#[test]
fn test_python_field_type_refs() {
    let tmp = TempDir::new().unwrap();
    setup_python_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["refs", "Repository"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("repo"), "Service.repo should reference Repository via field type");
}
```

- [ ] **Step 4: Add init indexes test**

```rust
#[test]
fn test_python_init_indexes_files() {
    let tmp = TempDir::new().unwrap();
    setup_python_project(tmp.path());
    let out = codix_cmd(tmp.path()).arg("init").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Indexed 1 Python file"), "stdout was: {}", stdout);
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: all tests pass (unit + integration)

- [ ] **Step 6: Commit**

```bash
git add test-project/src/main/python/app.py tests/integration.rs
git commit -m "test: add Python plugin integration tests"
```

---

### Task 4: Update TODO.md and final verification

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Check off Python plugin in TODO.md**

Find `- [ ] Add Python plugin` and change to `- [x] Add Python plugin`.

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: all tests pass, no warnings

- [ ] **Step 3: Commit**

```bash
git add TODO.md
git commit -m "docs: check off Python plugin in TODO"
```
