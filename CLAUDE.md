# CLAUDE.md

Behavioral guidelines to reduce common LLM coding mistakes. Merge with project-specific instructions as needed.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.

---

## Project-Specific Context

### Architecture

```
src/
├── main.rs              # CLI entry point, command handlers, output formatting
├── cli/mod.rs            # Clap CLI definition (Commands, Flags)
├── model.rs              # Core types: Symbol, SymbolKind, Visibility, relationships, queries
├── store/
│   ├── mod.rs            # Store trait (abstract storage interface)
│   └── sqlite.rs         # SQLite implementation (schema, CRUD, queries)
├── engine/
│   ├── mod.rs
│   ├── project.rs        # Project root detection (.codix/), path utilities
│   └── indexer.rs        # Full index, incremental reindex, file discovery
└── plugin/
    ├── mod.rs            # LanguagePlugin trait, PluginRegistry
    └── java/mod.rs       # Java plugin (tree-sitter symbol + relationship extraction)
```

### Key Patterns

- **Store trait** (`src/store/mod.rs`): All storage access goes through this trait. SQLite is the current backend. To add a new backend, implement the trait.
- **LanguagePlugin trait** (`src/plugin/mod.rs`): Each language is a compile-time plugin. To add a language: create `src/plugin/<lang>/mod.rs`, implement the trait, register in `PluginRegistry::new()`.
- **Two-pass relationship resolution**: Plugins emit relationships with `target_qualified_name` (string). After all files are indexed, `resolve_relationships()` matches these against the symbols table (first by `qualified_name`, then by `name` as fallback).
- **Incremental reindex**: Every query command runs `incremental_reindex()` which checks mtimes. `codix index` does a full drop+rebuild.

### Conventions

- Line numbers: 1-based (tree-sitter gives 0-based rows, converted during extraction)
- Columns: 0-based
- Paths: stored relative to project root, displayed relative to CWD
- `RelationshipKind::as_str()` uses kebab-case (e.g. `"field-type"`) — matches serde serialization
- Method signatures: `name(Type1,Type2)` — no spaces, no parameter names

### Testing

- Unit tests: `cargo test` (47 tests in `src/`)
- Integration tests: `cargo test --test integration` (15 tests in `tests/`)
- `test-project/` contains sample Java files for manual testing
- After manual testing, clean up: `rm -rf test-project/.codix`

### Design Documents

- Spec: `docs/superpowers/specs/2026-03-10-codix-design.md`
- Plan: `docs/superpowers/plans/2026-03-10-codix-implementation.md`
