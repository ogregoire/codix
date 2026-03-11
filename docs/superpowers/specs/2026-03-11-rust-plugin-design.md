# Rust Plugin Design

## Prerequisite: Visibility Refactor

`Visibility` changes from a fixed enum to a string wrapper, same as SymbolKind. Each plugin defines its own visibility values.

## Plugin Identity

- Name: `rust`
- Display name: `Rust`
- Grammar: `tree-sitter-rust`
- File extension: `.rs`

## Symbol Extraction

| Rust construct | Kind | Qualified name |
|---|---|---|
| `struct Foo {}` / tuple / unit | `struct` | `Foo` |
| `enum Color { Red, Blue }` | `enum` | `Color` |
| Enum variant `Red` | `variant` | `Color.Red` |
| Enum variant field `r: u8` | `field` | `Color.Blue.r` |
| `trait Foo {}` | `trait` | `Foo` |
| `type Foo = Bar` | `type-alias` | `Foo` |
| `fn foo()` (top-level) | `function` | `foo` |
| `impl Foo { fn bar() {} }` | `method` | `Foo.bar` |
| `impl Trait for Foo { fn baz() {} }` | `method` | `Foo.baz` |
| Trait method signature | `method` | `TraitName.bar` |
| Struct field | `field` | `Foo.x` |

## Package

Empty string. Rust modules are file-path based with no in-file declaration.

## Visibility

String values matching Rust syntax: `pub`, `pub(crate)`, `pub(super)`, `private`.

## Use Declaration Tracking

- `use path::Type;` maps `Type` to `path::Type`
- `use path::{A, B};` maps `A` to `path::A`, `B` to `path::B`
- `use path::*;` wildcard import (same mechanism as Java)
- `use path::Type as Alias;` maps `Alias` to `path::Type`
- `crate::`, `super::`, `self::` prefixes stored as-is

## Relationships

- **Implements** — `impl Trait for Struct`
- **Extends** — `trait Foo: Bar + Baz` (supertraits)
- **FieldType** — struct field types, parameter types, return types, type alias targets
- **Calls** — function/method calls
