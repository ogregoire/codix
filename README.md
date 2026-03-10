# Codix

A fast code symbol indexer built for AI coding agents. Codix indexes your codebase into a local SQLite database and provides precise symbol lookups, reference finding, and relationship queries — so AI agents can navigate code without burning tokens on grep and guesswork.

## Install

```bash
cargo install --path .
```

## Quick Start

```bash
cd your-java-project
codix init          # Creates .codix/ and indexes all source files
codix find UserService
codix refs UserService
codix impls Repository
```

## Commands

| Command | Description |
|---------|-------------|
| `codix init` | Initialize project and index all files |
| `codix index` | Full reindex (drop and rebuild) |
| `codix find <pattern>` | Find symbol definitions |
| `codix refs <pattern>` | Find all references to a symbol |
| `codix impls <pattern>` | Find implementations/subclasses |
| `codix supers <pattern>` | Find supertypes (extends/implements) |
| `codix callers <pattern>` | Find callers of a method |
| `codix callees <pattern>` | Find methods called by a method |
| `codix symbols <file>` | List symbols in a file |
| `codix package <pattern>` | List symbols in a package |

## Flags

| Flag | Description |
|------|-------------|
| `-f`, `--format text\|json` | Output format (default: text) |
| `-i`, `--case-insensitive` | Case-insensitive pattern matching |
| `-k`, `--kind <kind>` | Filter by symbol kind (class, interface, enum, record, annotation, method, field, constructor) |

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

Currently supported: **Java**

The architecture uses a plugin trait (`LanguagePlugin`) that makes adding new languages straightforward — each language provides its tree-sitter grammar and symbol extraction logic.

## For AI Agents

Codix is designed to be discovered and used by AI agents without documentation:

- Running `codix` or any subcommand without arguments shows helpful usage
- Text output is compact and token-efficient
- JSON output provides structured data when needed
- Every query auto-reindexes stale files, so the index is always fresh

## License

MIT
