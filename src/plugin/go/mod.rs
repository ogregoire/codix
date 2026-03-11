use super::LanguagePlugin;
use crate::model::*;
use std::path::Path;

pub struct GoPlugin;

impl LanguagePlugin for GoPlugin {
    fn name(&self) -> &str {
        "go"
    }
    fn display_name(&self) -> &str {
        "Go"
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.extension().and_then(|e| e.to_str()) == Some("go")
    }

    fn tree_sitter_language(&self) -> tree_sitter::Language {
        tree_sitter_go::LANGUAGE.into()
    }

    fn extract_symbols(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        _file_path: &Path,
    ) -> ExtractionResult {
        let root = tree.root_node();
        let mut symbols = Vec::new();
        let mut relationships = Vec::new();

        let package = find_package(root, source);

        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            match child.kind() {
                "type_declaration" => {
                    extract_type_declaration(
                        child,
                        source,
                        &package,
                        &mut symbols,
                        &mut relationships,
                    );
                }
                "function_declaration" => {
                    extract_function(child, source, &package, &mut symbols, &mut relationships);
                }
                "method_declaration" => {
                    extract_method(child, source, &package, &mut symbols, &mut relationships);
                }
                _ => {}
            }
        }

        ExtractionResult {
            symbols,
            relationships,
            wildcard_imports: Vec::new(),
        }
    }
}

fn find_package(root: tree_sitter::Node, source: &[u8]) -> String {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "package_clause" {
            let mut inner = child.walk();
            for pkg_child in child.children(&mut inner) {
                if pkg_child.kind() == "package_identifier" {
                    if let Ok(text) = pkg_child.utf8_text(source) {
                        return text.to_string();
                    }
                }
            }
        }
    }
    String::new()
}

fn visibility_from_name(name: &str) -> Visibility {
    if name.starts_with(|c: char| c.is_uppercase()) {
        Visibility::new("public")
    } else {
        Visibility::new("private")
    }
}

fn extract_type_declaration(
    node: tree_sitter::Node,
    source: &[u8],
    package: &str,
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_spec" {
            extract_type_spec(child, source, package, symbols, relationships);
        }
    }
}

fn extract_type_spec(
    node: tree_sitter::Node,
    source: &[u8],
    package: &str,
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

    let type_node = match node.child_by_field_name("type") {
        Some(n) => n,
        None => return,
    };

    let kind = match type_node.kind() {
        "struct_type" => "struct",
        "interface_type" => "interface",
        _ => return,
    };

    let start = node.start_position();
    let end = node.end_position();
    let local_id = symbols.len();
    let qualified_name = if package.is_empty() {
        name.clone()
    } else {
        format!("{}.{}", package, name)
    };

    symbols.push(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: None,
        qualified_name: qualified_name.clone(),
        kind: SymbolKind::new(kind),
        visibility: visibility_from_name(&name),
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: None,
        package: package.to_string(),
        type_text: None,
    });

    match kind {
        "struct" => extract_struct_members(
            type_node,
            source,
            &qualified_name,
            package,
            local_id,
            symbols,
            relationships,
        ),
        "interface" => extract_interface_members(
            type_node,
            source,
            &qualified_name,
            package,
            local_id,
            symbols,
            relationships,
        ),
        _ => {}
    }
}

fn extract_struct_members(
    struct_node: tree_sitter::Node,
    source: &[u8],
    parent_qualified_name: &str,
    package: &str,
    parent_local_id: usize,
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    let mut cursor = struct_node.walk();
    for child in struct_node.children(&mut cursor) {
        if child.kind() == "field_declaration_list" {
            let mut field_cursor = child.walk();
            for field in child.children(&mut field_cursor) {
                if field.kind() == "field_declaration" {
                    extract_struct_field(
                        field,
                        source,
                        parent_qualified_name,
                        package,
                        parent_local_id,
                        symbols,
                        relationships,
                    );
                }
            }
        }
    }
}

fn extract_struct_field(
    node: tree_sitter::Node,
    source: &[u8],
    parent_qualified_name: &str,
    package: &str,
    parent_local_id: usize,
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    let type_node = node.child_by_field_name("type");

    // Check for embedded field (no name, just a type)
    let name_node = node.child_by_field_name("name");
    if name_node.is_none() {
        // Embedded struct — this is an extends relationship
        if let Some(type_n) = type_node {
            if let Ok(type_text) = type_n.utf8_text(source) {
                let type_name = type_text.trim_start_matches('*');
                relationships.push(ExtractedRelationship {
                    source_local_id: parent_local_id,
                    target_qualified_name: type_name.to_string(),
                    kind: RelationshipKind::Extends,
                });
            }
        }
        return;
    }

    let name = match name_node.and_then(|n| n.utf8_text(source).ok()) {
        Some(s) => s.to_string(),
        None => return,
    };

    let start = node.start_position();
    let end = node.end_position();
    let local_id = symbols.len();
    let qualified_name = format!("{}.{}", parent_qualified_name, name);

    let type_text = type_node
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string());

    symbols.push(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: None,
        qualified_name,
        kind: SymbolKind::new("field"),
        visibility: visibility_from_name(&name),
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: Some(parent_local_id),
        package: package.to_string(),
        type_text: type_text.clone(),
    });

    // Field type relationship
    if let Some(ref type_str) = type_text {
        let type_name = extract_base_type(type_str);
        if is_user_type(&type_name) {
            relationships.push(ExtractedRelationship {
                source_local_id: local_id,
                target_qualified_name: type_name,
                kind: RelationshipKind::FieldType,
            });
        }
    }
}

fn extract_interface_members(
    iface_node: tree_sitter::Node,
    source: &[u8],
    parent_qualified_name: &str,
    package: &str,
    parent_local_id: usize,
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    let mut cursor = iface_node.walk();
    for child in iface_node.children(&mut cursor) {
        match child.kind() {
            "method_elem" => {
                let name = match child.child_by_field_name("name") {
                    Some(n) => match n.utf8_text(source) {
                        Ok(s) => s.to_string(),
                        Err(_) => continue,
                    },
                    None => continue,
                };

                let start = child.start_position();
                let end = child.end_position();
                let local_id = symbols.len();
                let qualified_name = format!("{}.{}", parent_qualified_name, name);

                symbols.push(ExtractedSymbol {
                    local_id,
                    name: name.clone(),
                    signature: Some(format!("{}()", name)),
                    qualified_name,
                    kind: SymbolKind::new("method"),
                    visibility: visibility_from_name(&name),
                    line: (start.row + 1) as i64,
                    column: start.column as i64,
                    end_line: (end.row + 1) as i64,
                    end_column: end.column as i64,
                    parent_local_id: Some(parent_local_id),
                    package: package.to_string(),
                    type_text: None,
                });

                // Extract parameter and return type relationships
                extract_param_type_relationships(child, source, local_id, relationships);
                extract_return_type_relationships(child, source, local_id, relationships);
            }
            // Embedded interface
            "type_elem" => {
                if let Some(type_id) = child.child(0) {
                    if let Ok(text) = type_id.utf8_text(source) {
                        if is_user_type(text) {
                            relationships.push(ExtractedRelationship {
                                source_local_id: parent_local_id,
                                target_qualified_name: text.to_string(),
                                kind: RelationshipKind::Extends,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn extract_function(
    node: tree_sitter::Node,
    source: &[u8],
    package: &str,
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

    let start = node.start_position();
    let end = node.end_position();
    let local_id = symbols.len();
    let qualified_name = if package.is_empty() {
        name.clone()
    } else {
        format!("{}.{}", package, name)
    };

    symbols.push(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: Some(format!("{}()", name)),
        qualified_name,
        kind: SymbolKind::new("function"),
        visibility: visibility_from_name(&name),
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: None,
        package: package.to_string(),
        type_text: None,
    });

    extract_param_type_relationships(node, source, local_id, relationships);
    extract_return_type_relationships(node, source, local_id, relationships);
    extract_call_relationships(node, source, local_id, relationships);
}

fn extract_receiver_type(node: tree_sitter::Node, source: &[u8]) -> String {
    let receiver = match node.child_by_field_name("receiver") {
        Some(r) => r,
        None => return String::new(),
    };
    let mut cursor = receiver.walk();
    for child in receiver.children(&mut cursor) {
        if child.kind() == "parameter_declaration" {
            if let Some(type_node) = child.child_by_field_name("type") {
                if let Ok(text) = type_node.utf8_text(source) {
                    return text.trim_start_matches('*').to_string();
                }
            }
        }
    }
    String::new()
}

fn extract_method(
    node: tree_sitter::Node,
    source: &[u8],
    package: &str,
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

    // Get receiver type to build qualified name
    let receiver_type = extract_receiver_type(node, source);

    let start = node.start_position();
    let end = node.end_position();
    let local_id = symbols.len();

    let qualified_name = if receiver_type.is_empty() {
        if package.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", package, name)
        }
    } else if package.is_empty() {
        format!("{}.{}", receiver_type, name)
    } else {
        format!("{}.{}.{}", package, receiver_type, name)
    };

    symbols.push(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: Some(format!("{}()", name)),
        qualified_name,
        kind: SymbolKind::new("method"),
        visibility: visibility_from_name(&name),
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: None,
        package: package.to_string(),
        type_text: None,
    });

    extract_param_type_relationships(node, source, local_id, relationships);
    extract_return_type_relationships(node, source, local_id, relationships);
    extract_call_relationships(node, source, local_id, relationships);
}

fn extract_param_type_relationships(
    node: tree_sitter::Node,
    source: &[u8],
    source_local_id: usize,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    let params = match node.child_by_field_name("parameters") {
        Some(p) => p,
        None => return,
    };
    let mut cursor = params.walk();
    for child in params.children(&mut cursor) {
        if child.kind() == "parameter_declaration"
            || child.kind() == "variadic_parameter_declaration"
        {
            if let Some(type_node) = child.child_by_field_name("type") {
                if let Ok(text) = type_node.utf8_text(source) {
                    let type_name = extract_base_type(text);
                    if is_user_type(&type_name) {
                        relationships.push(ExtractedRelationship {
                            source_local_id,
                            target_qualified_name: type_name,
                            kind: RelationshipKind::FieldType,
                        });
                    }
                }
            }
        }
    }
}

fn extract_return_type_relationships(
    node: tree_sitter::Node,
    source: &[u8],
    source_local_id: usize,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    let result = match node.child_by_field_name("result") {
        Some(r) => r,
        None => return,
    };

    if result.kind() == "parameter_list" {
        // Multiple return values
        let mut cursor = result.walk();
        for child in result.children(&mut cursor) {
            if child.kind() == "parameter_declaration" {
                if let Some(type_node) = child.child_by_field_name("type") {
                    if let Ok(text) = type_node.utf8_text(source) {
                        let type_name = extract_base_type(text);
                        if is_user_type(&type_name) {
                            relationships.push(ExtractedRelationship {
                                source_local_id,
                                target_qualified_name: type_name,
                                kind: RelationshipKind::FieldType,
                            });
                        }
                    }
                }
            }
        }
    } else {
        // Single return type
        if let Ok(text) = result.utf8_text(source) {
            let type_name = extract_base_type(text);
            if is_user_type(&type_name) {
                relationships.push(ExtractedRelationship {
                    source_local_id,
                    target_qualified_name: type_name,
                    kind: RelationshipKind::FieldType,
                });
            }
        }
    }
}

fn extract_call_relationships(
    node: tree_sitter::Node,
    source: &[u8],
    source_local_id: usize,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    let body = match node.child_by_field_name("body") {
        Some(b) => b,
        None => return,
    };
    collect_calls(body, source, source_local_id, relationships);
}

fn collect_calls(
    node: tree_sitter::Node,
    source: &[u8],
    source_local_id: usize,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    if node.kind() == "call_expression" {
        if let Some(func_node) = node.child_by_field_name("function") {
            let call_name = match func_node.kind() {
                "selector_expression" => {
                    // e.g. r.Save() — extract "Save"
                    func_node
                        .child_by_field_name("field")
                        .and_then(|f| f.utf8_text(source).ok())
                        .map(|s| s.to_string())
                }
                "identifier" => func_node.utf8_text(source).ok().map(|s| s.to_string()),
                _ => None,
            };
            if let Some(name) = call_name {
                relationships.push(ExtractedRelationship {
                    source_local_id,
                    target_qualified_name: name,
                    kind: RelationshipKind::Calls,
                });
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(child, source, source_local_id, relationships);
    }
}

/// Strip pointer, slice, map, and channel prefixes to get the base type name.
fn extract_base_type(type_str: &str) -> String {
    let s = type_str.trim();
    let s = s.trim_start_matches('*');
    let s = s.trim_start_matches("[]");
    let s = s.trim_start_matches("chan ");
    let s = s.trim_start_matches("<-chan ");
    let s = s.trim_start_matches("chan<- ");
    s.to_string()
}

/// Returns true if the type name looks like a user-defined type (starts with uppercase).
fn is_user_type(name: &str) -> bool {
    name.starts_with(|c: char| c.is_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_extract(source: &str) -> ExtractionResult {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_go::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let plugin = GoPlugin;
        plugin.extract_symbols(&tree, source.as_bytes(), Path::new("test.go"))
    }

    #[test]
    fn test_struct_extraction() {
        let result =
            parse_and_extract("package main\n\ntype Foo struct {\n\tName string\n\tAge  int\n}");
        assert_eq!(result.symbols.len(), 3);
        assert_eq!(result.symbols[0].name, "Foo");
        assert_eq!(result.symbols[0].kind, SymbolKind::new("struct"));
        assert_eq!(result.symbols[0].qualified_name, "main.Foo");
        assert_eq!(result.symbols[0].visibility, Visibility::new("public"));
        assert_eq!(result.symbols[1].name, "Name");
        assert_eq!(result.symbols[1].kind, SymbolKind::new("field"));
        assert_eq!(result.symbols[2].name, "Age");
    }

    #[test]
    fn test_interface_extraction() {
        let result = parse_and_extract(
            "package main\n\ntype Repository interface {\n\tSave(item Item)\n\tFind(id int) Item\n}"
        );
        assert_eq!(result.symbols.len(), 3);
        assert_eq!(result.symbols[0].name, "Repository");
        assert_eq!(result.symbols[0].kind, SymbolKind::new("interface"));
        assert_eq!(result.symbols[1].name, "Save");
        assert_eq!(result.symbols[1].kind, SymbolKind::new("method"));
        assert_eq!(result.symbols[2].name, "Find");
    }

    #[test]
    fn test_function_declaration() {
        let result = parse_and_extract("package main\n\nfunc main() {}");
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "main");
        assert_eq!(result.symbols[0].kind, SymbolKind::new("function"));
        assert_eq!(result.symbols[0].qualified_name, "main.main");
        assert_eq!(result.symbols[0].visibility, Visibility::new("private"));
    }

    #[test]
    fn test_method_declaration() {
        let result =
            parse_and_extract("package main\n\ntype Foo struct {}\n\nfunc (f *Foo) Bar() {}");
        assert_eq!(result.symbols.len(), 2);
        assert_eq!(result.symbols[1].name, "Bar");
        assert_eq!(result.symbols[1].kind, SymbolKind::new("method"));
        assert_eq!(result.symbols[1].qualified_name, "main.Foo.Bar");
        assert_eq!(result.symbols[1].visibility, Visibility::new("public"));
    }

    #[test]
    fn test_visibility() {
        let result =
            parse_and_extract("package main\n\ntype foo struct {\n\tname string\n\tAge  int\n}");
        assert_eq!(result.symbols[0].visibility, Visibility::new("private")); // foo
        assert_eq!(result.symbols[1].visibility, Visibility::new("private")); // name
        assert_eq!(result.symbols[2].visibility, Visibility::new("public")); // Age
    }

    #[test]
    fn test_struct_embedding() {
        let result = parse_and_extract(
            "package main\n\ntype Base struct {}\n\ntype Child struct {\n\tBase\n}",
        );
        assert_eq!(result.relationships.len(), 1);
        assert_eq!(result.relationships[0].kind, RelationshipKind::Extends);
        assert_eq!(result.relationships[0].target_qualified_name, "Base");
    }

    #[test]
    fn test_interface_embedding() {
        let result = parse_and_extract(
            "package main\n\ntype Reader interface {\n\tRead() error\n}\n\ntype ReadWriter interface {\n\tReader\n\tWrite() error\n}"
        );
        let extends: Vec<_> = result
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Extends)
            .collect();
        assert_eq!(extends.len(), 1);
        assert_eq!(extends[0].target_qualified_name, "Reader");
    }

    #[test]
    fn test_field_type_relationship() {
        let result =
            parse_and_extract("package main\n\ntype Service struct {\n\trepo Repository\n}");
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
        let result = parse_and_extract("package main\n\nfunc Process(r Repository) {}");
        let field_types: Vec<_> = result
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::FieldType)
            .collect();
        assert_eq!(field_types.len(), 1);
        assert_eq!(field_types[0].target_qualified_name, "Repository");
    }

    #[test]
    fn test_return_type_relationship() {
        let result =
            parse_and_extract("package main\n\nfunc NewService() Service { return Service{} }");
        let field_types: Vec<_> = result
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::FieldType)
            .collect();
        assert_eq!(field_types.len(), 1);
        assert_eq!(field_types[0].target_qualified_name, "Service");
    }

    #[test]
    fn test_call_relationship() {
        let result = parse_and_extract("package main\n\nfunc main() {\n\tProcess()\n}");
        let calls: Vec<_> = result
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Calls)
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].target_qualified_name, "Process");
    }

    #[test]
    fn test_pointer_type_stripped() {
        let result =
            parse_and_extract("package main\n\ntype Service struct {\n\trepo *Repository\n}");
        let field_types: Vec<_> = result
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::FieldType)
            .collect();
        assert_eq!(field_types.len(), 1);
        assert_eq!(field_types[0].target_qualified_name, "Repository");
    }
}
