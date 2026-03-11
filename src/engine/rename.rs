use std::collections::HashSet;
use std::path::Path;
use anyhow::Result;
use crate::model::*;
use crate::plugin::PluginRegistry;
use crate::store::Store;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct FileOccurrences {
    pub file_path: String,
    pub occurrences: Vec<RenameOccurrence>,
}

#[derive(Debug, Serialize)]
pub struct RenameResult {
    pub changes: Vec<FileOccurrences>,
    pub warnings: Vec<String>,
}

impl RenameResult {
    pub fn total_occurrences(&self) -> usize {
        self.changes.iter().map(|f| f.occurrences.len()).sum()
    }

    pub fn total_files(&self) -> usize {
        self.changes.len()
    }
}

/// Find all occurrences that would be renamed. Does not modify any files.
pub fn find_occurrences(
    root: &Path,
    store: &dyn Store,
    registry: &PluginRegistry,
    symbol: &Symbol,
    new_name: &str,
) -> Result<RenameResult> {
    // Validate: same name check
    if symbol.name == new_name {
        anyhow::bail!("Symbol is already named '{}'. Nothing to rename.", new_name);
    }

    // Determine language from file record
    let file_record = store.get_file(&symbol.file_path)?
        .ok_or_else(|| anyhow::anyhow!("File '{}' not found in index", symbol.file_path))?;

    let plugin = registry.all_plugins().into_iter()
        .find(|p| p.name() == file_record.language)
        .ok_or_else(|| anyhow::anyhow!("No plugin found for language '{}'", file_record.language))?;

    // Check plugin support
    if !plugin.supports(PluginCapability::Rename) {
        let supported = registry.supported_languages_for(PluginCapability::Rename);
        let supported_str = if supported.is_empty() {
            "none".to_string()
        } else {
            supported.join(", ")
        };
        anyhow::bail!(
            "Rename is not supported for {} files. Supported: {}",
            plugin.display_name(),
            supported_str
        );
    }

    // Check for name conflicts
    check_name_conflict(store, symbol, new_name)?;

    // Collect related symbols based on kind
    let related = collect_related_symbols(store, symbol)?;

    // Collect affected file paths (deduped)
    let mut affected_paths: HashSet<String> = HashSet::new();
    affected_paths.insert(symbol.file_path.clone());
    for sym in &related {
        affected_paths.insert(sym.file_path.clone());
    }

    let mut warnings = Vec::new();
    let mut changes = Vec::new();

    for file_path in &affected_paths {
        // Check if this file's language supports rename
        let file_rec = store.get_file(file_path)?;
        if let Some(ref fr) = file_rec {
            let file_plugin = registry.all_plugins().into_iter()
                .find(|p| p.name() == fr.language);
            if let Some(fp) = file_plugin {
                if !fp.supports(PluginCapability::Rename) {
                    warnings.push(format!(
                        "Warning: references in {} skipped ({} rename not supported)",
                        file_path, fp.display_name()
                    ));
                    continue;
                }
            }
        }

        let abs_path = root.join(file_path);
        let source = std::fs::read(&abs_path)?;

        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&plugin.tree_sitter_language())?;
        let tree = parser.parse(&source, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse {}", file_path))?;

        match plugin.find_rename_occurrences(
            &tree,
            &source,
            &symbol.name,
            &symbol.kind,
            &symbol.qualified_name,
        ) {
            Ok(occurrences) if !occurrences.is_empty() => {
                changes.push(FileOccurrences {
                    file_path: file_path.clone(),
                    occurrences,
                });
            }
            Ok(_) => {} // no occurrences in this file
            Err(RenameError::NotSupported { language }) => {
                warnings.push(format!(
                    "Warning: references in {} skipped ({} rename not supported)",
                    file_path, language
                ));
            }
        }
    }

    // Sort by file path, then occurrences by line
    changes.sort_by(|a, b| a.file_path.cmp(&b.file_path));
    for fc in &mut changes {
        fc.occurrences.sort_by_key(|o| (o.line, o.column));
    }

    Ok(RenameResult { changes, warnings })
}

/// Apply the rename: rewrite files and update the store.
pub fn apply_rename(
    root: &Path,
    store: &dyn Store,
    symbol: &Symbol,
    new_name: &str,
    result: &RenameResult,
) -> Result<()> {
    // Rewrite files
    for file_occ in &result.changes {
        let abs_path = root.join(&file_occ.file_path);
        let source = std::fs::read(&abs_path)?;

        // Apply replacements bottom-to-top (highest byte_offset first)
        let mut modified = source;
        let mut sorted_occs: Vec<&RenameOccurrence> = file_occ.occurrences.iter().collect();
        sorted_occs.sort_by(|a, b| b.byte_offset.cmp(&a.byte_offset));

        for occ in sorted_occs {
            let old_bytes = occ.old_text.as_bytes();
            let end = occ.byte_offset + old_bytes.len();
            modified.splice(occ.byte_offset..end, new_name.bytes());
        }

        std::fs::write(&abs_path, &modified)?;
    }

    // Update store
    let old_name = &symbol.name;
    let old_qualified = &symbol.qualified_name;
    let new_qualified = build_new_qualified_name(old_qualified, old_name, new_name);
    let new_signature = symbol.signature.as_ref().map(|sig| {
        build_new_signature(sig, old_name, new_name)
    });

    store.update_symbol_name(
        symbol.id,
        new_name,
        &new_qualified,
        new_signature.as_deref(),
    )?;

    // For class renames, cascade to children
    let kind_str = symbol.kind.as_str();
    if kind_str == "class" || kind_str == "interface" || kind_str == "enum"
        || kind_str == "record" || kind_str == "annotation" {
        store.update_child_qualified_names(symbol.id, old_qualified, &new_qualified)?;
    }

    store.update_relationship_targets(old_qualified, &new_qualified)?;

    // Update mtimes for modified files
    for file_occ in &result.changes {
        let abs_path = root.join(&file_occ.file_path);
        if let Ok(metadata) = std::fs::metadata(&abs_path) {
            if let Ok(mtime) = metadata.modified() {
                let mtime_secs = mtime
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                if let Some(file_rec) = store.get_file(&file_occ.file_path)? {
                    store.update_file_mtime(file_rec.id, mtime_secs)?;
                }
            }
        }
    }

    Ok(())
}

fn check_name_conflict(store: &dyn Store, symbol: &Symbol, new_name: &str) -> Result<()> {
    let new_qualified = build_new_qualified_name(&symbol.qualified_name, &symbol.name, new_name);
    let query = SymbolQuery {
        pattern: new_qualified.clone(),
        case_insensitive: false,
        kind: Some(symbol.kind.clone()),
    };
    let conflicts = store.find_symbol(&query)?;
    if let Some(conflict) = conflicts.first() {
        let label = conflict.signature.as_deref().unwrap_or(&conflict.name);
        anyhow::bail!(
            "A {} '{}' already exists in {} at {}:{}",
            conflict.kind.as_str(),
            label,
            conflict.package,
            conflict.file_path,
            conflict.line
        );
    }
    Ok(())
}

fn collect_related_symbols(store: &dyn Store, symbol: &Symbol) -> Result<Vec<Symbol>> {
    let kind_str = symbol.kind.as_str();
    let mut related = Vec::new();

    match kind_str {
        "class" | "interface" | "enum" | "record" | "annotation" => {
            related.extend(store.find_references(symbol.id)?);
        }
        "method" | "constructor" => {
            related.extend(store.find_callers(symbol.id)?);
            related.extend(store.find_implementations(symbol.id)?);

            // Walk up the override chain: find if this method overrides a supertype method
            if let Some(parent_id) = symbol.parent_symbol_id {
                let supertypes = store.find_supertypes(parent_id)?;
                for supertype in &supertypes {
                    // Look for methods with the same name in the supertype
                    let query = SymbolQuery {
                        pattern: symbol.name.clone(),
                        case_insensitive: false,
                        kind: Some(SymbolKind::new("method")),
                    };
                    let candidates = store.find_symbol(&query)?;
                    for candidate in candidates {
                        if candidate.parent_symbol_id == Some(supertype.id) {
                            related.push(candidate);
                        }
                    }
                }
            }
        }
        "field" | "enum_constant" => {
            related.extend(store.find_references(symbol.id)?);
        }
        _ => {}
    }

    Ok(related)
}

fn build_new_qualified_name(old_qualified: &str, old_name: &str, new_name: &str) -> String {
    if let Some(pos) = old_qualified.rfind(old_name) {
        let mut result = String::with_capacity(old_qualified.len());
        result.push_str(&old_qualified[..pos]);
        result.push_str(new_name);
        result.push_str(&old_qualified[pos + old_name.len()..]);
        result
    } else {
        old_qualified.to_string()
    }
}

fn build_new_signature(old_signature: &str, old_name: &str, new_name: &str) -> String {
    if let Some(pos) = old_signature.find(old_name) {
        let mut result = String::with_capacity(old_signature.len());
        result.push_str(&old_signature[..pos]);
        result.push_str(new_name);
        result.push_str(&old_signature[pos + old_name.len()..]);
        result
    } else {
        old_signature.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_new_qualified_name_method() {
        assert_eq!(
            build_new_qualified_name("com.foo.Foo.save(Person)", "save", "findById"),
            "com.foo.Foo.findById(Person)"
        );
    }

    #[test]
    fn test_build_new_qualified_name_class() {
        assert_eq!(
            build_new_qualified_name("com.foo.UserService", "UserService", "AccountService"),
            "com.foo.AccountService"
        );
    }

    #[test]
    fn test_build_new_signature() {
        assert_eq!(
            build_new_signature("save(Person)", "save", "findById"),
            "findById(Person)"
        );
    }

    #[test]
    fn test_build_new_signature_no_params() {
        assert_eq!(
            build_new_signature("getAll()", "getAll", "findAll"),
            "findAll()"
        );
    }
}
