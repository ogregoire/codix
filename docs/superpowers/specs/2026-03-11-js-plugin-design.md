# JavaScript Plugin Design

## Prerequisite: SymbolKind Refactor

`SymbolKind` changes from a fixed enum to a string wrapper. Each plugin defines its own valid kinds. CLI kind validation is removed from the core and delegated to plugins.

## Plugin Identity

- Name: `js`
- Display name: `JavaScript`
- Grammar: `tree-sitter-tsx` (superset that handles JS, JSX, TS, TSX)
- File extensions: `.js`, `.mjs`, `.cjs`, `.jsx`, `.ts`, `.tsx`

## Symbol Extraction

| JS construct | Kind | Qualified name |
|---|---|---|
| `class Foo {}` | `class` | `Foo` |
| `function foo() {}` | `function` | `foo` (top-level) |
| `const foo = () => {}` | `function` | `foo` (top-level) |
| `const foo = function() {}` | `function` | `foo` (top-level) |
| Class method / getter / setter | `method` | `Foo.bar` |
| Class field | `field` | `Foo.x` |
| `constructor()` | `constructor` | `Foo.constructor` |

## Package

Empty string. JavaScript has no package concept.

## Qualified Names

No package prefix. Top-level symbols use their name directly. Class members use `ClassName.memberName`. Method signatures use `name()` (no parameter types since JS is untyped).

## Visibility

`public` for everything, `private` for `#private` fields/methods.

## Relationships

- **Extends** — `class Foo extends Bar`
- **Calls** — method/function call resolution with receiver inference

## Out of Scope (v1)

- Import/export tracking
- Standalone `const`/`let`/`var` declarations
- Object literal methods
- Dynamic properties
- Field type tracking (JS is untyped)
