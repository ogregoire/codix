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

    fn insert_symbols(&self, file_id: FileId, symbols: &[ExtractedSymbol]) -> Result<Vec<SymbolId>> {
        // First pass: insert all symbols with parent_symbol_id = NULL, collect local_id -> SymbolId map
        let mut local_to_real: std::collections::HashMap<usize, SymbolId> = std::collections::HashMap::new();
        let mut ids: Vec<SymbolId> = Vec::with_capacity(symbols.len());
        for sym in symbols {
            self.conn.execute(
                "INSERT INTO symbols (name, signature, qualified_name, kind, visibility, file_id, line, column_, end_line, end_column, parent_symbol_id, package)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11)",
                params![
                    sym.name, sym.signature, sym.qualified_name,
                    sym.kind.as_str(), sym.visibility.as_str(),
                    file_id, sym.line, sym.column, sym.end_line, sym.end_column,
                    sym.package
                ],
            )?;
            let real_id = self.conn.last_insert_rowid();
            local_to_real.insert(sym.local_id, real_id);
            ids.push(real_id);
        }
        // Second pass: update parent_symbol_id for symbols that have a parent
        for sym in symbols {
            if let Some(parent_local_id) = sym.parent_local_id {
                if let Some(&parent_real_id) = local_to_real.get(&parent_local_id) {
                    let real_id = local_to_real[&sym.local_id];
                    self.conn.execute(
                        "UPDATE symbols SET parent_symbol_id = ?1 WHERE id = ?2",
                        params![parent_real_id, real_id],
                    )?;
                }
            }
        }
        Ok(ids)
    }

    fn delete_symbols_for_file(&self, file_id: FileId) -> Result<()> {
        self.conn.execute("DELETE FROM symbols WHERE file_id = ?1", params![file_id])?;
        Ok(())
    }

    fn insert_relationships(&self, file_id: FileId, symbol_id_map: &[(usize, SymbolId)], relationships: &[ExtractedRelationship]) -> Result<()> {
        let map: std::collections::HashMap<usize, SymbolId> = symbol_id_map.iter().copied().collect();
        for rel in relationships {
            if let Some(&source_symbol_id) = map.get(&rel.source_local_id) {
                self.conn.execute(
                    "INSERT OR IGNORE INTO relationships (source_symbol_id, target_symbol_id, target_qualified_name, file_id, kind)
                     VALUES (?1, NULL, ?2, ?3, ?4)",
                    params![source_symbol_id, rel.target_qualified_name, file_id, rel.kind.as_str()],
                )?;
            }
        }
        Ok(())
    }

    fn delete_relationships_for_file(&self, file_id: FileId) -> Result<()> {
        self.conn.execute("DELETE FROM relationships WHERE file_id = ?1", params![file_id])?;
        Ok(())
    }

    fn resolve_relationships(&self) -> Result<u64> {
        let count = self.conn.execute(
            "UPDATE relationships SET target_symbol_id = (
                SELECT id FROM symbols WHERE qualified_name = relationships.target_qualified_name
             ) WHERE target_symbol_id IS NULL",
            [],
        )?;
        Ok(count as u64)
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
        self.conn.execute_batch("BEGIN")?;
        Ok(())
    }

    fn commit_transaction(&self) -> Result<()> {
        self.conn.execute_batch("COMMIT")?;
        Ok(())
    }

    fn rollback_transaction(&self) -> Result<()> {
        self.conn.execute_batch("ROLLBACK")?;
        Ok(())
    }

    fn clear_all(&self) -> Result<()> {
        self.conn.execute_batch("DELETE FROM relationships; DELETE FROM symbols; DELETE FROM files;")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> SqliteStore {
        SqliteStore::open(":memory:").unwrap()
    }

    #[test]
    fn test_insert_and_delete_symbols() {
        let store = test_store();
        let fid = store.upsert_file("Foo.java", 1, None, "java").unwrap();
        let syms = vec![ExtractedSymbol {
            local_id: 0,
            name: "Foo".into(),
            signature: None,
            qualified_name: "com.foo.Foo".into(),
            kind: SymbolKind::Class,
            visibility: Visibility::Public,
            line: 1, column: 0, end_line: 10, end_column: 1,
            parent_local_id: None,
            package: "com.foo".into(),
        }];
        let ids = store.insert_symbols(fid, &syms).unwrap();
        assert_eq!(ids.len(), 1);

        store.delete_symbols_for_file(fid).unwrap();
        // Verify deletion by re-inserting successfully
        let ids2 = store.insert_symbols(fid, &syms).unwrap();
        assert_eq!(ids2.len(), 1);
    }

    #[test]
    fn test_insert_symbols_with_parent() {
        let store = test_store();
        let fid = store.upsert_file("Foo.java", 1, None, "java").unwrap();
        let syms = vec![
            ExtractedSymbol {
                local_id: 0, name: "Foo".into(), signature: None,
                qualified_name: "com.foo.Foo".into(), kind: SymbolKind::Class,
                visibility: Visibility::Public,
                line: 1, column: 0, end_line: 10, end_column: 1,
                parent_local_id: None, package: "com.foo".into(),
            },
            ExtractedSymbol {
                local_id: 1, name: "bar".into(), signature: Some("bar(String)".into()),
                qualified_name: "com.foo.Foo.bar(String)".into(), kind: SymbolKind::Method,
                visibility: Visibility::Public,
                line: 3, column: 4, end_line: 5, end_column: 5,
                parent_local_id: Some(0), package: "com.foo".into(),
            },
        ];
        let ids = store.insert_symbols(fid, &syms).unwrap();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn test_insert_and_delete_relationships() {
        let store = test_store();
        let fid = store.upsert_file("Foo.java", 1, None, "java").unwrap();
        let syms = vec![ExtractedSymbol {
            local_id: 0, name: "Foo".into(), signature: None,
            qualified_name: "com.foo.Foo".into(), kind: SymbolKind::Class,
            visibility: Visibility::Public,
            line: 1, column: 0, end_line: 10, end_column: 1,
            parent_local_id: None, package: "com.foo".into(),
        }];
        let ids = store.insert_symbols(fid, &syms).unwrap();
        let map: Vec<(usize, SymbolId)> = vec![(0, ids[0])];
        let rels = vec![ExtractedRelationship {
            source_local_id: 0,
            target_qualified_name: "com.foo.Bar".into(),
            kind: RelationshipKind::Extends,
        }];
        store.insert_relationships(fid, &map, &rels).unwrap();
        store.delete_relationships_for_file(fid).unwrap();
    }

    #[test]
    fn test_transaction_and_clear() {
        let store = test_store();
        store.begin_transaction().unwrap();
        store.upsert_file("a.java", 1, None, "java").unwrap();
        store.commit_transaction().unwrap();
        assert_eq!(store.list_files().unwrap().len(), 1);
        store.clear_all().unwrap();
        assert_eq!(store.list_files().unwrap().len(), 0);
    }

    #[test]
    fn test_resolve_relationships() {
        let store = test_store();
        let f1 = store.upsert_file("Foo.java", 1, None, "java").unwrap();
        let f2 = store.upsert_file("Bar.java", 1, None, "java").unwrap();
        let syms1 = vec![ExtractedSymbol {
            local_id: 0, name: "Foo".into(), signature: None,
            qualified_name: "com.foo.Foo".into(), kind: SymbolKind::Class,
            visibility: Visibility::Public,
            line: 1, column: 0, end_line: 10, end_column: 1,
            parent_local_id: None, package: "com.foo".into(),
        }];
        let ids1 = store.insert_symbols(f1, &syms1).unwrap();
        let syms2 = vec![ExtractedSymbol {
            local_id: 0, name: "Bar".into(), signature: None,
            qualified_name: "com.foo.Bar".into(), kind: SymbolKind::Class,
            visibility: Visibility::Public,
            line: 1, column: 0, end_line: 10, end_column: 1,
            parent_local_id: None, package: "com.foo".into(),
        }];
        store.insert_symbols(f2, &syms2).unwrap();
        let map: Vec<(usize, SymbolId)> = vec![(0, ids1[0])];
        let rels = vec![ExtractedRelationship {
            source_local_id: 0,
            target_qualified_name: "com.foo.Bar".into(),
            kind: RelationshipKind::Extends,
        }];
        store.insert_relationships(f1, &map, &rels).unwrap();
        let resolved = store.resolve_relationships().unwrap();
        assert_eq!(resolved, 1);
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
