# codix

A fast code symbol indexer built for AI coding agents. codix indexes your codebase into a local SQLite database and provides precise symbol lookups, reference finding, and relationship queries — so AI agents can navigate code without burning tokens on grep and guesswork.

## Install

```bash
cargo install codix
```

## Quick Start

```bash
cd your-project
codix init          # Creates .codix/ and indexes all source files
codix find UserService
codix refs UserService
codix impls Repository
```

## Commands

**Project management**

| Command | Description |
|---------|-------------|
| `codix init` | Initialize project and index all files |
| `codix index` | Full reindex (drop and rebuild) |
| `codix status` | Show index statistics (file count per language) |
| `codix config` | Get or set configuration values (e.g. `index.languages`) |

**Symbol lookup**

| Command | Description |
|---------|-------------|
| `codix find <pattern>` | Find symbol definitions |
| `codix symbols <file>` | List symbols defined in a file |
| `codix package <pattern>` | List symbols in a package |

**Relationships**

| Command | Description |
|---------|-------------|
| `codix refs <pattern>` | Find symbols that reference a given symbol (extends, implements, calls, field types, annotations) |
| `codix impls <pattern>` | Find implementations/subclasses |
| `codix supers <pattern>` | Find supertypes (extends/implements) |
| `codix callers <pattern>` | Find callers of a method (resolves receiver types) |
| `codix callees <pattern>` | Find methods called by a method (resolves receiver types) |

When multiple symbols match a pattern, codix shows each match with a copy-pasteable command using the fully qualified name.

## Flags

| Flag | Description |
|------|-------------|
| `-v`, `--verbose` | Show diagnostic info (files reindexed, timing) |
| `-f`, `--format text\|json` | Output format (default: text) |
| `-i`, `--case-insensitive` | Case-insensitive pattern matching |
| `-k`, `--kind <kind>` | Filter by symbol kind (class, interface, enum, record, annotation, method, field, constructor, function, struct) |

## Patterns

Patterns are glob-based and match against both simple names and qualified names:

```bash
codix find UserService          # exact match
codix find "User*"              # glob
codix find "com.foo.*Service"   # qualified name glob
codix find "*" -k interface     # all interfaces
```

## Output

**Text** (default) — compact, one result per line, paths relative to your current directory:

```
src/main/java/com/foo/UserService.java:3  public class UserService
src/main/java/com/foo/UserService.java:6  public method save(Person)
```

**JSON** — structured output with `-f json`.

## How It Works

- Parses source files using [tree-sitter](https://tree-sitter.github.io/) for fast, accurate syntax analysis
- Stores symbols and relationships in a local SQLite database (`.codix/index.db`)
- Automatically re-indexes modified files on every query (mtime-based staleness detection)
- `codix index` forces a full rebuild when needed

## Language Support

Currently supported: **Go**, **Java**, **JavaScript/TypeScript**, **Python**, **Rust**

All languages are enabled by default. To install with only specific languages:

```bash
cargo install codix --no-default-features --features "lang-java,lang-python"
```

Available features: `lang-go`, `lang-java`, `lang-javascript`, `lang-python`, `lang-rust`.

## For AI Agents

codix is designed to be discovered and used by AI agents without documentation:

- Running `codix` or any subcommand without arguments shows helpful usage
- Text output is compact and token-efficient
- JSON output provides structured data when needed
- Every query auto-reindexes stale files, so the index is always fresh

## License

MIT
