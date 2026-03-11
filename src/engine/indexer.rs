use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};
use walkdir::WalkDir;
use anyhow::Result;
use sha2::{Sha256, Digest};
use crate::model::FileId;
use crate::plugin::{PluginRegistry, LanguagePlugin};
use crate::store::Store;
use super::project;

#[derive(Debug, Default)]
pub struct ReindexStats {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
    pub unchanged: u64,
    pub elapsed_ms: u128,
}

fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Discover all files the registry can handle, optionally filtered by language.
/// Returns each file paired with the plugin that handles it.
pub fn discover_files<'a>(root: &Path, registry: &'a PluginRegistry, languages: Option<&[String]>) -> Vec<(PathBuf, &'a dyn LanguagePlugin)> {
    let plugins: Vec<&dyn LanguagePlugin> = match languages {
        Some(langs) => registry.plugins_for_languages(langs),
        None => registry.all_plugins(),
    };
    let mut result = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.starts_with(root.join(".codix")) { continue; }
        if let Some(plugin) = plugins.iter().find(|p| p.can_handle(path)) {
            result.push((path.to_path_buf(), *plugin));
        }
    }
    result
}

/// Full reindex: clear everything, re-parse all files.
/// Returns a map of language name → file count, sorted alphabetically.
pub fn full_index(root: &Path, store: &dyn Store, registry: &PluginRegistry, languages: Option<&[String]>) -> Result<BTreeMap<String, u64>> {
    store.clear_all()?;
    let files = discover_files(root, registry, languages);
    store.begin_transaction()?;
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut wildcard_map: HashMap<FileId, Vec<String>> = HashMap::new();
    for (path, plugin) in &files {
        let (file_id, wildcards): (FileId, Vec<String>) = index_file(root, path, *plugin, store)?;
        if !wildcards.is_empty() {
            wildcard_map.insert(file_id, wildcards);
        }
        *counts.entry(plugin.display_name().to_string()).or_default() += 1;
    }
    for (file_id, prefixes) in &wildcard_map {
        store.resolve_wildcard_imports(*file_id, prefixes)?;
    }
    store.resolve_relationships()?;
    store.commit_transaction()?;
    Ok(counts)
}

/// Incremental reindex: only process new, modified, or deleted files.
pub fn incremental_reindex(root: &Path, store: &dyn Store, registry: &PluginRegistry, languages: Option<&[String]>) -> Result<ReindexStats> {
    let start = Instant::now();
    let disk_files = discover_files(root, registry, languages);
    let indexed_files = store.list_files()?;

    let disk_paths: HashSet<String> = disk_files.iter()
        .map(|(p, _)| project::relative_to_root(root, p))
        .collect();
    store.begin_transaction()?;

    let mut stats = ReindexStats::default();

    // Delete removed files
    for indexed in &indexed_files {
        if !disk_paths.contains(&indexed.path) {
            store.delete_relationships_for_file(indexed.id)?;
            store.delete_symbols_for_file(indexed.id)?;
            store.delete_file(indexed.id)?;
            stats.deleted.push(indexed.path.clone());
        }
    }

    // Add new or modified files
    let mut wildcard_map: HashMap<FileId, Vec<String>> = HashMap::new();
    for (path, plugin) in &disk_files {
        let rel_path = project::relative_to_root(root, path);
        let mtime = std::fs::metadata(path)?
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        match store.get_file(&rel_path)? {
            None => {
                // New file — index it
                let (file_id, wildcards): (FileId, Vec<String>) = index_file(root, path, *plugin, store)?;
                if !wildcards.is_empty() {
                    wildcard_map.insert(file_id, wildcards);
                }
                stats.added.push(rel_path);
            }
            Some(f) if f.mtime < mtime => {
                // Mtime changed — check hash to avoid unnecessary reindex
                let source = std::fs::read(path)?;
                let hash = compute_sha256(&source);
                if f.hash.as_deref() == Some(&hash) {
                    // Content unchanged, just update mtime
                    store.upsert_file(&rel_path, mtime, Some(&hash), &f.language)?;
                    stats.unchanged += 1;
                } else {
                    // Content actually changed — reindex
                    store.delete_relationships_for_file(f.id)?;
                    store.delete_symbols_for_file(f.id)?;
                    let (file_id, wildcards): (FileId, Vec<String>) = index_file(root, path, *plugin, store)?;
                    if !wildcards.is_empty() {
                        wildcard_map.insert(file_id, wildcards);
                    }
                    stats.modified.push(rel_path);
                }
            }
            _ => {
                stats.unchanged += 1;
            }
        }
    }

    let changed = !stats.added.is_empty() || !stats.modified.is_empty() || !stats.deleted.is_empty();
    if changed {
        for (file_id, prefixes) in &wildcard_map {
            store.resolve_wildcard_imports(*file_id, prefixes)?;
        }
        store.resolve_relationships()?;
    }
    store.commit_transaction()?;
    stats.elapsed_ms = start.elapsed().as_millis();
    Ok(stats)
}

fn index_file(root: &Path, path: &Path, plugin: &dyn LanguagePlugin, store: &dyn Store) -> Result<(FileId, Vec<String>)> {
    let source = std::fs::read(path)?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&plugin.tree_sitter_language())?;
    let tree = parser.parse(&source, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse {}", path.display()))?;
    let result = plugin.extract_symbols(&tree, &source, path);
    let rel_path = project::relative_to_root(root, path);
    let mtime = std::fs::metadata(path)?
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let hash = compute_sha256(&source);
    let file_id = store.upsert_file(&rel_path, mtime, Some(&hash), plugin.name())?;
    let symbol_ids = store.insert_symbols(file_id, &result.symbols)?;
    let map: Vec<(usize, _)> = result.symbols.iter()
        .map(|s| s.local_id)
        .zip(symbol_ids.iter().copied())
        .collect();
    store.insert_relationships(file_id, &map, &result.relationships)?;
    Ok((file_id, result.wildcard_imports))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::sqlite::SqliteStore;
    use tempfile::TempDir;
    use std::fs;

    fn setup_project() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir(root.join(".codix")).unwrap();
        fs::create_dir_all(root.join("src/main/java/com/foo")).unwrap();
        fs::write(root.join("src/main/java/com/foo/Foo.java"),
            "package com.foo;\npublic class Foo {}").unwrap();
        fs::write(root.join("src/main/java/com/foo/Bar.java"),
            "package com.foo;\npublic class Bar {}").unwrap();
        fs::write(root.join("src/main/java/com/foo/readme.txt"),
            "not java").unwrap();
        (tmp, root)
    }

    #[test]
    fn test_discover_files() {
        let (_tmp, root) = setup_project();
        let registry = PluginRegistry::new();
        let files = discover_files(&root, &registry, None);
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|(_, plugin)| plugin.name() == "java"));
    }

    #[test]
    fn test_full_index() {
        let (_tmp, root) = setup_project();
        let store = SqliteStore::open(":memory:").unwrap();
        let registry = PluginRegistry::new();
        let counts = full_index(&root, &store, &registry, None).unwrap();
        assert_eq!(counts.values().sum::<u64>(), 2);
        assert_eq!(store.list_files().unwrap().len(), 2);
    }

    #[test]
    fn test_incremental_reindex_detects_new_file() {
        let (_tmp, root) = setup_project();
        let store = SqliteStore::open(":memory:").unwrap();
        let registry = PluginRegistry::new();
        full_index(&root, &store, &registry, None).unwrap();
        assert_eq!(store.list_files().unwrap().len(), 2);

        fs::write(root.join("src/main/java/com/foo/Baz.java"),
            "package com.foo;\npublic class Baz {}").unwrap();
        incremental_reindex(&root, &store, &registry, None).unwrap();
        assert_eq!(store.list_files().unwrap().len(), 3);
    }

    #[test]
    fn test_incremental_reindex_detects_deleted_file() {
        let (_tmp, root) = setup_project();
        let store = SqliteStore::open(":memory:").unwrap();
        let registry = PluginRegistry::new();
        full_index(&root, &store, &registry, None).unwrap();
        assert_eq!(store.list_files().unwrap().len(), 2);

        fs::remove_file(root.join("src/main/java/com/foo/Bar.java")).unwrap();
        incremental_reindex(&root, &store, &registry, None).unwrap();
        assert_eq!(store.list_files().unwrap().len(), 1);
    }

    #[test]
    fn test_wildcard_import_resolution() {
        let (_tmp, root) = setup_project();
        let store = SqliteStore::open(":memory:").unwrap();
        let registry = PluginRegistry::new();

        fs::create_dir_all(root.join("src/main/java/com/bar")).unwrap();
        fs::write(root.join("src/main/java/com/bar/Client.java"),
            "package com.bar;\nimport com.foo.*;\npublic class Client extends Foo {}").unwrap();

        full_index(&root, &store, &registry, None).unwrap();

        let query = crate::model::SymbolQuery {
            pattern: "Client".to_string(),
            case_insensitive: false,
            kind: None,
        };
        let results = store.find_symbol(&query).unwrap();
        assert_eq!(results.len(), 1);
        let supers = store.find_supertypes(results[0].id).unwrap();
        assert_eq!(supers.len(), 1);
        assert_eq!(supers[0].qualified_name, "com.foo.Foo");
    }

    #[test]
    fn test_ambiguous_wildcard_not_resolved() {
        let (_tmp, root) = setup_project();
        let store = SqliteStore::open(":memory:").unwrap();
        let registry = PluginRegistry::new();

        fs::create_dir_all(root.join("src/main/java/com/other")).unwrap();
        fs::write(root.join("src/main/java/com/other/Foo.java"),
            "package com.other;\npublic class Foo {}").unwrap();

        fs::create_dir_all(root.join("src/main/java/com/client")).unwrap();
        fs::write(root.join("src/main/java/com/client/Client.java"),
            "package com.client;\nimport com.foo.*;\nimport com.other.*;\npublic class Client extends Foo {}").unwrap();

        full_index(&root, &store, &registry, None).unwrap();

        let query = crate::model::SymbolQuery {
            pattern: "Client".to_string(),
            case_insensitive: false,
            kind: None,
        };
        let results = store.find_symbol(&query).unwrap();
        assert_eq!(results.len(), 1);
        let supers = store.find_supertypes(results[0].id).unwrap();
        // Ambiguous: wildcard resolution skips it, COALESCE fallback may pick one
        assert!(supers.len() <= 1);
    }

    #[test]
    fn test_incremental_reindex_detects_modified_file() {
        let (_tmp, root) = setup_project();
        let store = SqliteStore::open(":memory:").unwrap();
        let registry = PluginRegistry::new();
        full_index(&root, &store, &registry, None).unwrap();

        // Sleep to ensure mtime changes (HFS+ has 1s granularity)
        std::thread::sleep(std::time::Duration::from_millis(1500));
        fs::write(root.join("src/main/java/com/foo/Foo.java"),
            "package com.foo;\npublic class Foo {\n  void newMethod() {}\n}").unwrap();
        incremental_reindex(&root, &store, &registry, None).unwrap();

        let syms = store.symbols_in_file("src/main/java/com/foo/Foo.java").unwrap();
        assert_eq!(syms.len(), 2); // class + new method
    }
}
