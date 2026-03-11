use super::LanguagePlugin;
use crate::model::*;
use std::path::Path;

pub struct JsPlugin;

const JS_EXTENSIONS: &[&str] = &["js", "mjs", "cjs", "jsx", "ts", "tsx"];

impl LanguagePlugin for JsPlugin {
    fn name(&self) -> &str {
        "js"
    }
    fn display_name(&self) -> &str {
        "JavaScript"
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| JS_EXTENSIONS.contains(&e))
            .unwrap_or(false)
    }

    fn tree_sitter_language(&self) -> tree_sitter::Language {
        tree_sitter_typescript::LANGUAGE_TSX.into()
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

        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            match child.kind() {
                "class_declaration" | "abstract_class_declaration" => {
                    extract_class(child, source, &mut symbols, &mut relationships);
                }
                "function_declaration" => {
                    if let Some(sym) = extract_function_declaration(child, source, symbols.len()) {
                        symbols.push(sym);
                    }
                }
                "lexical_declaration" | "variable_declaration" => {
                    extract_variable_functions(child, source, &mut symbols);
                }
                "export_statement" => {
                    extract_exported(child, source, &mut symbols, &mut relationships);
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

fn extract_exported(
    node: tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<ExtractedSymbol>,
    relationships: &mut Vec<ExtractedRelationship>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "class_declaration" | "abstract_class_declaration" => {
                extract_class(child, source, symbols, relationships);
            }
            "function_declaration" => {
                if let Some(sym) = extract_function_declaration(child, source, symbols.len()) {
                    symbols.push(sym);
                }
            }
            "lexical_declaration" | "variable_declaration" => {
                extract_variable_functions(child, source, symbols);
            }
            _ => {}
        }
    }
}

fn extract_class(
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
    let class_local_id = symbols.len();

    symbols.push(ExtractedSymbol {
        local_id: class_local_id,
        name: name.clone(),
        signature: None,
        qualified_name: name.clone(),
        kind: SymbolKind::new("class"),
        visibility: Visibility::new("public"),
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: None,
        package: String::new(),
        type_text: None,
    });

    // Extract extends relationship (class_heritage → extends_clause → identifier)
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "class_heritage" {
            let mut heritage_cursor = child.walk();
            for heritage_child in child.children(&mut heritage_cursor) {
                if heritage_child.kind() == "extends_clause" {
                    if let Some(type_node) = heritage_child.child(1) {
                        if let Ok(text) = type_node.utf8_text(source) {
                            relationships.push(ExtractedRelationship {
                                source_local_id: class_local_id,
                                target_qualified_name: text.to_string(),
                                kind: RelationshipKind::Extends,
                            });
                        }
                    }
                }
            }
        }
    }

    // Extract class body members
    if let Some(body) = node.child_by_field_name("body") {
        let mut body_cursor = body.walk();
        for member in body.children(&mut body_cursor) {
            match member.kind() {
                "method_definition" => {
                    if let Some(sym) =
                        extract_method(member, source, &name, class_local_id, symbols.len())
                    {
                        symbols.push(sym);
                    }
                }
                "public_field_definition" => {
                    if let Some(sym) =
                        extract_field(member, source, &name, class_local_id, symbols.len())
                    {
                        symbols.push(sym);
                    }
                }
                _ => {}
            }
        }
    }
}

fn extract_function_declaration(
    node: tree_sitter::Node,
    source: &[u8],
    local_id: usize,
) -> Option<ExtractedSymbol> {
    let name = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(source).ok())?
        .to_string();

    let start = node.start_position();
    let end = node.end_position();

    Some(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: Some(format!("{}()", name)),
        qualified_name: name,
        kind: SymbolKind::new("function"),
        visibility: Visibility::new("public"),
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: None,
        package: String::new(),
        type_text: None,
    })
}

fn extract_variable_functions(
    node: tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<ExtractedSymbol>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            let value = child.child_by_field_name("value");
            let is_function = value
                .map(|v| matches!(v.kind(), "arrow_function" | "function_expression"))
                .unwrap_or(false);
            if !is_function {
                continue;
            }
            let name = child
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok());
            if let Some(name) = name {
                let start = node.start_position();
                let end = node.end_position();
                symbols.push(ExtractedSymbol {
                    local_id: symbols.len(),
                    name: name.to_string(),
                    signature: Some(format!("{}()", name)),
                    qualified_name: name.to_string(),
                    kind: SymbolKind::new("function"),
                    visibility: Visibility::new("public"),
                    line: (start.row + 1) as i64,
                    column: start.column as i64,
                    end_line: (end.row + 1) as i64,
                    end_column: end.column as i64,
                    parent_local_id: None,
                    package: String::new(),
                    type_text: None,
                });
            }
        }
    }
}

fn extract_method(
    node: tree_sitter::Node,
    source: &[u8],
    class_name: &str,
    parent_local_id: usize,
    local_id: usize,
) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(source).ok()?.to_string();
    let is_private = name.starts_with('#');

    let is_constructor = name == "constructor";
    let kind = if is_constructor {
        SymbolKind::new("constructor")
    } else {
        SymbolKind::new("method")
    };

    let display_name = if is_private { &name[1..] } else { &name };
    let qualified_name = format!("{}.{}", class_name, display_name);

    let start = node.start_position();
    let end = node.end_position();

    Some(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: Some(format!("{}()", name)),
        qualified_name,
        kind,
        visibility: if is_private {
            Visibility::new("private")
        } else {
            Visibility::new("public")
        },
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: Some(parent_local_id),
        package: String::new(),
        type_text: None,
    })
}

fn extract_field(
    node: tree_sitter::Node,
    source: &[u8],
    class_name: &str,
    parent_local_id: usize,
    local_id: usize,
) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(source).ok()?.to_string();
    let is_private = name.starts_with('#');

    let display_name = if is_private { &name[1..] } else { &name };
    let qualified_name = format!("{}.{}", class_name, display_name);

    let start = node.start_position();
    let end = node.end_position();

    Some(ExtractedSymbol {
        local_id,
        name: name.clone(),
        signature: None,
        qualified_name,
        kind: SymbolKind::new("field"),
        visibility: if is_private {
            Visibility::new("private")
        } else {
            Visibility::new("public")
        },
        line: (start.row + 1) as i64,
        column: start.column as i64,
        end_line: (end.row + 1) as i64,
        end_column: end.column as i64,
        parent_local_id: Some(parent_local_id),
        package: String::new(),
        type_text: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_extract(source: &str) -> ExtractionResult {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let plugin = JsPlugin;
        plugin.extract_symbols(&tree, source.as_bytes(), Path::new("test.js"))
    }

    #[test]
    fn test_class_extraction() {
        let result = parse_and_extract(
            "class Foo {\n  #name;\n  constructor(name) { this.#name = name; }\n  run() {}\n}",
        );
        assert_eq!(result.symbols.len(), 4);
        assert_eq!(result.symbols[0].name, "Foo");
        assert_eq!(result.symbols[0].kind, SymbolKind::new("class"));
        assert_eq!(result.symbols[1].name, "#name");
        assert_eq!(result.symbols[1].kind, SymbolKind::new("field"));
        assert_eq!(result.symbols[1].visibility, Visibility::new("private"));
        assert_eq!(result.symbols[2].name, "constructor");
        assert_eq!(result.symbols[2].kind, SymbolKind::new("constructor"));
        assert_eq!(result.symbols[3].name, "run");
        assert_eq!(result.symbols[3].kind, SymbolKind::new("method"));
        assert_eq!(result.symbols[3].qualified_name, "Foo.run");
    }

    #[test]
    fn test_function_declaration() {
        let result = parse_and_extract("function main() { return 42; }");
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "main");
        assert_eq!(result.symbols[0].kind, SymbolKind::new("function"));
        assert_eq!(result.symbols[0].signature, Some("main()".to_string()));
    }

    #[test]
    fn test_arrow_function() {
        let result = parse_and_extract("const helper = () => { return 42; };");
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "helper");
        assert_eq!(result.symbols[0].kind, SymbolKind::new("function"));
    }

    #[test]
    fn test_function_expression() {
        let result = parse_and_extract("const helper = function() { return 42; };");
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "helper");
        assert_eq!(result.symbols[0].kind, SymbolKind::new("function"));
    }

    #[test]
    fn test_private_field() {
        let result = parse_and_extract("class Foo { #secret = 42; }");
        assert_eq!(result.symbols.len(), 2);
        assert_eq!(result.symbols[1].name, "#secret");
        assert_eq!(result.symbols[1].visibility, Visibility::new("private"));
        assert_eq!(result.symbols[1].qualified_name, "Foo.secret");
    }

    #[test]
    fn test_extends_relationship() {
        let result = parse_and_extract("class Bar extends Foo {}");
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.relationships.len(), 1);
        assert_eq!(result.relationships[0].target_qualified_name, "Foo");
        assert_eq!(result.relationships[0].kind, RelationshipKind::Extends);
    }

    #[test]
    fn test_exported_class() {
        let result = parse_and_extract("export class Foo { run() {} }");
        assert_eq!(result.symbols.len(), 2);
        assert_eq!(result.symbols[0].name, "Foo");
        assert_eq!(result.symbols[1].name, "run");
    }

    #[test]
    fn test_exported_function() {
        let result = parse_and_extract("export function main() {}");
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "main");
        assert_eq!(result.symbols[0].kind, SymbolKind::new("function"));
    }

    #[test]
    fn test_typescript_file() {
        let result =
            parse_and_extract("class Service {\n  private name: string;\n  serve(): void {}\n}");
        assert_eq!(result.symbols.len(), 3);
        assert_eq!(result.symbols[0].name, "Service");
    }
}
