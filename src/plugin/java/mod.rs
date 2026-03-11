use std::collections::HashMap;
use std::path::Path;
use crate::model::*;
use super::LanguagePlugin;

pub struct JavaPlugin;

impl LanguagePlugin for JavaPlugin {
    fn name(&self) -> &str {
        "java"
    }
    fn display_name(&self) -> &str {
        "Java"
    }

    fn file_extensions(&self) -> &[&str] {
        &["java"]
    }

    fn tree_sitter_language(&self) -> tree_sitter::Language {
        tree_sitter_java::LANGUAGE.into()
    }

    fn extract_symbols(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        _file_path: &Path,
    ) -> ExtractionResult {
        let root = tree.root_node();
        let mut symbols = Vec::new();

        // Find package declaration
        let package = find_package(root, source);

        let (import_map, wildcard_imports) = parse_imports(root, source);

        // Walk top-level children for type declarations
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if let Some(type_symbol) = extract_type_declaration(child, source, &package, symbols.len()) {
                let type_local_id = type_symbol.local_id;
                let type_qualified_name = type_symbol.qualified_name.clone();
                let type_package = type_symbol.package.clone();
                symbols.push(type_symbol);

                // Extract members from the class body
                extract_members(child, source, &type_qualified_name, &type_package, type_local_id, &import_map, &mut symbols);
            }
        }

        let mut relationships = Vec::new();

        // Extract relationships from top-level type declarations
        let mut cursor2 = root.walk();
        for child in root.children(&mut cursor2) {
            let type_local_id = match child.kind() {
                "class_declaration" | "interface_declaration" | "enum_declaration" | "record_declaration" | "annotation_type_declaration" => {
                    // Find the local_id for this type by matching qualified_name
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source).ok());
                    name.and_then(|n| {
                        symbols.iter().find(|s| s.name == n && s.parent_local_id.is_none()).map(|s| s.local_id)
                    })
                }
                _ => None,
            };
            if let Some(type_local_id) = type_local_id {
                extract_type_relationships(child, source, type_local_id, &symbols, &mut relationships, &import_map, &package);
            }
        }

        ExtractionResult {
            symbols,
            relationships,
            wildcard_imports,
        }
    }
}

fn find_package(root: tree_sitter::Node, source: &[u8]) -> String {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "package_declaration" {
            // package_declaration text is like "package com.foo;"
            // We want the scoped_identifier or identifier child
            let mut pkg_cursor = child.walk();
            for pkg_child in child.children(&mut pkg_cursor) {
                let kind = pkg_child.kind();
                if kind == "scoped_identifier" || kind == "identifier" {
                    if let Ok(text) = pkg_child.utf8_text(source) {
                        return text.to_string();
                    }
                }
            }
        }
    }
    String::new()
}

fn parse_imports(root: tree_sitter::Node, source: &[u8]) -> (HashMap<String, String>, Vec<String>) {
    let mut import_map = HashMap::new();
    let mut wildcards = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() != "import_declaration" {
            continue;
        }
        let text = child.utf8_text(source).unwrap_or("");
        if text.contains("static ") {
            continue;
        }
        // Check if wildcard: last non-semicolon child is "asterisk"
        let mut is_wildcard = false;
        let mut pkg_node = None;
        let mut inner = child.walk();
        for ic in child.children(&mut inner) {
            match ic.kind() {
                "asterisk" => is_wildcard = true,
                "scoped_identifier" | "identifier" => pkg_node = Some(ic),
                _ => {}
            }
        }
        if let Some(node) = pkg_node {
            if let Ok(fqn) = node.utf8_text(source) {
                if is_wildcard {
                    wildcards.push(fqn.to_string());
                } else {
                    // simple name is last segment after "."
                    let simple = fqn.rsplit('.').next().unwrap_or(fqn);
                    import_map.insert(simple.to_string(), fqn.to_string());
                }
            }
        }
    }
    (import_map, wildcards)
}

fn resolve_type_name(simple_name: &str, import_map: &HashMap<String, String>, package: &str) -> String {
    if let Some(qualified) = import_map.get(simple_name) {
        return qualified.clone();
    }
    if !package.is_empty() {
        return format!("{}.{}", package, simple_name);
    }
    simple_name.to_string()
}

fn extract_type_declaration(
    node: tree_sitter::Node,
    source: &[u8],
    package: &str,
    local_id: usize,
) -> Option<ExtractedSymbol> {
    let kind = match node.kind() {
        "class_declaration" => SymbolKind::Class,
        "interface_declaration" => SymbolKind::Interface,
        "enum_declaration" => SymbolKind::Enum,
        "record_declaration" => SymbolKind::Record,
        "annotation_type_declaration" => SymbolKind::Annotation,
        _ => return None,
    };

    let name = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())?;

    let visibility = extract_visibility(node, source);

    let start = node.start_position();
    let end = node.end_position();

    let qualified_name = if package.is_empty() {
        name.clone()
    } else {
        format!("{}.{}", package, name)
    };

    Some(ExtractedSymbol {
        local_id,
        name,
        signature: None,
        qualified_name,
        kind,
        visibility,
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: None,
        package: package.to_string(),
        type_text: None,
    })
}

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
                    "constructor_declaration" | "compact_constructor_declaration" => {
                        if let Some(symbol) = extract_constructor(member, type_node, source, parent_qualified_name, package, parent_local_id, local_id, import_map) {
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
                        if let Some(mut nested_symbol) = extract_type_declaration(member, source, package, local_id) {
                            let name = nested_symbol.name.clone();
                            nested_symbol.qualified_name = format!("{}.{}", parent_qualified_name, name);
                            nested_symbol.parent_local_id = Some(parent_local_id);
                            let nested_local_id = nested_symbol.local_id;
                            let nested_qn = nested_symbol.qualified_name.clone();
                            symbols.push(nested_symbol);
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

fn extract_formal_params(node: tree_sitter::Node, source: &[u8]) -> String {
    let mut params = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "formal_parameter" || child.kind() == "spread_parameter" {
            // Iterate children to find the type node (skip modifiers and the final identifier/name)
            let mut param_cursor = child.walk();
            for param_child in child.children(&mut param_cursor) {
                let kind = param_child.kind();
                if kind == "modifiers" {
                    continue;
                }
                // The type is any named node that is not the last identifier (name)
                if param_child.is_named() && kind != "identifier" {
                    if let Ok(text) = param_child.utf8_text(source) {
                        params.push(text.to_string());
                        break;
                    }
                }
            }
        }
    }
    params.join(",")
}

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

    let signature = format!("{}({})", name, param_types);
    let qualified_name = format!("{}.{}", parent_qualified_name, signature);

    let type_text = if kind == SymbolKind::Constructor {
        None
    } else {
        method_return_type(node, source).map(|t| resolve_type_name(&t, import_map, package))
    };

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

fn extract_constructor(
    node: tree_sitter::Node,
    type_node: tree_sitter::Node,
    source: &[u8],
    parent_qualified_name: &str,
    package: &str,
    parent_local_id: usize,
    local_id: usize,
    _import_map: &HashMap<String, String>,
) -> Option<ExtractedSymbol> {
    let name = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())?;

    let visibility = extract_visibility(node, source);
    let start = node.start_position();
    let end = node.end_position();

    // For compact constructors, parameters come from the record declaration
    let params_owner = if node.kind() == "compact_constructor_declaration" { type_node } else { node };
    let param_types = {
        let mut result = String::new();
        let mut cursor = params_owner.walk();
        for child in params_owner.children(&mut cursor) {
            if child.kind() == "formal_parameters" {
                result = extract_formal_params(child, source);
                break;
            }
        }
        result
    };

    let signature = format!("{}({})", name, param_types);
    let qualified_name = format!("{}.{}", parent_qualified_name, signature);

    Some(ExtractedSymbol {
        local_id,
        name,
        signature: Some(signature),
        qualified_name,
        kind: SymbolKind::Constructor,
        visibility,
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: Some(parent_local_id),
        package: package.to_string(),
        type_text: None,
    })
}

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

    let visibility = extract_visibility(node, source);
    let start = node.start_position();
    let end = node.end_position();
    let qualified_name = format!("{}.{}", parent_qualified_name, name);

    let type_text = field_type_name(node, source)
        .map(|t| resolve_type_name(&t, import_map, package));

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

fn extract_type_relationships(
    type_node: tree_sitter::Node,
    source: &[u8],
    type_local_id: usize,
    symbols: &[ExtractedSymbol],
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
    package: &str,
) {
    extract_annotations(type_node, source, type_local_id, relationships, import_map, package);
    let mut cursor = type_node.walk();
    for child in type_node.children(&mut cursor) {
        match child.kind() {
            "superclass" => {
                // class extends X
                if let Some(name) = first_type_identifier(child, source) {
                    let resolved = resolve_type_name(&name, import_map, package);
                    relationships.push(ExtractedRelationship {
                        source_local_id: type_local_id,
                        target_qualified_name: resolved,
                        kind: RelationshipKind::Extends,
                    });
                }
            }
            "extends_interfaces" => {
                // interface extends X, Y
                collect_type_identifiers(child, source, type_local_id, RelationshipKind::Extends, relationships, import_map, package);
            }
            "super_interfaces" => {
                // class implements X, Y
                collect_type_identifiers(child, source, type_local_id, RelationshipKind::Implements, relationships, import_map, package);
            }
            "class_body" | "interface_body" | "enum_body" | "annotation_type_body" => {
                let type_qn = symbols.iter()
                    .find(|s| s.local_id == type_local_id)
                    .map(|s| s.qualified_name.as_str())
                    .unwrap_or("");
                extract_body_relationships(child, source, type_local_id, type_qn, symbols, relationships, import_map, package);
            }
            _ => {}
        }
    }
}

fn first_type_identifier(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_identifier" {
            return child.utf8_text(source).ok().map(|s| s.to_string());
        }
        // Recurse one level for nodes like `type_list`
        let mut inner = child.walk();
        for inner_child in child.children(&mut inner) {
            if inner_child.kind() == "type_identifier" {
                return inner_child.utf8_text(source).ok().map(|s| s.to_string());
            }
        }
    }
    None
}

fn collect_type_identifiers(
    node: tree_sitter::Node,
    source: &[u8],
    source_local_id: usize,
    kind: RelationshipKind,
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
    package: &str,
) {
    if node.kind() == "type_identifier" {
        if let Ok(name) = node.utf8_text(source) {
            let resolved = resolve_type_name(name, import_map, package);
            relationships.push(ExtractedRelationship {
                source_local_id,
                target_qualified_name: resolved,
                kind,
            });
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_type_identifiers(child, source, source_local_id, kind, relationships, import_map, package);
    }
}

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

                extract_annotations(member, source, field_local_id, relationships, import_map, package);

                if let Some(type_name) = field_type_name(member, source) {
                    let resolved = resolve_type_name(&type_name, import_map, package);
                    relationships.push(ExtractedRelationship {
                        source_local_id: field_local_id,
                        target_qualified_name: resolved,
                        kind: RelationshipKind::FieldType,
                    });
                }
            }
            "method_declaration" | "constructor_declaration" | "compact_constructor_declaration" => {
                let method_local_id = symbols.iter()
                    .find(|s| (s.kind == SymbolKind::Method || s.kind == SymbolKind::Constructor)
                        && s.parent_local_id == Some(type_local_id)
                        && s.line == (member.start_position().row + 1) as i64)
                    .map(|s| s.local_id)
                    .unwrap_or(type_local_id);

                extract_annotations(member, source, method_local_id, relationships, import_map, package);
                extract_param_annotations(member, source, method_local_id, relationships, import_map, package);

                // Build scope for this method: fields + params + this
                let mut scope = field_scope.clone();
                for (name, qtype) in extract_method_params(member, source, import_map, package) {
                    scope.insert(name, qtype);
                }
                scope.insert("this".to_string(), type_qualified_name.to_string());

                collect_method_invocations(member, source, method_local_id, relationships, &scope, type_qualified_name);
            }
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
        }
    }
}

fn field_type_name(field_node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    // The type is the first named child that is a type node
    let mut cursor = field_node.walk();
    for child in field_node.children(&mut cursor) {
        match child.kind() {
            "type_identifier" | "generic_type" => {
                return child.utf8_text(source).ok().map(|s| {
                    // For generic types like "List<String>", just return the base name
                    let base = s.split('<').next().unwrap_or(s);
                    base.to_string()
                });
            }
            "modifiers" => continue,
            _ => {}
        }
    }
    None
}

fn method_return_type(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
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
            "integral_type" | "floating_point_type" | "boolean_type" => return None,
            _ => continue,
        }
    }
    None
}

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
                let target = if let Some(obj_node) = node.child_by_field_name("object") {
                    if let Ok(receiver) = obj_node.utf8_text(source) {
                        if receiver == "this" {
                            format!("{}.{}", enclosing_class, method_name)
                        } else if let Some(receiver_type) = scope.get(receiver) {
                            format!("{}.{}", receiver_type, method_name)
                        } else {
                            method_name.to_string()
                        }
                    } else {
                        method_name.to_string()
                    }
                } else {
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
        // Skip nested type declarations to avoid double-counting their method calls
        match child.kind() {
            "class_declaration" | "interface_declaration" | "enum_declaration"
            | "record_declaration" | "annotation_type_declaration" => continue,
            _ => collect_method_invocations(child, source, method_local_id, relationships, scope, enclosing_class),
        }
    }
}

fn extract_annotations(
    node: tree_sitter::Node,
    source: &[u8],
    source_local_id: usize,
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
    package: &str,
) {
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
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" || child.kind() == "scoped_identifier" {
            return child.utf8_text(source).ok().map(|s| s.to_string());
        }
    }
    None
}

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

fn extract_visibility(node: tree_sitter::Node, source: &[u8]) -> Visibility {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "modifiers" {
            let text = child.utf8_text(source).unwrap_or("");
            if text.contains("public") {
                return Visibility::Public;
            }
            if text.contains("protected") {
                return Visibility::Protected;
            }
            if text.contains("private") {
                return Visibility::Private;
            }
        }
    }
    Visibility::PackagePrivate
}

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

    #[test]
    fn test_extract_methods() {
        let source = "package com.foo;\npublic class Svc {\n  public void save(Person p) {}\n  private int count() { return 0; }\n}";
        let result = parse_java(source);
        assert_eq!(result.symbols.len(), 3); // class + 2 methods
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
    fn test_extract_extends() {
        let source = "package com.foo;\npublic class UserService extends BaseService {}";
        let result = parse_java(source);
        assert_eq!(result.relationships.len(), 1);
        assert_eq!(result.relationships[0].kind, RelationshipKind::Extends);
        assert_eq!(result.relationships[0].target_qualified_name, "com.foo.BaseService");
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
        assert_eq!(field_types[0].target_qualified_name, "com.foo.Repository");
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

    #[test]
    fn test_import_resolves_extends() {
        let source = "package com.bar;\nimport com.foo.BaseService;\npublic class UserService extends BaseService {}";
        let result = parse_java(source);
        assert_eq!(result.relationships.len(), 1);
        assert_eq!(result.relationships[0].target_qualified_name, "com.foo.BaseService");
    }

    #[test]
    fn test_same_package_resolves_type() {
        let source = "package com.foo;\npublic class UserService extends BaseService {}";
        let result = parse_java(source);
        assert_eq!(result.relationships.len(), 1);
        assert_eq!(result.relationships[0].target_qualified_name, "com.foo.BaseService");
    }

    #[test]
    fn test_import_resolves_field_type() {
        let source = "package com.bar;\nimport com.foo.Repository;\npublic class Svc {\n  private Repository repo;\n}";
        let result = parse_java(source);
        let field_types: Vec<_> = result.relationships.iter()
            .filter(|r| r.kind == RelationshipKind::FieldType).collect();
        assert_eq!(field_types.len(), 1);
        assert_eq!(field_types[0].target_qualified_name, "com.foo.Repository");
    }

    #[test]
    fn test_wildcard_imports_returned() {
        let source = "package com.bar;\nimport com.foo.*;\npublic class Svc extends Base {}";
        let result = parse_java(source);
        assert_eq!(result.relationships[0].target_qualified_name, "com.bar.Base");
        assert_eq!(result.wildcard_imports, vec!["com.foo".to_string()]);
    }

    #[test]
    fn test_method_calls_with_import_resolved_receiver() {
        let source = "package com.bar;\nimport com.foo.Repository;\npublic class Svc {\n  private Repository repo;\n  void work() { repo.save(); }\n}";
        let result = parse_java(source);
        let calls: Vec<_> = result.relationships.iter()
            .filter(|r| r.kind == RelationshipKind::Calls).collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].target_qualified_name, "com.foo.Repository.save");
    }

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

    #[test]
    fn test_no_package_no_qualification() {
        let source = "public class Svc extends Base {}";
        let result = parse_java(source);
        assert_eq!(result.relationships[0].target_qualified_name, "Base");
    }

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

    #[test]
    fn test_record_compact_constructor() {
        let source = "package com.foo;\npublic record Point(int x, int y) {\n  Point {\n    if (x < 0) throw new IllegalArgumentException();\n  }\n}";
        let result = parse_java(source);
        assert_eq!(result.symbols.len(), 2);
        let ctor = result.symbols.iter().find(|s| s.kind == SymbolKind::Constructor).unwrap();
        assert_eq!(ctor.name, "Point");
        assert_eq!(ctor.signature, Some("Point(int,int)".to_string()));
        assert_eq!(ctor.qualified_name, "com.foo.Point.Point(int,int)");
        assert_eq!(ctor.parent_local_id, Some(0));
    }

    #[test]
    fn test_record_canonical_constructor() {
        let source = "package com.foo;\npublic record Point(int x, int y) {\n  Point(int x, int y) {\n    this.x = x;\n    this.y = y;\n  }\n}";
        let result = parse_java(source);
        assert_eq!(result.symbols.len(), 2);
        let ctor = result.symbols.iter().find(|s| s.kind == SymbolKind::Constructor).unwrap();
        assert_eq!(ctor.name, "Point");
        assert_eq!(ctor.signature, Some("Point(int,int)".to_string()));
        assert_eq!(ctor.qualified_name, "com.foo.Point.Point(int,int)");
    }
}
