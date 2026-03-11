use serde::Serialize;

pub type FileId = i64;
pub type SymbolId = i64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SymbolKind(String);

impl SymbolKind {
    pub fn new(s: &str) -> Self {
        SymbolKind(s.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Visibility(String);

impl Visibility {
    pub fn new(s: &str) -> Self {
        Visibility(s.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginCapability {
    Rename,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenameOccurrence {
    pub line: i64,
    pub column: i64,
    pub byte_offset: usize,
    pub old_text: String,
}

#[derive(Debug, Clone)]
pub enum RenameError {
    NotSupported { language: String },
}

impl std::fmt::Display for RenameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenameError::NotSupported { language } => {
                write!(f, "Rename is not supported for {} files", language)
            }
        }
    }
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
        assert_eq!(SymbolKind::new("class").as_str(), "class");
        assert_eq!(SymbolKind::new("method").as_str(), "method");
    }

    #[test]
    fn test_relationship_kind_display() {
        assert_eq!(RelationshipKind::Extends.as_str(), "extends");
        assert_eq!(RelationshipKind::Implements.as_str(), "implements");
    }

    #[test]
    fn test_visibility_display() {
        assert_eq!(Visibility::new("public").as_str(), "public");
        assert_eq!(Visibility::new("package-private").as_str(), "package-private");
    }

}
