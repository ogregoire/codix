use std::collections::HashMap;
use std::path::Path;
use crate::model::*;
use super::LanguagePlugin;

pub struct RustPlugin;

impl LanguagePlugin for RustPlugin {
    fn name(&self) -> &str {
        "rust"
    }
    fn display_name(&self) -> &str {
        "Rust"
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.extension().and_then(|e| e.to_str()) == Some("rs")
    }

    fn tree_sitter_language(&self) -> tree_sitter::Language {
        tree_sitter_rust::LANGUAGE.into()
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

        let (import_map, wildcard_imports) = parse_use_declarations(root, source);

        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            match child.kind() {
                "struct_item" => {
                    extract_struct(child, source, &mut symbols, &mut relationships, &import_map);
                }
                "enum_item" => {
                    extract_enum(child, source, &mut symbols, &mut relationships);
                }
                "trait_item" => {
                    extract_trait(child, source, &mut symbols, &mut relationships, &import_map);
                }
                "impl_item" => {
                    extract_impl(child, source, &mut symbols, &mut relationships, &import_map);
                }
                "function_item" => {
                    extract_function(child, source, &mut symbols, &mut relationships, &import_map);
                }
                "type_item" => {
                    extract_type_alias(child, source, &mut symbols, &mut relationships, &import_map);
                }
                _ => {}
            }
        }

        ExtractionResult {
            symbols,
            relationships,
            wildcard_imports,
        }
    }
}

// --- Use declaration parsing ---

fn parse_use_declarations(root: tree_sitter::Node, source: &[u8]) -> (HashMap<String, String>, Vec<String>) {
    let mut import_map = HashMap::new();
    let mut wildcards = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "use_declaration" {
            // The use_declaration has a single child describing the use tree
            let mut inner = child.walk();
            for uc in child.children(&mut inner) {
                parse_use_tree(uc, source, "", &mut import_map, &mut wildcards);
            }
        }
    }
    (import_map, wildcards)
}

fn parse_use_tree(
    node: tree_sitter::Node,
    source: &[u8],
    prefix: &str,
    import_map: &mut HashMap<String, String>,
    wildcards: &mut Vec<String>,
) {
    match node.kind() {
        "use_as_clause" => {
            // use path::Type as Alias;
            let path = node.child_by_field_name("path")
                .and_then(|n| n.utf8_text(source).ok());
            let alias = node.child_by_field_name("alias")
                .and_then(|n| n.utf8_text(source).ok());
            if let (Some(path), Some(alias)) = (path, alias) {
                let full_path = if prefix.is_empty() { path.to_string() } else { format!("{}::{}", prefix, path) };
                import_map.insert(alias.to_string(), full_path);
            }
        }
        "scoped_use_list" => {
            // use path::{A, B};
            let path_node = node.child_by_field_name("path");
            let new_prefix = match path_node.and_then(|n| n.utf8_text(source).ok()) {
                Some(p) => if prefix.is_empty() { p.to_string() } else { format!("{}::{}", prefix, p) },
                None => prefix.to_string(),
            };
            if let Some(list) = node.child_by_field_name("list") {
                let mut cursor = list.walk();
                for child in list.children(&mut cursor) {
                    parse_use_tree(child, source, &new_prefix, import_map, wildcards);
                }
            }
        }
        "use_wildcard" => {
            // use path::*;
            if let Some(path_node) = node.child(0) {
                if let Ok(path) = path_node.utf8_text(source) {
                    let full_path = if prefix.is_empty() { path.to_string() } else { format!("{}::{}", prefix, path) };
                    wildcards.push(full_path);
                }
            }
        }
        "scoped_identifier" | "identifier" => {
            // Simple use path::Type;
            if let Ok(text) = node.utf8_text(source) {
                let full_path = if prefix.is_empty() { text.to_string() } else { format!("{}::{}", prefix, text) };
                let simple = text.rsplit("::").next().unwrap_or(text);
                import_map.insert(simple.to_string(), full_path);
            }
        }
        _ => {}
    }
}

// --- Visibility ---

fn extract_visibility(node: tree_sitter::Node, source: &[u8]) -> Visibility {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            if let Ok(text) = child.utf8_text(source) {
                return Visibility::new(text.trim());
            }
        }
    }
    Visibility::new("private")
}

// --- Type resolution ---

fn resolve_type_name(name: &str, import_map: &HashMap<String, String>) -> String {
    if let Some(resolved) = import_map.get(name) {
        return resolved.clone();
    }
    name.to_string()
}

/// Extract all user type names from a type expression.
/// e.g. `Box<dyn Repository>` → ["Box", "Repository"]
/// e.g. `&mut Item` → ["Item"]
/// e.g. `HashMap<String, Vec<Item>>` → ["HashMap", "Item"]
fn extract_type_names(node: tree_sitter::Node, source: &[u8], out: &mut Vec<String>) {
    match node.kind() {
        "type_identifier" => {
            if let Ok(s) = node.utf8_text(source) {
                out.push(s.to_string());
            }
        }
        "scoped_type_identifier" => {
            if let Ok(s) = node.utf8_text(source) {
                out.push(s.to_string());
            }
        }
        "generic_type" => {
            // Base type + type arguments
            if let Some(t) = node.child_by_field_name("type") {
                extract_type_names(t, source, out);
            }
            if let Some(args) = node.child_by_field_name("type_arguments") {
                let mut cursor = args.walk();
                for child in args.children(&mut cursor) {
                    extract_type_names(child, source, out);
                }
            }
        }
        "reference_type" | "pointer_type" => {
            if let Some(t) = node.child_by_field_name("type") {
                extract_type_names(t, source, out);
            }
        }
        "dynamic_type" => {
            // dyn Trait → Trait
            if let Some(t) = node.child_by_field_name("trait") {
                extract_type_names(t, source, out);
            } else {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "type_identifier" || child.kind() == "scoped_type_identifier" {
                        if let Ok(s) = child.utf8_text(source) {
                            out.push(s.to_string());
                        }
                    }
                }
            }
        }
        "abstract_type" => {
            // impl Trait → Trait
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "type_identifier" || child.kind() == "scoped_type_identifier" {
                    if let Ok(s) = child.utf8_text(source) {
                        out.push(s.to_string());
                    }
                }
            }
        }
        "tuple_type" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_type_names(child, source, out);
            }
        }
        _ => {}
    }
}

fn is_user_type(name: &str) -> bool {
    // Skip primitive types and common std types
    let first = name.split("::").last().unwrap_or(name);
    first.starts_with(|c: char| c.is_uppercase())
}

// --- Struct extraction ---

fn extract_struct(
    node: tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
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
    let visibility = extract_visibility(node, source);

    symbols.push(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: None,
        qualified_name: name.clone(),
        kind: SymbolKind::new("struct"),
        visibility,
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: None,
        package: String::new(),
        type_text: None,
    });

    // Extract fields
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "field_declaration_list" {
            let mut field_cursor = child.walk();
            for field in child.children(&mut field_cursor) {
                if field.kind() == "field_declaration" {
                    extract_field(field, source, &name, local_id, symbols, relationships, import_map);
                }
            }
        }
    }
}

fn extract_field(
    node: tree_sitter::Node,
    source: &[u8],
    parent_name: &str,
    parent_local_id: usize,
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
) {
    let name = match node.child_by_field_name("name") {
        Some(n) => match n.utf8_text(source) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        },
        None => return,
    };

    let type_node = node.child_by_field_name("type");
    let type_text = type_node.and_then(|n| n.utf8_text(source).ok()).map(|s| s.to_string());

    let start = node.start_position();
    let end = node.end_position();
    let local_id = symbols.len();
    let visibility = extract_visibility(node, source);

    symbols.push(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: None,
        qualified_name: format!("{}.{}", parent_name, name),
        kind: SymbolKind::new("field"),
        visibility,
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: Some(parent_local_id),
        package: String::new(),
        type_text: type_text.clone(),
    });

    // Field type relationship
    if let Some(tn) = type_node {
        let mut type_names = Vec::new();
        extract_type_names(tn, source, &mut type_names);
        for type_name in type_names {
            if is_user_type(&type_name) {
                let resolved = resolve_type_name(&type_name, import_map);
                relationships.push(ExtractedRelationship {
                    source_local_id: local_id,
                    target_qualified_name: resolved,
                    kind: RelationshipKind::FieldType,
                });
            }
        }
    }
}

// --- Enum extraction ---

fn extract_enum(
    node: tree_sitter::Node,
    source: &[u8],
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
    let visibility = extract_visibility(node, source);

    symbols.push(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: None,
        qualified_name: name.clone(),
        kind: SymbolKind::new("enum"),
        visibility,
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: None,
        package: String::new(),
        type_text: None,
    });

    // Extract variants
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "enum_variant_list" {
            let mut variant_cursor = child.walk();
            for variant in child.children(&mut variant_cursor) {
                if variant.kind() == "enum_variant" {
                    extract_enum_variant(variant, source, &name, local_id, symbols, relationships);
                }
            }
        }
    }
}

fn extract_enum_variant(
    node: tree_sitter::Node,
    source: &[u8],
    enum_name: &str,
    enum_local_id: usize,
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
    let variant_local_id = symbols.len();

    symbols.push(ExtractedSymbol {
        local_id: variant_local_id,
        name: name.clone(),
        signature: None,
        qualified_name: format!("{}.{}", enum_name, name),
        kind: SymbolKind::new("variant"),
        visibility: Visibility::new("pub"),
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: Some(enum_local_id),
        package: String::new(),
        type_text: None,
    });

    // Extract variant fields (struct variants)
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "field_declaration_list" {
            let variant_qn = format!("{}.{}", enum_name, name);
            let mut field_cursor = child.walk();
            for field in child.children(&mut field_cursor) {
                if field.kind() == "field_declaration" {
                    extract_variant_field(field, source, &variant_qn, variant_local_id, symbols, relationships);
                }
            }
        }
    }
}

fn extract_variant_field(
    node: tree_sitter::Node,
    source: &[u8],
    parent_qn: &str,
    parent_local_id: usize,
    symbols: &mut Vec<ExtractedSymbol>,
    _relationships: &mut Vec<ExtractedRelationship>,
) {
    let name = match node.child_by_field_name("name") {
        Some(n) => match n.utf8_text(source) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        },
        None => return,
    };

    let type_text = node.child_by_field_name("type")
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string());

    let start = node.start_position();
    let end = node.end_position();

    symbols.push(ExtractedSymbol {
        local_id: symbols.len(),
        name: name.clone(),
        signature: None,
        qualified_name: format!("{}.{}", parent_qn, name),
        kind: SymbolKind::new("field"),
        visibility: Visibility::new("pub"),
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: Some(parent_local_id),
        package: String::new(),
        type_text,
    });
}

// --- Trait extraction ---

fn extract_trait(
    node: tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
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
    let visibility = extract_visibility(node, source);

    symbols.push(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: None,
        qualified_name: name.clone(),
        kind: SymbolKind::new("trait"),
        visibility,
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: None,
        package: String::new(),
        type_text: None,
    });

    // Extract supertraits: trait Foo: Bar + Baz
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "trait_bounds" {
            extract_trait_bounds(child, source, local_id, relationships, import_map);
        }
    }

    // Extract method signatures from trait body
    if let Some(body) = node.child_by_field_name("body") {
        let mut body_cursor = body.walk();
        for member in body.children(&mut body_cursor) {
            if member.kind() == "function_signature_item" || member.kind() == "function_item" {
                extract_trait_method(member, source, &name, local_id, symbols, relationships, import_map);
            }
        }
    }
}

fn extract_trait_bounds(
    node: tree_sitter::Node,
    source: &[u8],
    source_local_id: usize,
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let mut type_names = Vec::new();
        extract_type_names(child, source, &mut type_names);
        for type_name in type_names {
            if is_user_type(&type_name) {
                let resolved = resolve_type_name(&type_name, import_map);
                relationships.push(ExtractedRelationship {
                    source_local_id,
                    target_qualified_name: resolved,
                    kind: RelationshipKind::Extends,
                });
            }
        }
    }
}

fn extract_trait_method(
    node: tree_sitter::Node,
    source: &[u8],
    trait_name: &str,
    trait_local_id: usize,
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
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
    let visibility = extract_visibility(node, source);

    symbols.push(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: Some(format!("{}()", name)),
        qualified_name: format!("{}.{}", trait_name, name),
        kind: SymbolKind::new("method"),
        visibility,
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: Some(trait_local_id),
        package: String::new(),
        type_text: None,
    });

    extract_param_type_relationships(node, source, local_id, relationships, import_map);
    extract_return_type_relationships(node, source, local_id, relationships, import_map);
}

// --- Impl extraction ---

fn extract_impl(
    node: tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
) {
    let type_node = match node.child_by_field_name("type") {
        Some(n) => n,
        None => return,
    };
    let type_name = match type_node.utf8_text(source) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };

    // Check for trait impl: impl Trait for Type
    let trait_node = node.child_by_field_name("trait");
    if let Some(trait_n) = trait_node {
        let mut trait_names = Vec::new();
        extract_type_names(trait_n, source, &mut trait_names);
        if let Some(trait_type_name) = trait_names.into_iter().next() {
            let resolved = resolve_type_name(&trait_type_name, import_map);
            let type_local_id = symbols.iter()
                .find(|s| s.name == type_name && s.parent_local_id.is_none())
                .map(|s| s.local_id);
            if let Some(id) = type_local_id {
                relationships.push(ExtractedRelationship {
                    source_local_id: id,
                    target_qualified_name: resolved,
                    kind: RelationshipKind::Implements,
                });
            }
        }
    }

    // Extract methods from impl body
    if let Some(body) = node.child_by_field_name("body") {
        let mut body_cursor = body.walk();
        for member in body.children(&mut body_cursor) {
            if member.kind() == "function_item" {
                extract_impl_method(member, source, &type_name, symbols, relationships, import_map);
            }
        }
    }
}

fn extract_impl_method(
    node: tree_sitter::Node,
    source: &[u8],
    type_name: &str,
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
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
    let visibility = extract_visibility(node, source);

    symbols.push(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: Some(format!("{}()", name)),
        qualified_name: format!("{}.{}", type_name, name),
        kind: SymbolKind::new("method"),
        visibility,
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: None,
        package: String::new(),
        type_text: None,
    });

    extract_param_type_relationships(node, source, local_id, relationships, import_map);
    extract_return_type_relationships(node, source, local_id, relationships, import_map);
    extract_call_relationships(node, source, local_id, relationships);
}

// --- Function extraction ---

fn extract_function(
    node: tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
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
    let visibility = extract_visibility(node, source);

    symbols.push(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: Some(format!("{}()", name)),
        qualified_name: name.clone(),
        kind: SymbolKind::new("function"),
        visibility,
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: None,
        package: String::new(),
        type_text: None,
    });

    extract_param_type_relationships(node, source, local_id, relationships, import_map);
    extract_return_type_relationships(node, source, local_id, relationships, import_map);
    extract_call_relationships(node, source, local_id, relationships);
}

// --- Type alias extraction ---

fn extract_type_alias(
    node: tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
) {
    let name = match node.child_by_field_name("name") {
        Some(n) => match n.utf8_text(source) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        },
        None => return,
    };

    let type_node = node.child_by_field_name("type");
    let type_text = type_node.and_then(|n| n.utf8_text(source).ok()).map(|s| s.to_string());

    let start = node.start_position();
    let end = node.end_position();
    let local_id = symbols.len();
    let visibility = extract_visibility(node, source);

    symbols.push(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: None,
        qualified_name: name.clone(),
        kind: SymbolKind::new("type-alias"),
        visibility,
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: None,
        package: String::new(),
        type_text: type_text.clone(),
    });

    if let Some(tn) = type_node {
        let mut type_names = Vec::new();
        extract_type_names(tn, source, &mut type_names);
        for type_name in type_names {
            if is_user_type(&type_name) {
                let resolved = resolve_type_name(&type_name, import_map);
                relationships.push(ExtractedRelationship {
                    source_local_id: local_id,
                    target_qualified_name: resolved,
                    kind: RelationshipKind::FieldType,
                });
            }
        }
    }
}

// --- Relationship helpers ---

fn extract_param_type_relationships(
    node: tree_sitter::Node,
    source: &[u8],
    source_local_id: usize,
    relationships: &mut Vec<ExtractedRelationship>,
    import_map: &HashMap<String, String>,
) {
    let params = match node.child_by_field_name("parameters") {
        Some(p) => p,
        None => return,
    };
    let mut cursor = params.walk();
    for child in params.children(&mut cursor) {
        if child.kind() == "parameter" {
            if let Some(type_node) = child.child_by_field_name("type") {
                let mut type_names = Vec::new();
                extract_type_names(type_node, source, &mut type_names);
                for type_name in type_names {
                    if is_user_type(&type_name) {
                        let resolved = resolve_type_name(&type_name, import_map);
                        relationships.push(ExtractedRelationship {
                            source_local_id,
                            target_qualified_name: resolved,
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
    import_map: &HashMap<String, String>,
) {
    let return_type = match node.child_by_field_name("return_type") {
        Some(r) => r,
        None => return,
    };
    let mut type_names = Vec::new();
    extract_type_names(return_type, source, &mut type_names);
    for type_name in type_names {
        if is_user_type(&type_name) {
            let resolved = resolve_type_name(&type_name, import_map);
            relationships.push(ExtractedRelationship {
                source_local_id,
                target_qualified_name: resolved,
                kind: RelationshipKind::FieldType,
            });
        }
    }
}

fn extract_call_relationships(
    node: tree_sitter::Node,
    source: &[u8],
    source_local_id: usize,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    if let Some(body) = node.child_by_field_name("body") {
        collect_calls(body, source, source_local_id, relationships);
    }
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
                "field_expression" => {
                    // e.g. self.save() → extract "save"
                    func_node.child_by_field_name("field")
                        .and_then(|f| f.utf8_text(source).ok())
                        .map(|s| s.to_string())
                }
                "identifier" => {
                    func_node.utf8_text(source).ok().map(|s| s.to_string())
                }
                "scoped_identifier" => {
                    // e.g. Foo::new() → extract "new"
                    func_node.child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source).ok())
                        .map(|s| s.to_string())
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_extract(source: &str) -> ExtractionResult {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let plugin = RustPlugin;
        plugin.extract_symbols(&tree, source.as_bytes(), Path::new("test.rs"))
    }

    #[test]
    fn test_struct_extraction() {
        let result = parse_and_extract("pub struct Foo {\n    pub name: String,\n    age: u32,\n}");
        assert_eq!(result.symbols.len(), 3);
        assert_eq!(result.symbols[0].name, "Foo");
        assert_eq!(result.symbols[0].kind, SymbolKind::new("struct"));
        assert_eq!(result.symbols[0].visibility, Visibility::new("pub"));
        assert_eq!(result.symbols[1].name, "name");
        assert_eq!(result.symbols[1].kind, SymbolKind::new("field"));
        assert_eq!(result.symbols[1].visibility, Visibility::new("pub"));
        assert_eq!(result.symbols[2].name, "age");
        assert_eq!(result.symbols[2].visibility, Visibility::new("private"));
    }

    #[test]
    fn test_enum_with_variants() {
        let result = parse_and_extract("pub enum Color {\n    Red,\n    Blue { r: u8 },\n}");
        assert_eq!(result.symbols.len(), 4); // enum + 2 variants + 1 field
        assert_eq!(result.symbols[0].name, "Color");
        assert_eq!(result.symbols[0].kind, SymbolKind::new("enum"));
        assert_eq!(result.symbols[1].name, "Red");
        assert_eq!(result.symbols[1].kind, SymbolKind::new("variant"));
        assert_eq!(result.symbols[1].qualified_name, "Color.Red");
        assert_eq!(result.symbols[2].name, "Blue");
        assert_eq!(result.symbols[2].kind, SymbolKind::new("variant"));
        assert_eq!(result.symbols[3].name, "r");
        assert_eq!(result.symbols[3].kind, SymbolKind::new("field"));
        assert_eq!(result.symbols[3].qualified_name, "Color.Blue.r");
    }

    #[test]
    fn test_trait_extraction() {
        let result = parse_and_extract("pub trait Repository {\n    fn save(&self, item: Item);\n    fn find(&self, id: u32) -> Item;\n}");
        assert_eq!(result.symbols.len(), 3);
        assert_eq!(result.symbols[0].name, "Repository");
        assert_eq!(result.symbols[0].kind, SymbolKind::new("trait"));
        assert_eq!(result.symbols[1].name, "save");
        assert_eq!(result.symbols[1].kind, SymbolKind::new("method"));
        assert_eq!(result.symbols[1].qualified_name, "Repository.save");
    }

    #[test]
    fn test_trait_supertraits() {
        let result = parse_and_extract("pub trait ReadWrite: Read + Write {}");
        let extends: Vec<_> = result.relationships.iter()
            .filter(|r| r.kind == RelationshipKind::Extends)
            .collect();
        assert_eq!(extends.len(), 2);
    }

    #[test]
    fn test_impl_method() {
        let result = parse_and_extract("struct Foo {}\nimpl Foo {\n    pub fn bar(&self) {}\n}");
        assert_eq!(result.symbols.len(), 2); // struct + method
        assert_eq!(result.symbols[1].name, "bar");
        assert_eq!(result.symbols[1].kind, SymbolKind::new("method"));
        assert_eq!(result.symbols[1].qualified_name, "Foo.bar");
        assert_eq!(result.symbols[1].visibility, Visibility::new("pub"));
    }

    #[test]
    fn test_trait_impl() {
        let result = parse_and_extract("struct Foo {}\ntrait Bar {}\nimpl Bar for Foo {}");
        let impls: Vec<_> = result.relationships.iter()
            .filter(|r| r.kind == RelationshipKind::Implements)
            .collect();
        assert_eq!(impls.len(), 1);
        assert_eq!(impls[0].target_qualified_name, "Bar");
    }

    #[test]
    fn test_function_extraction() {
        let result = parse_and_extract("pub fn main() {}");
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "main");
        assert_eq!(result.symbols[0].kind, SymbolKind::new("function"));
        assert_eq!(result.symbols[0].visibility, Visibility::new("pub"));
    }

    #[test]
    fn test_type_alias() {
        let result = parse_and_extract("pub type Result<T> = std::result::Result<T, MyError>;");
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "Result");
        assert_eq!(result.symbols[0].kind, SymbolKind::new("type-alias"));
    }

    #[test]
    fn test_field_type_relationship() {
        let result = parse_and_extract("struct Service {\n    repo: Repository,\n}");
        let field_types: Vec<_> = result.relationships.iter()
            .filter(|r| r.kind == RelationshipKind::FieldType)
            .collect();
        assert_eq!(field_types.len(), 1);
        assert_eq!(field_types[0].target_qualified_name, "Repository");
    }

    #[test]
    fn test_param_type_relationship() {
        let result = parse_and_extract("fn process(r: Repository) {}");
        let field_types: Vec<_> = result.relationships.iter()
            .filter(|r| r.kind == RelationshipKind::FieldType)
            .collect();
        assert_eq!(field_types.len(), 1);
        assert_eq!(field_types[0].target_qualified_name, "Repository");
    }

    #[test]
    fn test_return_type_relationship() {
        let result = parse_and_extract("fn new_service() -> Service { todo!() }");
        let field_types: Vec<_> = result.relationships.iter()
            .filter(|r| r.kind == RelationshipKind::FieldType)
            .collect();
        assert_eq!(field_types.len(), 1);
        assert_eq!(field_types[0].target_qualified_name, "Service");
    }

    #[test]
    fn test_call_relationship() {
        let result = parse_and_extract("fn main() {\n    process();\n}");
        let calls: Vec<_> = result.relationships.iter()
            .filter(|r| r.kind == RelationshipKind::Calls)
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].target_qualified_name, "process");
    }

    #[test]
    fn test_use_import_resolution() {
        let result = parse_and_extract("use crate::model::Repository;\nstruct Service {\n    repo: Repository,\n}");
        let field_types: Vec<_> = result.relationships.iter()
            .filter(|r| r.kind == RelationshipKind::FieldType)
            .collect();
        assert_eq!(field_types.len(), 1);
        assert_eq!(field_types[0].target_qualified_name, "crate::model::Repository");
    }

    #[test]
    fn test_use_alias() {
        let result = parse_and_extract("use std::io::Result as IoResult;\nfn foo() -> IoResult { todo!() }");
        let field_types: Vec<_> = result.relationships.iter()
            .filter(|r| r.kind == RelationshipKind::FieldType)
            .collect();
        assert_eq!(field_types.len(), 1);
        assert_eq!(field_types[0].target_qualified_name, "std::io::Result");
    }

    #[test]
    fn test_pub_crate_visibility() {
        let result = parse_and_extract("pub(crate) struct Foo {}");
        assert_eq!(result.symbols[0].visibility, Visibility::new("pub(crate)"));
    }

    #[test]
    fn test_reference_type_stripped() {
        let result = parse_and_extract("fn process(r: &Repository) {}");
        let field_types: Vec<_> = result.relationships.iter()
            .filter(|r| r.kind == RelationshipKind::FieldType)
            .collect();
        assert_eq!(field_types.len(), 1);
        assert_eq!(field_types[0].target_qualified_name, "Repository");
    }
}
