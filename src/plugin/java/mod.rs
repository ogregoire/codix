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
            if let Some(symbol) = extract_type_declaration(child, source, &package, symbols.len()) {
                symbols.push(symbol);
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
}
