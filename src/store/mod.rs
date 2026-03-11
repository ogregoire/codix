use crate::model::*;
use anyhow::Result;
use std::collections::BTreeMap;

pub mod sqlite;

#[derive(Debug, Default)]
pub struct LanguageStats {
    pub files: u64,
    pub symbols: BTreeMap<String, u64>,
    pub relationships: BTreeMap<String, u64>,
    pub unresolved: u64,
}

pub trait Store {
    fn upsert_file(
        &self,
        path: &str,
        mtime: i64,
        hash: Option<&str>,
        language: &str,
    ) -> Result<FileId>;
    fn get_file(&self, path: &str) -> Result<Option<FileRecord>>;
    fn list_files(&self) -> Result<Vec<FileRecord>>;
    fn delete_file(&self, file_id: FileId) -> Result<()>;

    fn insert_symbols(&self, file_id: FileId, symbols: &[ExtractedSymbol])
        -> Result<Vec<SymbolId>>;
    fn delete_symbols_for_file(&self, file_id: FileId) -> Result<()>;

    fn insert_relationships(
        &self,
        file_id: FileId,
        symbol_id_map: &[(usize, SymbolId)],
        relationships: &[ExtractedRelationship],
    ) -> Result<()>;
    fn delete_relationships_for_file(&self, file_id: FileId) -> Result<()>;
    fn resolve_relationships(&self) -> Result<u64>;
    fn resolve_wildcard_imports(&self, file_id: FileId, prefixes: &[String]) -> Result<u64>;

    fn find_symbol(&self, query: &SymbolQuery) -> Result<Vec<Symbol>>;
    fn find_references(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>>;
    fn find_implementations(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>>;
    fn find_supertypes(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>>;
    fn find_callers(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>>;
    fn find_callees(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>>;
    fn symbols_in_file(&self, file_path: &str) -> Result<Vec<Symbol>>;
    fn symbols_in_package(&self, package: &str, query: &SymbolQuery) -> Result<Vec<Symbol>>;

    fn update_symbol_name(
        &self,
        symbol_id: SymbolId,
        new_name: &str,
        new_qualified_name: &str,
        new_signature: Option<&str>,
    ) -> Result<()>;
    fn update_child_qualified_names(
        &self,
        parent_symbol_id: SymbolId,
        old_prefix: &str,
        new_prefix: &str,
    ) -> Result<()>;
    fn update_relationship_targets(
        &self,
        old_qualified_name: &str,
        new_qualified_name: &str,
    ) -> Result<()>;
    fn update_file_mtime(&self, file_id: FileId, new_mtime: i64) -> Result<()>;

    fn index_stats(&self) -> Result<BTreeMap<String, LanguageStats>>;

    fn begin_transaction(&self) -> Result<()>;
    fn commit_transaction(&self) -> Result<()>;
    fn clear_all(&self) -> Result<()>;
}
