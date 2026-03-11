use anyhow::Result;
use rusqlite::{Connection, params};
use crate::model::*;
use crate::store::Store;

fn glob_to_like(pattern: &str) -> String {
    let mut like = String::with_capacity(pattern.len());
    for ch in pattern.chars() {
        match ch {
            '*' => like.push('%'),
            '?' => like.push('_'),
            '%' => like.push_str("\\%"),
            '_' => like.push_str("\\_"),
            c => like.push(c),
        }
    }
    like
}

pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    fn symbol_from_row(row: &rusqlite::Row) -> rusqlite::Result<Symbol> {
        let kind_str: String = row.get(3)?;
        let vis_str: String = row.get(5)?;
        Ok(Symbol {
            id: row.get(0)?,
            name: row.get(1)?,
            signature: row.get(2)?,
            kind: SymbolKind::parse_kind(&kind_str).unwrap_or(SymbolKind::Class),
            qualified_name: row.get(4)?,
            visibility: Visibility::parse_visibility(&vis_str).unwrap_or(Visibility::Public),
            file_id: row.get(6)?,
            line: row.get(7)?,
            column: row.get(8)?,
            end_line: row.get(9)?,
            end_column: row.get(10)?,
            parent_symbol_id: row.get(11)?,
            package: row.get(12)?,
            file_path: row.get(13)?,
        })
    }

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
                package TEXT NOT NULL,
                type_text TEXT
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
            CREATE INDEX IF NOT EXISTS idx_relationships_target_qname_kind ON relationships(target_qualified_name, kind);
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
                "INSERT INTO symbols (name, signature, qualified_name, kind, visibility, file_id, line, column_, end_line, end_column, parent_symbol_id, package, type_text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11, ?12)",
                params![
                    sym.name, sym.signature, sym.qualified_name,
                    sym.kind.as_str(), sym.visibility.as_str(),
                    file_id, sym.line, sym.column, sym.end_line, sym.end_column,
                    sym.package, sym.type_text
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
        // First try exact match on qualified_name, then fall back to simple name match.
        // This handles the case where the Java plugin emits "Repository" but the
        // symbol's qualified_name is "com.foo.Repository".
        let count = self.conn.execute(
            "UPDATE relationships SET target_symbol_id = COALESCE(
                (SELECT id FROM symbols WHERE qualified_name = relationships.target_qualified_name LIMIT 1),
                (SELECT id FROM symbols WHERE name = relationships.target_qualified_name LIMIT 1)
             ) WHERE target_symbol_id IS NULL",
            [],
        )?;
        Ok(count as u64)
    }

    fn resolve_wildcard_imports(&self, file_id: FileId, prefixes: &[String]) -> Result<u64> {
        let mut stmt = self.conn.prepare(
            "SELECT source_symbol_id, target_qualified_name, kind FROM relationships WHERE file_id = ?1 AND target_symbol_id IS NULL"
        )?;
        let rows: Vec<(i64, String, String)> = stmt.query_map(params![file_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;

        let mut resolved_count = 0u64;
        for (source_id, target_name, kind) in &rows {
            let simple_name = target_name.rsplit('.').next().unwrap_or(target_name);

            let mut matches = Vec::new();
            for prefix in prefixes {
                let candidate = format!("{}.{}", prefix, simple_name);
                let exists: bool = self.conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM symbols WHERE qualified_name = ?1)",
                    params![candidate],
                    |row| row.get(0),
                )?;
                if exists {
                    matches.push(candidate);
                }
            }

            if matches.len() == 1 {
                self.conn.execute(
                    "DELETE FROM relationships WHERE source_symbol_id = ?1 AND target_qualified_name = ?2 AND kind = ?3",
                    params![source_id, target_name, kind],
                )?;
                self.conn.execute(
                    "INSERT OR IGNORE INTO relationships (source_symbol_id, target_symbol_id, target_qualified_name, file_id, kind) VALUES (?1, NULL, ?2, ?3, ?4)",
                    params![source_id, matches[0], file_id, kind],
                )?;
                resolved_count += 1;
            }
        }
        Ok(resolved_count)
    }

    fn find_symbol(&self, query: &SymbolQuery) -> Result<Vec<Symbol>> {
        let like_pattern = glob_to_like(&query.pattern);
        let collate = if query.case_insensitive { " COLLATE NOCASE" } else { "" };

        let base = format!(
            "SELECT s.id, s.name, s.signature, s.kind, s.qualified_name, s.visibility, \
             s.file_id, s.line, s.column_, s.end_line, s.end_column, s.parent_symbol_id, s.package, f.path as file_path \
             FROM symbols s JOIN files f ON s.file_id = f.id \
             WHERE (s.name LIKE ?1{collate} ESCAPE '\\' OR s.qualified_name LIKE ?2{collate} ESCAPE '\\')"
        );

        let mut sql = base;
        if query.kind.is_some() {
            sql.push_str(" AND s.kind = ?3");
        }

        let pattern = query.pattern.clone();
        let like1 = like_pattern.clone();
        let like2 = like_pattern.clone();
        let case_insensitive = query.case_insensitive;

        let mut stmt = self.conn.prepare(&sql)?;
        let rows: Vec<Symbol> = if let Some(kind) = query.kind {
            stmt.query_map(params![like1, like2, kind.as_str()], Self::symbol_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map(params![like1, like2], Self::symbol_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        // Apply precise glob matching to filter out SQL LIKE over-matches
        let results = rows.into_iter().filter(|sym| {
            if case_insensitive {
                let pat_lower = pattern.to_lowercase();
                glob_match::glob_match(&pat_lower, &sym.name.to_lowercase())
                    || glob_match::glob_match(&pat_lower, &sym.qualified_name.to_lowercase())
            } else {
                glob_match::glob_match(&pattern, &sym.name)
                    || glob_match::glob_match(&pattern, &sym.qualified_name)
            }
        }).collect();

        Ok(results)
    }

    fn find_references(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, s.signature, s.kind, s.qualified_name, s.visibility, \
             s.file_id, s.line, s.column_, s.end_line, s.end_column, s.parent_symbol_id, s.package, f.path as file_path \
             FROM relationships r \
             JOIN symbols s ON s.id = r.source_symbol_id \
             JOIN files f ON s.file_id = f.id \
             WHERE r.target_symbol_id = ?"
        )?;
        let symbols = stmt.query_map(params![symbol_id], Self::symbol_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(symbols)
    }

    fn find_implementations(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>> {
        let extends = RelationshipKind::Extends.as_str();
        let implements = RelationshipKind::Implements.as_str();
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, s.signature, s.kind, s.qualified_name, s.visibility, \
             s.file_id, s.line, s.column_, s.end_line, s.end_column, s.parent_symbol_id, s.package, f.path as file_path \
             FROM relationships r \
             JOIN symbols s ON s.id = r.source_symbol_id \
             JOIN files f ON s.file_id = f.id \
             WHERE r.target_symbol_id = ? AND r.kind IN ('extends', 'implements')"
        )?;
        let _ = (extends, implements); // used as string literals above
        let symbols = stmt.query_map(params![symbol_id], Self::symbol_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(symbols)
    }

    fn find_supertypes(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, s.signature, s.kind, s.qualified_name, s.visibility, \
             s.file_id, s.line, s.column_, s.end_line, s.end_column, s.parent_symbol_id, s.package, f.path as file_path \
             FROM relationships r \
             JOIN symbols s ON s.id = r.target_symbol_id \
             JOIN files f ON s.file_id = f.id \
             WHERE r.source_symbol_id = ? AND r.kind IN ('extends', 'implements')"
        )?;
        let symbols = stmt.query_map(params![symbol_id], Self::symbol_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(symbols)
    }

    fn find_callers(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, s.signature, s.kind, s.qualified_name, s.visibility, \
             s.file_id, s.line, s.column_, s.end_line, s.end_column, s.parent_symbol_id, s.package, f.path as file_path \
             FROM relationships r \
             JOIN symbols s ON s.id = r.source_symbol_id \
             JOIN files f ON s.file_id = f.id \
             WHERE r.kind = 'calls' \
             AND (r.target_symbol_id = ?1 \
                  OR r.target_qualified_name = ( \
                      SELECT parent.qualified_name || '.' || target.name \
                      FROM symbols target \
                      JOIN symbols parent ON parent.id = target.parent_symbol_id \
                      WHERE target.id = ?1 \
                  ))"
        )?;
        let symbols = stmt.query_map(params![symbol_id], Self::symbol_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(symbols)
    }

    fn find_callees(&self, symbol_id: SymbolId) -> Result<Vec<Symbol>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT callee.id, callee.name, callee.signature, callee.kind, callee.qualified_name, callee.visibility, \
             callee.file_id, callee.line, callee.column_, callee.end_line, callee.end_column, callee.parent_symbol_id, callee.package, f.path as file_path \
             FROM relationships r \
             JOIN symbols callee ON callee.kind IN ('method', 'constructor') \
             JOIN symbols parent ON parent.id = callee.parent_symbol_id \
                 AND parent.qualified_name || '.' || callee.name = r.target_qualified_name \
             JOIN files f ON callee.file_id = f.id \
             WHERE r.source_symbol_id = ?1 AND r.kind = 'calls' \
             UNION \
             SELECT s.id, s.name, s.signature, s.kind, s.qualified_name, s.visibility, \
             s.file_id, s.line, s.column_, s.end_line, s.end_column, s.parent_symbol_id, s.package, f2.path as file_path \
             FROM relationships r2 \
             JOIN symbols s ON s.id = r2.target_symbol_id \
             JOIN files f2 ON s.file_id = f2.id \
             WHERE r2.source_symbol_id = ?1 AND r2.kind = 'calls' AND r2.target_symbol_id IS NOT NULL"
        )?;
        let symbols = stmt.query_map(params![symbol_id], Self::symbol_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(symbols)
    }

    fn symbols_in_file(&self, file_path: &str) -> Result<Vec<Symbol>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, s.signature, s.kind, s.qualified_name, s.visibility, \
             s.file_id, s.line, s.column_, s.end_line, s.end_column, s.parent_symbol_id, s.package, f.path as file_path \
             FROM symbols s JOIN files f ON s.file_id = f.id \
             WHERE f.path = ?"
        )?;
        let symbols = stmt.query_map(params![file_path], Self::symbol_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(symbols)
    }

    fn symbols_in_package(&self, package: &str, query: &SymbolQuery) -> Result<Vec<Symbol>> {
        let like_pattern = glob_to_like(&query.pattern);
        let collate = if query.case_insensitive { " COLLATE NOCASE" } else { "" };

        let mut sql = format!(
            "SELECT s.id, s.name, s.signature, s.kind, s.qualified_name, s.visibility, \
             s.file_id, s.line, s.column_, s.end_line, s.end_column, s.parent_symbol_id, s.package, f.path as file_path \
             FROM symbols s JOIN files f ON s.file_id = f.id \
             WHERE s.package = ?1 AND s.name LIKE ?2{collate} ESCAPE '\\'"
        );
        if query.kind.is_some() {
            sql.push_str(" AND s.kind = ?3");
        }

        let pattern = query.pattern.clone();
        let case_insensitive = query.case_insensitive;

        let mut stmt = self.conn.prepare(&sql)?;
        let rows: Vec<Symbol> = if let Some(kind) = query.kind {
            stmt.query_map(params![package, like_pattern, kind.as_str()], Self::symbol_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map(params![package, like_pattern], Self::symbol_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        // Apply precise glob matching on name
        let results = rows.into_iter().filter(|sym| {
            if case_insensitive {
                glob_match::glob_match(&pattern.to_lowercase(), &sym.name.to_lowercase())
            } else {
                glob_match::glob_match(&pattern, &sym.name)
            }
        }).collect();

        Ok(results)
    }

    fn begin_transaction(&self) -> Result<()> {
        self.conn.execute_batch("BEGIN")?;
        Ok(())
    }

    fn commit_transaction(&self) -> Result<()> {
        self.conn.execute_batch("COMMIT")?;
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
            type_text: None,
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
                parent_local_id: None, package: "com.foo".into(), type_text: None,
            },
            ExtractedSymbol {
                local_id: 1, name: "bar".into(), signature: Some("bar(String)".into()),
                qualified_name: "com.foo.Foo.bar(String)".into(), kind: SymbolKind::Method,
                visibility: Visibility::Public,
                line: 3, column: 4, end_line: 5, end_column: 5,
                parent_local_id: Some(0), package: "com.foo".into(), type_text: None,
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
            parent_local_id: None, package: "com.foo".into(), type_text: None,
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
            parent_local_id: None, package: "com.foo".into(), type_text: None,
        }];
        let ids1 = store.insert_symbols(f1, &syms1).unwrap();
        let syms2 = vec![ExtractedSymbol {
            local_id: 0, name: "Bar".into(), signature: None,
            qualified_name: "com.foo.Bar".into(), kind: SymbolKind::Class,
            visibility: Visibility::Public,
            line: 1, column: 0, end_line: 10, end_column: 1,
            parent_local_id: None, package: "com.foo".into(), type_text: None,
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
    fn test_resolve_wildcard_imports() {
        let store = test_store();

        let f1 = store.upsert_file("Repository.java", 1, None, "java").unwrap();
        let syms1 = vec![ExtractedSymbol {
            local_id: 0, name: "Repository".into(), signature: None,
            qualified_name: "com.foo.Repository".into(), kind: SymbolKind::Interface,
            visibility: Visibility::Public,
            line: 1, column: 0, end_line: 10, end_column: 1,
            parent_local_id: None, package: "com.foo".into(), type_text: None,
        }];
        store.insert_symbols(f1, &syms1).unwrap();

        let f2 = store.upsert_file("UserService.java", 1, None, "java").unwrap();
        let syms2 = vec![ExtractedSymbol {
            local_id: 0, name: "UserService".into(), signature: None,
            qualified_name: "com.bar.UserService".into(), kind: SymbolKind::Class,
            visibility: Visibility::Public,
            line: 1, column: 0, end_line: 10, end_column: 1,
            parent_local_id: None, package: "com.bar".into(), type_text: None,
        }];
        let ids2 = store.insert_symbols(f2, &syms2).unwrap();
        let map2: Vec<(usize, SymbolId)> = vec![(0, ids2[0])];
        let rels = vec![ExtractedRelationship {
            source_local_id: 0,
            target_qualified_name: "com.bar.Repository".into(),
            kind: RelationshipKind::Implements,
        }];
        store.insert_relationships(f2, &map2, &rels).unwrap();

        let resolved = store.resolve_wildcard_imports(f2, &["com.foo".to_string()]).unwrap();
        assert_eq!(resolved, 1);

        let count = store.resolve_relationships().unwrap();
        assert!(count >= 1);
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

    /// Returns (store, user_service_id, save_person_id, person_repo_id, find_all_id)
    fn seed_store() -> (SqliteStore, SymbolId, SymbolId, SymbolId, SymbolId) {
        let store = test_store();

        // File 1: UserService.java
        let f1 = store.upsert_file("UserService.java", 1, None, "java").unwrap();
        let syms1 = vec![
            ExtractedSymbol {
                local_id: 0, name: "UserService".into(), signature: None,
                qualified_name: "com.foo.UserService".into(), kind: SymbolKind::Class,
                visibility: Visibility::Public,
                line: 1, column: 0, end_line: 20, end_column: 1,
                parent_local_id: None, package: "com.foo".into(), type_text: None,
            },
            ExtractedSymbol {
                local_id: 1, name: "save".into(), signature: Some("save(Person)".into()),
                qualified_name: "com.foo.UserService.save(Person)".into(), kind: SymbolKind::Method,
                visibility: Visibility::Public,
                line: 5, column: 4, end_line: 10, end_column: 5,
                parent_local_id: Some(0), package: "com.foo".into(), type_text: None,
            },
        ];
        let ids1 = store.insert_symbols(f1, &syms1).unwrap();
        let user_service_id = ids1[0];
        let save_person_id = ids1[1];

        // File 2: PersonRepo.java
        let f2 = store.upsert_file("PersonRepo.java", 1, None, "java").unwrap();
        let syms2 = vec![
            ExtractedSymbol {
                local_id: 0, name: "PersonRepo".into(), signature: None,
                qualified_name: "com.foo.PersonRepo".into(), kind: SymbolKind::Interface,
                visibility: Visibility::Public,
                line: 1, column: 0, end_line: 15, end_column: 1,
                parent_local_id: None, package: "com.foo".into(), type_text: None,
            },
            ExtractedSymbol {
                local_id: 1, name: "findAll".into(), signature: Some("findAll()".into()),
                qualified_name: "com.foo.PersonRepo.findAll()".into(), kind: SymbolKind::Method,
                visibility: Visibility::Public,
                line: 3, column: 4, end_line: 3, end_column: 30,
                parent_local_id: Some(0), package: "com.foo".into(), type_text: None,
            },
        ];
        let ids2 = store.insert_symbols(f2, &syms2).unwrap();
        let person_repo_id = ids2[0];
        let find_all_id = ids2[1];

        // Relationship: UserService implements PersonRepo
        let map1: Vec<(usize, SymbolId)> = vec![(0, user_service_id), (1, save_person_id)];
        let rels1 = vec![ExtractedRelationship {
            source_local_id: 0,
            target_qualified_name: "com.foo.PersonRepo".into(),
            kind: RelationshipKind::Implements,
        }];
        store.insert_relationships(f1, &map1, &rels1).unwrap();

        // Relationship: UserService.save(Person) calls PersonRepo.findAll()
        let rels2 = vec![ExtractedRelationship {
            source_local_id: 1,
            target_qualified_name: "com.foo.PersonRepo.findAll()".into(),
            kind: RelationshipKind::Calls,
        }];
        store.insert_relationships(f1, &map1, &rels2).unwrap();

        store.resolve_relationships().unwrap();

        (store, user_service_id, save_person_id, person_repo_id, find_all_id)
    }

    #[test]
    fn test_find_symbol_exact() {
        let (store, _, _, _, _) = seed_store();
        let q = SymbolQuery { pattern: "UserService".into(), case_insensitive: false, kind: None };
        let results = store.find_symbol(&q).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "UserService");
    }

    #[test]
    fn test_find_symbol_glob() {
        let (store, _, _, _, _) = seed_store();
        let q = SymbolQuery { pattern: "*Service".into(), case_insensitive: false, kind: None };
        let results = store.find_symbol(&q).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "UserService");
    }

    #[test]
    fn test_find_symbol_by_kind() {
        let (store, _, _, _, _) = seed_store();
        let q = SymbolQuery { pattern: "*".into(), case_insensitive: false, kind: Some(SymbolKind::Interface) };
        let results = store.find_symbol(&q).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "PersonRepo");
    }

    #[test]
    fn test_find_implementations() {
        let (store, _, _, person_repo_id, _) = seed_store();
        let results = store.find_implementations(person_repo_id).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "UserService");
    }

    #[test]
    fn test_find_supertypes() {
        let (store, user_service_id, _, _, _) = seed_store();
        let results = store.find_supertypes(user_service_id).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "PersonRepo");
    }

    #[test]
    fn test_find_callers() {
        let (store, _, _, _, find_all_id) = seed_store();
        let results = store.find_callers(find_all_id).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "save");
    }

    #[test]
    fn test_find_callees() {
        let (store, _, save_person_id, _, _) = seed_store();
        let results = store.find_callees(save_person_id).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "findAll");
    }

    #[test]
    fn test_find_references() {
        let (store, _, _, person_repo_id, _) = seed_store();
        let results = store.find_references(person_repo_id).unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().any(|s| s.name == "UserService"));
    }

    #[test]
    fn test_symbols_in_file() {
        let (store, _, _, _, _) = seed_store();
        let results = store.symbols_in_file("UserService.java").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_symbols_in_package() {
        let (store, _, _, _, _) = seed_store();
        let q = SymbolQuery { pattern: "*".into(), case_insensitive: false, kind: None };
        let results = store.symbols_in_package("com.foo", &q).unwrap();
        assert_eq!(results.len(), 4);
    }

    #[test]
    fn test_find_callers_via_method_key() {
        let store = test_store();
        let f1 = store.upsert_file("Repository.java", 1, None, "java").unwrap();
        let syms1 = vec![
            ExtractedSymbol {
                local_id: 0, name: "Repository".into(), signature: None,
                qualified_name: "com.foo.Repository".into(), kind: SymbolKind::Interface,
                visibility: Visibility::Public,
                line: 1, column: 0, end_line: 5, end_column: 1,
                parent_local_id: None, package: "com.foo".into(), type_text: None,
            },
            ExtractedSymbol {
                local_id: 1, name: "save".into(), signature: Some("save(Object)".into()),
                qualified_name: "com.foo.Repository.save(Object)".into(), kind: SymbolKind::Method,
                visibility: Visibility::Public,
                line: 3, column: 4, end_line: 3, end_column: 30,
                parent_local_id: Some(0), package: "com.foo".into(), type_text: None,
            },
        ];
        let ids1 = store.insert_symbols(f1, &syms1).unwrap();
        let save_id = ids1[1];

        let f2 = store.upsert_file("Service.java", 1, None, "java").unwrap();
        let syms2 = vec![
            ExtractedSymbol {
                local_id: 0, name: "Service".into(), signature: None,
                qualified_name: "com.bar.Service".into(), kind: SymbolKind::Class,
                visibility: Visibility::Public,
                line: 1, column: 0, end_line: 10, end_column: 1,
                parent_local_id: None, package: "com.bar".into(), type_text: None,
            },
            ExtractedSymbol {
                local_id: 1, name: "doWork".into(), signature: Some("doWork()".into()),
                qualified_name: "com.bar.Service.doWork()".into(), kind: SymbolKind::Method,
                visibility: Visibility::Public,
                line: 5, column: 4, end_line: 8, end_column: 5,
                parent_local_id: Some(0), package: "com.bar".into(), type_text: None,
            },
        ];
        let ids2 = store.insert_symbols(f2, &syms2).unwrap();
        let map2: Vec<(usize, SymbolId)> = vec![(0, ids2[0]), (1, ids2[1])];

        let rels = vec![ExtractedRelationship {
            source_local_id: 1,
            target_qualified_name: "com.foo.Repository.save".into(),
            kind: RelationshipKind::Calls,
        }];
        store.insert_relationships(f2, &map2, &rels).unwrap();
        store.resolve_relationships().unwrap();

        let callers = store.find_callers(save_id).unwrap();
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].name, "doWork");
    }

    #[test]
    fn test_find_callees_via_method_key() {
        let store = test_store();
        let f1 = store.upsert_file("Repository.java", 1, None, "java").unwrap();
        let syms1 = vec![
            ExtractedSymbol {
                local_id: 0, name: "Repository".into(), signature: None,
                qualified_name: "com.foo.Repository".into(), kind: SymbolKind::Interface,
                visibility: Visibility::Public,
                line: 1, column: 0, end_line: 5, end_column: 1,
                parent_local_id: None, package: "com.foo".into(), type_text: None,
            },
            ExtractedSymbol {
                local_id: 1, name: "save".into(), signature: Some("save(Object)".into()),
                qualified_name: "com.foo.Repository.save(Object)".into(), kind: SymbolKind::Method,
                visibility: Visibility::Public,
                line: 3, column: 4, end_line: 3, end_column: 30,
                parent_local_id: Some(0), package: "com.foo".into(), type_text: None,
            },
        ];
        store.insert_symbols(f1, &syms1).unwrap();

        let f2 = store.upsert_file("Service.java", 1, None, "java").unwrap();
        let syms2 = vec![
            ExtractedSymbol {
                local_id: 0, name: "Service".into(), signature: None,
                qualified_name: "com.bar.Service".into(), kind: SymbolKind::Class,
                visibility: Visibility::Public,
                line: 1, column: 0, end_line: 10, end_column: 1,
                parent_local_id: None, package: "com.bar".into(), type_text: None,
            },
            ExtractedSymbol {
                local_id: 1, name: "doWork".into(), signature: Some("doWork()".into()),
                qualified_name: "com.bar.Service.doWork()".into(), kind: SymbolKind::Method,
                visibility: Visibility::Public,
                line: 5, column: 4, end_line: 8, end_column: 5,
                parent_local_id: Some(0), package: "com.bar".into(), type_text: None,
            },
        ];
        let ids2 = store.insert_symbols(f2, &syms2).unwrap();
        let dowork_id = ids2[1];
        let map2: Vec<(usize, SymbolId)> = vec![(0, ids2[0]), (1, ids2[1])];

        let rels = vec![ExtractedRelationship {
            source_local_id: 1,
            target_qualified_name: "com.foo.Repository.save".into(),
            kind: RelationshipKind::Calls,
        }];
        store.insert_relationships(f2, &map2, &rels).unwrap();
        store.resolve_relationships().unwrap();

        let callees = store.find_callees(dowork_id).unwrap();
        assert_eq!(callees.len(), 1);
        assert_eq!(callees[0].name, "save");
    }
}
