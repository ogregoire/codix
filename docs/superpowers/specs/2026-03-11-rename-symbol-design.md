# Rename Symbol Design

## Overview

Add a `codix rename` command that precisely renames a symbol (class, method, field, etc.) across the codebase. Uses the existing index to find affected files and tree-sitter AST to locate exact identifier nodes to replace. Dry-run by default, apply with `--apply`.

Java only in v1. Other languages return a clear "not supported" error.

## CLI Interface

```
codix rename <pattern> <new-name> [flags]
```

### Flags

| Flag | Short | Description |
|------|-------|-------------|
| `--apply` | | Apply the rename (dry-run by default) |
| `--kind` | `-k` | Filter by symbol kind (method, class, field...) |
| `--case-insensitive` | `-i` | Case-insensitive pattern matching |
| `--format` | `-f` | Output format: text (default) or json |

### Examples

```bash
# Dry-run: see what would change
codix rename 'com.foo.UserService.save(Person)' findById

# Apply it
codix rename 'com.foo.UserService.save(Person)' findById --apply

# Pattern match with kind filter
codix rename 'save*' findById -k method
```

### Disambiguation

When multiple symbols match, show suggested commands (same UX as existing relational commands):

```
Multiple symbols match 'save'. Be more specific:
  src/UserService.java:15  private field save
  → codix rename 'com.foo.UserService.save' findById -k field
  src/UserService.java:22  public method save(Person)
  → codix rename 'com.foo.UserService.save(Person)' findById -k method
```

### Output

**Dry-run (default):**

```
# Text
src/UserService.java:15:10  save → findById
src/UserService.java:42:8   save → findById
src/Repository.java:7:17    save → findById

3 occurrences in 2 files

# JSON (-f json)
{
  "changes": [
    {"file": "src/UserService.java", "line": 15, "column": 10, "old": "save", "new": "findById"},
    ...
  ],
  "summary": {"occurrences": 3, "files": 2}
}
```

**After `--apply`:**

```
Renamed 3 occurrences in 2 files
```

### Error Messages

| Situation | Message |
|-----------|---------|
| No match | `No symbol found matching 'xyz'. Try: codix find 'xyz*'` |
| Multiple matches | Disambiguation with suggested `codix rename` commands |
| Unsupported language | `Rename is not supported for [language] files. Supported: [list]` (dynamic) |
| Name conflict | `A method 'findById(Person)' already exists in com.foo.UserService at src/UserService.java:30` |
| Same name | `Symbol is already named 'save'. Nothing to rename.` |

## Plugin Trait Changes

### PluginCapability enum

```rust
enum PluginCapability {
    Rename,
}
```

### New trait methods

```rust
// In LanguagePlugin:
fn supports(&self, capability: PluginCapability) -> bool { false }

fn find_rename_occurrences(
    &self,
    tree: &tree_sitter::Tree,
    source: &[u8],
    symbol_name: &str,
    symbol_kind: &SymbolKind,
    symbol_qualified_name: &str,
) -> Result<Vec<RenameOccurrence>, RenameError>
```

Default `find_rename_occurrences` returns `RenameError::NotSupported`.

### Supporting types

```rust
struct RenameOccurrence {
    line: i64,        // 1-based
    column: i64,      // 0-based
    byte_offset: usize, // offset into source bytes (from tree-sitter node.start_byte())
    old_text: String,
}

enum RenameError {
    NotSupported { language: String },
}
```

```

`RenameError` is only for plugin-level errors (capability check). All other error conditions (no match, name conflict, same name) use `anyhow::bail!` in the engine/command handler, consistent with the existing codebase pattern.
```

## Engine Orchestration

New module: `src/engine/rename.rs`.

### Algorithm

1. **Incremental reindex** — same as all query commands, ensures index is fresh
2. **Resolve target symbol** — via `store.find_symbol()` with disambiguation
3. **Validate** — if new name equals old name, bail: `Symbol is already named 'save'. Nothing to rename.`
4. **Determine language** — look up the `FileRecord` for the symbol's `file_path` to get the language (language is on `FileRecord`, not `Symbol`). Find the corresponding plugin.
5. **Check plugin support** — verify `plugin.supports(PluginCapability::Rename)`. If not, error with dynamic list of supported languages built from `registry.all_plugins().filter(|p| p.supports(Rename))`.
6. **Check for name conflicts** — query the store for an existing symbol with the new name under the same parent. For methods, reconstruct the signature with the new name (e.g. `save(Person)` → `findById(Person)`) and check for a match. If conflict found, bail with the conflict error message.
7. **Collect related symbols from index** based on symbol kind:
   - **Class/Interface/Enum:** `find_references` (field types, extends, implements, annotations)
   - **Method:** `find_callers` + `find_implementations` (overrides). For the override chain: find the parent class, get its supertypes via `find_supertypes`, then query for methods with the same name/signature in those supertypes. Repeat up the hierarchy.
   - **Field:** `find_references`
8. **Collect affected files** — deduplicate the union of the declaring file plus files containing related symbols
9. **For each affected file:** read, create a `tree_sitter::Parser`, set language via `plugin.tree_sitter_language()`, parse the source, call `plugin.find_rename_occurrences()`
10. **Return results** sorted by file path, then line number

### Apply phase

When `--apply` is passed:

1. For each file, apply replacements **bottom-to-top** (highest `byte_offset` first) so earlier offsets aren't invalidated
2. Write modified content back to disk
3. Perform targeted store updates (see below)

### Targeted store update (no full reindex)

After applying the rename, update the index directly:

1. **Update the symbol** — name, qualified_name, signature
2. **Cascade for class renames** — update qualified_name of all child symbols
3. **Update relationships** — any `target_qualified_name` pointing to the old name
4. **Update file mtimes** — so incremental reindex doesn't re-process

## Store Changes

Four new methods on the `Store` trait:

```rust
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

fn update_file_mtime(
    &self,
    file_id: FileId,
    new_mtime: i64,
) -> Result<()>;
```

## Java Plugin: AST Occurrence Finding

The Java plugin walks the tree-sitter AST, matching identifier nodes by name and discriminating by **parent node type**.

### Method renames

Match identifier nodes where parent is:
- `method_declaration` — the declaration
- `method_invocation` — call sites (`obj.save()`, `save()`)
- `super_method_invocation` — `super.save()`

### Class/Interface/Enum renames

Match identifier/type_identifier nodes where parent is:
- `class_declaration`, `interface_declaration`, `enum_declaration`, `record_declaration` — the declaration
- Type usages — field types, parameter types, return types, `extends`/`implements` clauses, `new X()`, casts, `instanceof`, generic type arguments
- `import_declaration` — the simple name at the end
- `annotation` — `@X` if it's an annotation type
- `constructor_declaration` — constructors share the class name and must be renamed too

### Field renames

Match identifier nodes where parent is:
- `variable_declarator` inside `field_declaration` — the declaration
- `field_access` — `this.myField`, `obj.myField`
- `identifier` — bare `myField` reference within the same class

### Disambiguation

Parent node type is the discriminator. A `method_invocation > identifier` is a method reference. A `field_access > identifier` is a field reference. Tree-sitter provides this structural context.

## Edge Cases

- **Cross-language references:** If a symbol is referenced from a file whose language doesn't support rename, the dry-run warns: `Warning: 1 reference in src/utils.go skipped ([language] rename not supported)`
- **Override chains:** Renaming a method on an interface follows `find_implementations` to get the full override chain. All overrides are renamed.
- **Constructor impact from class rename:** Constructors share the class name. The plugin handles this via `constructor_declaration` nodes.

## Limitations (v1)

- **Java only** — other languages return "not supported" with dynamic list of supported languages
- **No local variables or parameters** — only indexed symbols (classes, methods, fields, etc.)
- **No reflection** — string references like `getMethod("save")` are not detected
- **No Javadoc** — `@link` and `@see` references are not renamed
- **No string literals** — occurrences inside strings are not detected
