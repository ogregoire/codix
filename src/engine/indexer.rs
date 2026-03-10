use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;
use anyhow::Result;
use crate::plugin::{PluginRegistry, LanguagePlugin};
use crate::store::Store;
use super::project;

/// Discover all files the registry can handle.
pub fn discover_files(root: &Path, registry: &PluginRegistry) -> Vec<(PathBuf, String)> {
    let extensions = registry.all_extensions();
    let mut result = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.starts_with(root.join(".codix")) { continue; }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if extensions.contains(&ext) {
                result.push((path.to_path_buf(), ext.to_string()));
            }
        }
    }
    result
}

/// Full reindex: clear everything, re-parse all files.
pub fn full_index(root: &Path, store: &dyn Store, registry: &PluginRegistry) -> Result<u64> {
    store.clear_all()?;
    let files = discover_files(root, registry);
    store.begin_transaction()?;
    let mut count = 0u64;
    for (path, ext) in &files {
        let plugin = registry.plugin_for_extension(ext).unwrap();
        index_file(root, path, plugin, store)?;
        count += 1;
    }
    store.resolve_relationships()?;
    store.commit_transaction()?;
    Ok(count)
}

/// Incremental reindex: only process new, modified, or deleted files.
pub fn incremental_reindex(root: &Path, store: &dyn Store, registry: &PluginRegistry) -> Result<()> {
    let disk_files = discover_files(root, registry);
    let indexed_files = store.list_files()?;

    let disk_paths: HashSet<String> = disk_files.iter()
        .map(|(p, _)| project::relative_to_root(root, p))
        .collect();
    store.begin_transaction()?;

    // Delete removed files
    for indexed in &indexed_files {
        if !disk_paths.contains(&indexed.path) {
            store.delete_relationships_for_file(indexed.id)?;
            store.delete_symbols_for_file(indexed.id)?;
            store.delete_file(indexed.id)?;
        }
    }

    // Add new or modified files
    for (path, ext) in &disk_files {
        let rel_path = project::relative_to_root(root, path);
        let mtime = std::fs::metadata(path)?
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let needs_reindex = match store.get_file(&rel_path)? {
            None => true,
            Some(f) => f.mtime < mtime,
        };

        if needs_reindex {
            if let Some(f) = store.get_file(&rel_path)? {
                store.delete_relationships_for_file(f.id)?;
                store.delete_symbols_for_file(f.id)?;
            }
            let plugin = registry.plugin_for_extension(ext).unwrap();
            index_file(root, path, plugin, store)?;
        }
    }

    store.resolve_relationships()?;
    store.commit_transaction()?;
    Ok(())
}

fn index_file(root: &Path, path: &Path, plugin: &dyn LanguagePlugin, store: &dyn Store) -> Result<()> {
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
    let file_id = store.upsert_file(&rel_path, mtime, None, plugin.name())?;
    let symbol_ids = store.insert_symbols(file_id, &result.symbols)?;
    let map: Vec<(usize, _)> = result.symbols.iter()
        .map(|s| s.local_id)
        .zip(symbol_ids.iter().copied())
        .collect();
    store.insert_relationships(file_id, &map, &result.relationships)?;
    Ok(())
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
        let files = discover_files(&root, &registry);
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|(_, ext)| ext == "java"));
    }

    #[test]
    fn test_full_index() {
        let (_tmp, root) = setup_project();
        let store = SqliteStore::open(":memory:").unwrap();
        let registry = PluginRegistry::new();
        let count = full_index(&root, &store, &registry).unwrap();
        assert_eq!(count, 2);
        assert_eq!(store.list_files().unwrap().len(), 2);
    }

    #[test]
    fn test_incremental_reindex_detects_new_file() {
        let (_tmp, root) = setup_project();
        let store = SqliteStore::open(":memory:").unwrap();
        let registry = PluginRegistry::new();
        full_index(&root, &store, &registry).unwrap();
        assert_eq!(store.list_files().unwrap().len(), 2);

        fs::write(root.join("src/main/java/com/foo/Baz.java"),
            "package com.foo;\npublic class Baz {}").unwrap();
        incremental_reindex(&root, &store, &registry).unwrap();
        assert_eq!(store.list_files().unwrap().len(), 3);
    }

    #[test]
    fn test_incremental_reindex_detects_deleted_file() {
        let (_tmp, root) = setup_project();
        let store = SqliteStore::open(":memory:").unwrap();
        let registry = PluginRegistry::new();
        full_index(&root, &store, &registry).unwrap();
        assert_eq!(store.list_files().unwrap().len(), 2);

        fs::remove_file(root.join("src/main/java/com/foo/Bar.java")).unwrap();
        incremental_reindex(&root, &store, &registry).unwrap();
        assert_eq!(store.list_files().unwrap().len(), 1);
    }

    #[test]
    fn test_incremental_reindex_detects_modified_file() {
        let (_tmp, root) = setup_project();
        let store = SqliteStore::open(":memory:").unwrap();
        let registry = PluginRegistry::new();
        full_index(&root, &store, &registry).unwrap();

        // Sleep to ensure mtime changes (HFS+ has 1s granularity)
        std::thread::sleep(std::time::Duration::from_millis(1500));
        fs::write(root.join("src/main/java/com/foo/Foo.java"),
            "package com.foo;\npublic class Foo {\n  void newMethod() {}\n}").unwrap();
        incremental_reindex(&root, &store, &registry).unwrap();

        let syms = store.symbols_in_file("src/main/java/com/foo/Foo.java").unwrap();
        assert_eq!(syms.len(), 2); // class + new method
    }
}
