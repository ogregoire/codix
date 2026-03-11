use crate::model::*;
use std::path::Path;

pub struct PythonPlugin;

impl super::LanguagePlugin for PythonPlugin {
    fn name(&self) -> &str {
        "python"
    }
    fn display_name(&self) -> &str {
        "Python"
    }

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
                extract_class(
                    def,
                    source,
                    parent_local_id,
                    parent_name,
                    symbols,
                    relationships,
                );
            }
            "function_definition" => {
                extract_function(
                    def,
                    source,
                    parent_local_id,
                    parent_name,
                    symbols,
                    relationships,
                );
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
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "identifier" => return child.utf8_text(source).ok().map(|s| s.to_string()),
            "attribute" => {
                return child.utf8_text(source).ok().map(|s| s.to_string());
            }
            "call" => {
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
    if name.starts_with("__") && name.ends_with("__") {
        // Dunder methods (__init__, __str__, etc.) are public
        Visibility::new("public")
    } else if name.starts_with("__") {
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

    // Extract base classes -> Extends relationships
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
                    extract_function(
                        child,
                        source,
                        Some(local_id),
                        &qualified_name,
                        symbols,
                        relationships,
                    );
                }
                "decorated_definition" => {
                    extract_decorated(
                        child,
                        source,
                        Some(local_id),
                        &qualified_name,
                        symbols,
                        relationships,
                    );
                }
                "class_definition" => {
                    extract_class(
                        child,
                        source,
                        Some(local_id),
                        &qualified_name,
                        symbols,
                        relationships,
                    );
                }
                "expression_statement" => {
                    let mut ec = child.walk();
                    for expr in child.children(&mut ec) {
                        if expr.kind() == "assignment" {
                            extract_class_field(
                                expr,
                                source,
                                local_id,
                                &qualified_name,
                                symbols,
                                relationships,
                            );
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

    let signature = build_signature(&name, node, source);

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

    if let Some(params) = node.child_by_field_name("parameters") {
        extract_param_type_relationships(params, source, local_id, relationships);
    }

    if let Some(rt) = return_type {
        extract_field_type_relationships(rt, source, local_id, relationships);
    }

    if let Some(body) = node.child_by_field_name("body") {
        collect_calls(body, source, local_id, relationships);
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
                    extract_field_type_relationships(
                        type_node,
                        source,
                        source_local_id,
                        relationships,
                    );
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
            if let Ok(s) = node.utf8_text(source) {
                out.push(s.to_string());
            }
        }
        "type" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_type_names(child, source, out);
            }
        }
        "generic_type" | "subscript" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_type_names(child, source, out);
            }
        }
        "binary_operator" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_type_names(child, source, out);
            }
        }
        _ => {}
    }
}

fn is_user_type(name: &str) -> bool {
    let base = name.split('.').next_back().unwrap_or(name);
    base.starts_with(|c: char| c.is_uppercase())
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
                "attribute" => func
                    .child_by_field_name("attribute")
                    .and_then(|a| a.utf8_text(source).ok())
                    .map(|s| s.to_string()),
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
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(child, source, source_local_id, relationships);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::LanguagePlugin;

    fn parse_and_extract(code: &str) -> ExtractionResult {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(code, None).unwrap();
        let plugin = PythonPlugin;
        plugin.extract_symbols(&tree, code.as_bytes(), std::path::Path::new("test.py"))
    }

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
        assert_eq!(result.symbols.len(), 2);
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

    #[test]
    fn test_class_field() {
        let result = parse_and_extract("class Foo:\n    name: str\n");
        assert_eq!(result.symbols.len(), 2);
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
        assert_eq!(result.symbols[4].visibility, Visibility::new("public"));
    }

    #[test]
    fn test_extends() {
        let result = parse_and_extract("class Foo(Bar, Baz):\n    pass\n");
        let extends: Vec<_> = result
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Extends)
            .collect();
        assert_eq!(extends.len(), 2);
        assert_eq!(extends[0].target_qualified_name, "Bar");
        assert_eq!(extends[1].target_qualified_name, "Baz");
    }

    #[test]
    fn test_field_type_relationship() {
        let result = parse_and_extract("class Foo:\n    repo: Repository\n");
        let field_types: Vec<_> = result
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::FieldType)
            .collect();
        assert_eq!(field_types.len(), 1);
        assert_eq!(field_types[0].target_qualified_name, "Repository");
    }

    #[test]
    fn test_param_type_relationship() {
        let result = parse_and_extract("def process(item: Item) -> None:\n    pass\n");
        let field_types: Vec<_> = result
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::FieldType)
            .collect();
        assert_eq!(field_types.len(), 1);
        assert_eq!(field_types[0].target_qualified_name, "Item");
    }

    #[test]
    fn test_decorator_annotated_by() {
        let result = parse_and_extract("@dataclass\nclass Foo:\n    pass\n");
        let annotations: Vec<_> = result
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::AnnotatedBy)
            .collect();
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].target_qualified_name, "dataclass");
    }

    #[test]
    fn test_call_relationship() {
        let result = parse_and_extract("def main():\n    process()\n");
        let calls: Vec<_> = result
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Calls)
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].target_qualified_name, "process");
    }
}
