# Java Import Handling — Design Spec

> **For agentic workers:** This spec describes the design for Java import handling in codix. Use superpowers:writing-plans to create the implementation plan.

**Goal:** Resolve Java type references to fully qualified names using import declarations and same-package rules, replacing the current fragile simple-name fallback.

**Architecture:** Import resolution happens in two places: (1) the Java plugin resolves single-type imports and same-package references at extraction time, (2) the indexer resolves wildcard imports in a post-index pass using the store. The existing COALESCE fallback remains as a last resort.

## Current Problem

The Java plugin extracts type references as simple names (e.g., `Repository` instead of `com.foo.Repository`). The store's `resolve_relationships()` uses a COALESCE fallback that tries qualified name first, then simple name. This breaks when multiple symbols share the same simple name across different packages.

## Resolution Priority Chain

1. **Single-type import** — `import com.foo.Repository` maps `Repository` → `com.foo.Repository`
2. **Same-package implicit** — classes in the same package reference each other without imports; `package com.foo` + `Bar` → `com.foo.Bar`
3. **Wildcard import** — post-index pass: `import com.foo.*` + `Bar` → check if `com.foo.Bar` exists in index
4. **COALESCE fallback** — existing behavior, last resort

## Data Model Changes

`ExtractionResult` gains a new field:

```rust
pub struct ExtractionResult {
    pub symbols: Vec<ExtractedSymbol>,
    pub relationships: Vec<ExtractedRelationship>,
    pub wildcard_imports: Vec<String>,  // e.g. ["com.foo", "com.bar"]
}
```

No new database tables. Wildcard prefixes are transient (in-memory only during indexing).

## Java Plugin Changes

1. **Parse import declarations** from the AST — walk top-level nodes for `import_declaration`:
   - Single-type: `import com.foo.Repository` → add to import map (`HashMap<String, String>`)
   - Wildcard: `import com.foo.*` → add `"com.foo"` to wildcard list
   - Static imports (`import static`): ignored (method call resolution is a separate feature)

2. **Resolve type names during relationship extraction** — when emitting an `ExtractedRelationship`:
   - Look up simple name in import map → if found, use qualified name
   - Otherwise, prepend file's own package (same-package implicit)
   - If no package (default package), leave as simple name

3. **Return wildcard prefixes** in `ExtractionResult::wildcard_imports`.

4. **Method call targets** (`Calls` relationships) remain simple names — receiver type tracking is a separate feature.

## Indexer Changes

1. **During `index_file()`**: capture `wildcard_imports` from `ExtractionResult`, associate with file_id in an in-memory `HashMap<FileId, Vec<String>>`.

2. **New function `resolve_wildcard_imports(store, wildcard_map)`** — runs after all files are indexed, before `resolve_relationships()`:
   - Query relationships where `target_symbol_id IS NULL`
   - For each unresolved relationship, look up the file's wildcard prefixes
   - For each prefix, check if `{prefix}.{simple_name}` matches a symbol's `qualified_name`
   - If exactly one match across all prefixes: update `target_qualified_name`
   - If multiple matches (ambiguous): leave unresolved

3. **Ordering**: index all files → resolve wildcards → resolve relationships.

4. **Incremental reindex**: wildcard map only covers files reindexed in this run. Previously resolved relationships are unaffected.

## Testing Strategy

**Java plugin unit tests:**
- Single-type imports resolve to qualified names
- Same-package references resolve without imports
- Wildcard imports: simple names left unresolved, prefixes returned
- Mix of import types
- Default package (no package): names left as simple names
- Static imports: ignored

**Indexer unit tests:**
- Wildcard resolution: cross-package reference via `import com.foo.*`
- Ambiguous wildcard (multiple prefixes match): stays unresolved
- No wildcard match: falls through to COALESCE

**Integration tests:**
- `codix refs` on a symbol referenced via import in another file
- Same-package reference without explicit import

**Test project expansion:** Add second package (e.g., `com.bar`) with cross-package imports.

## Out of Scope

- Static import resolution
- Method receiver type tracking
- `ParameterType`, `ReturnType`, `Throws` relationship extraction
- `java.lang.*` implicit imports (could be added later)
