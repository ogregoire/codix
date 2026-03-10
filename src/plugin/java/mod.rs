use std::path::Path;
use crate::model::*;
use super::LanguagePlugin;

pub struct JavaPlugin;

impl LanguagePlugin for JavaPlugin {
    fn name(&self) -> &str {
        "java"
    }

    fn file_extensions(&self) -> &[&str] {
        &["java"]
    }

    fn symbol_kinds(&self) -> &[&str] {
        &["class", "interface", "enum", "record", "annotation", "method", "field", "constructor"]
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

        // Walk top-level children for type declarations
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if let Some(type_symbol) = extract_type_declaration(child, source, &package, symbols.len()) {
                let type_local_id = type_symbol.local_id;
                let type_qualified_name = type_symbol.qualified_name.clone();
                let type_package = type_symbol.package.clone();
                symbols.push(type_symbol);

                // Extract members from the class body
                extract_members(child, source, &type_qualified_name, &type_package, type_local_id, &mut symbols);
            }
        }

        ExtractionResult {
            symbols,
            relationships: Vec::new(),
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
    })
}

fn extract_members(
    type_node: tree_sitter::Node,
    source: &[u8],
    parent_qualified_name: &str,
    package: &str,
    parent_local_id: usize,
    symbols: &mut Vec<ExtractedSymbol>,
) {
    let body_kind = match type_node.kind() {
        "class_declaration" | "record_declaration" => "class_body",
        "interface_declaration" => "interface_body",
        "enum_declaration" => "enum_body",
        _ => return,
    };

    let mut cursor = type_node.walk();
    for child in type_node.children(&mut cursor) {
        if child.kind() == body_kind {
            let mut body_cursor = child.walk();
            for member in child.children(&mut body_cursor) {
                let local_id = symbols.len();
                let maybe_symbol = match member.kind() {
                    "method_declaration" => extract_method(member, source, parent_qualified_name, package, parent_local_id, local_id, SymbolKind::Method),
                    "constructor_declaration" => extract_method(member, source, parent_qualified_name, package, parent_local_id, local_id, SymbolKind::Constructor),
                    "field_declaration" => extract_field(member, source, parent_qualified_name, package, parent_local_id, local_id),
                    _ => None,
                };
                if let Some(symbol) = maybe_symbol {
                    symbols.push(symbol);
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
    })
}

fn extract_field(
    node: tree_sitter::Node,
    source: &[u8],
    parent_qualified_name: &str,
    package: &str,
    parent_local_id: usize,
    local_id: usize,
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
    })
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
    fn test_method_overloads() {
        let source = "package com.foo;\npublic class Svc {\n  void save(Person p) {}\n  void save(String s, int i) {}\n}";
        let result = parse_java(source);
        let methods: Vec<_> = result.symbols.iter().filter(|s| s.kind == SymbolKind::Method).collect();
        assert_eq!(methods.len(), 2);
        assert_eq!(methods[0].signature, Some("save(Person)".to_string()));
        assert_eq!(methods[1].signature, Some("save(String,int)".to_string()));
        assert_ne!(methods[0].qualified_name, methods[1].qualified_name);
    }
}
