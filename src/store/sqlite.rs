use anyhow::Result;
use rusqlite::{Connection, params};
use crate::model::*;
use crate::store::Store;

pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                mtime INTEGER NOT NULL,
                hash TEXT,
                language TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS symbols (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                signature TEXT,
                qualified_name TEXT NOT NULL,
                kind TEXT NOT NULL,
                visibility TEXT NOT NULL,
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                line INTEGER NOT NULL,
                column_ INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                end_column INTEGER NOT NULL,
                parent_symbol_id INTEGER REFERENCES symbols(id) ON DELETE SET NULL,
                package TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS relationships (
                source_symbol_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
                target_symbol_id INTEGER REFERENCES symbols(id) ON DELETE CASCADE,
                target_qualified_name TEXT NOT NULL,
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                kind TEXT NOT NULL,
                PRIMARY KEY (source_symbol_id, target_qualified_name, kind)
            );
            CREATE INDEX IF NOT EXISTS idx_symbols_file_id ON symbols(file_id);
            CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_qualified_name ON symbols(qualified_name);
            CREATE INDEX IF NOT EXISTS idx_symbols_package ON symbols(package);
            CREATE INDEX IF NOT EXISTS idx_relationships_file_id ON relationships(file_id);
            CREATE INDEX IF NOT EXISTS idx_relationships_target ON relationships(target_symbol_id);
            CREATE INDEX IF NOT EXISTS idx_relationships_source ON relationships(source_symbol_id);
        ")?;
        Ok(SqliteStore { conn })
    }
}

impl Store for SqliteStore {
    fn upsert_file(&self, path: &str, mtime: i64, hash: Option<&str>, language: &str) -> Result<FileId> {
        self.conn.execute(
            "INSERT INTO files (path, mtime, hash, language) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET mtime=excluded.mtime, hash=excluded.hash, language=excluded.language",
            params![path, mtime, hash, language],
        )?;
        let id: i64 = self.conn.query_row(
            "SELECT id FROM files WHERE path = ?1",
            params![path],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    fn get_file(&self, path: &str) -> Result<Option<FileRecord>> {
        let result = self.conn.query_row(
            "SELECT id, path, mtime, hash, language FROM files WHERE path = ?1",
            params![path],
            |row| Ok(FileRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                mtime: row.get(2)?,
                hash: row.get(3)?,
                language: row.get(4)?,
            }),
        );
        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn list_files(&self) -> Result<Vec<FileRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, mtime, hash, language FROM files"
        )?;
        let files = stmt.query_map([], |row| Ok(FileRecord {
            id: row.get(0)?,
            path: row.get(1)?,
            mtime: row.get(2)?,
            hash: row.get(3)?,
            language: row.get(4)?,
        }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(files)
    }

    fn delete_file(&self, file_id: FileId) -> Result<()> {
        self.conn.execute("DELETE FROM files WHERE id = ?1", params![file_id])?;
        Ok(())
    }

    fn insert_symbols(&self, _file_id: FileId, _symbols: &[ExtractedSymbol]) -> Result<Vec<SymbolId>> {
        todo!()
    }

    fn delete_symbols_for_file(&self, _file_id: FileId) -> Result<()> {
        todo!()
    }

    fn insert_relationships(&self, _file_id: FileId, _symbol_id_map: &[(usize, SymbolId)], _relationships: &[ExtractedRelationship]) -> Result<()> {
        todo!()
    }

    fn delete_relationships_for_file(&self, _file_id: FileId) -> Result<()> {
        todo!()
    }

    fn resolve_relationships(&self) -> Result<u64> {
        todo!()
    }

    fn find_symbol(&self, _query: &SymbolQuery) -> Result<Vec<Symbol>> {
        todo!()
    }

    fn find_references(&self, _symbol_id: SymbolId) -> Result<Vec<Symbol>> {
        todo!()
    }

    fn find_implementations(&self, _symbol_id: SymbolId) -> Result<Vec<Symbol>> {
        todo!()
    }

    fn find_supertypes(&self, _symbol_id: SymbolId) -> Result<Vec<Symbol>> {
        todo!()
    }

    fn find_callers(&self, _symbol_id: SymbolId) -> Result<Vec<Symbol>> {
        todo!()
    }

    fn find_callees(&self, _symbol_id: SymbolId) -> Result<Vec<Symbol>> {
        todo!()
    }

    fn symbols_in_file(&self, _file_path: &str) -> Result<Vec<Symbol>> {
        todo!()
    }

    fn symbols_in_package(&self, _package: &str, _query: &SymbolQuery) -> Result<Vec<Symbol>> {
        todo!()
    }

    fn begin_transaction(&self) -> Result<()> {
        todo!()
    }

    fn commit_transaction(&self) -> Result<()> {
        todo!()
    }

    fn rollback_transaction(&self) -> Result<()> {
        todo!()
    }

    fn clear_all(&self) -> Result<()> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> SqliteStore {
        SqliteStore::open(":memory:").unwrap()
    }

    #[test]
    fn test_upsert_and_get_file() {
        let store = test_store();
        let id = store.upsert_file("src/Foo.java", 1000, None, "java").unwrap();
        let file = store.get_file("src/Foo.java").unwrap().unwrap();
        assert_eq!(file.id, id);
        assert_eq!(file.path, "src/Foo.java");
        assert_eq!(file.mtime, 1000);
        assert_eq!(file.language, "java");
    }

    #[test]
    fn test_upsert_file_updates_mtime() {
        let store = test_store();
        let id1 = store.upsert_file("src/Foo.java", 1000, None, "java").unwrap();
        let id2 = store.upsert_file("src/Foo.java", 2000, None, "java").unwrap();
        assert_eq!(id1, id2);
        let file = store.get_file("src/Foo.java").unwrap().unwrap();
        assert_eq!(file.mtime, 2000);
    }

    #[test]
    fn test_list_files() {
        let store = test_store();
        store.upsert_file("a.java", 1, None, "java").unwrap();
        store.upsert_file("b.java", 2, None, "java").unwrap();
        let files = store.list_files().unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_delete_file() {
        let store = test_store();
        let id = store.upsert_file("a.java", 1, None, "java").unwrap();
        store.delete_file(id).unwrap();
        assert!(store.get_file("a.java").unwrap().is_none());
    }
}
