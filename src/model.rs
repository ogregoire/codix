use serde::Serialize;

pub type FileId = i64;
pub type SymbolId = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SymbolKind {
    Class,
    Interface,
    Enum,
    Record,
    Annotation,
    Method,
    Field,
    Constructor,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::Enum => "enum",
            SymbolKind::Record => "record",
            SymbolKind::Annotation => "annotation",
            SymbolKind::Method => "method",
            SymbolKind::Field => "field",
            SymbolKind::Constructor => "constructor",
        }
    }

    pub fn parse_kind(s: &str) -> Option<SymbolKind> {
        match s {
            "class" => Some(SymbolKind::Class),
            "interface" => Some(SymbolKind::Interface),
            "enum" => Some(SymbolKind::Enum),
            "record" => Some(SymbolKind::Record),
            "annotation" => Some(SymbolKind::Annotation),
            "method" => Some(SymbolKind::Method),
            "field" => Some(SymbolKind::Field),
            "constructor" => Some(SymbolKind::Constructor),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Visibility {
    Public,
    Protected,
    PackagePrivate,
    Private,
}

impl Visibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Protected => "protected",
            Visibility::PackagePrivate => "package-private",
            Visibility::Private => "private",
        }
    }

    pub fn parse_visibility(s: &str) -> Option<Visibility> {
        match s {
            "public" => Some(Visibility::Public),
            "protected" => Some(Visibility::Protected),
            "package-private" => Some(Visibility::PackagePrivate),
            "private" => Some(Visibility::Private),
            _ => None,
        }
    }
}

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

#[derive(Debug, Clone, Serialize)]
pub struct FileRecord {
    pub id: FileId,
    pub path: String,
    pub mtime: i64,
    pub hash: Option<String>,
    pub language: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub signature: Option<String>,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub visibility: Visibility,
    pub file_id: FileId,
    pub file_path: String,
    pub line: i64,
    pub column: i64,
    pub end_line: i64,
    pub end_column: i64,
    pub parent_symbol_id: Option<SymbolId>,
    pub package: String,
}

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

#[derive(Debug, Clone, Serialize)]
pub struct ExtractedRelationship {
    pub source_local_id: usize,
    pub target_qualified_name: String,
    pub kind: RelationshipKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtractionResult {
    pub symbols: Vec<ExtractedSymbol>,
    pub relationships: Vec<ExtractedRelationship>,
    pub wildcard_imports: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SymbolQuery {
    pub pattern: String,
    pub case_insensitive: bool,
    pub kind: Option<SymbolKind>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_kind_display() {
        assert_eq!(SymbolKind::Class.as_str(), "class");
        assert_eq!(SymbolKind::Method.as_str(), "method");
    }

    #[test]
    fn test_relationship_kind_display() {
        assert_eq!(RelationshipKind::Extends.as_str(), "extends");
        assert_eq!(RelationshipKind::Implements.as_str(), "implements");
    }

    #[test]
    fn test_visibility_display() {
        assert_eq!(Visibility::Public.as_str(), "public");
        assert_eq!(Visibility::PackagePrivate.as_str(), "package-private");
    }

}
