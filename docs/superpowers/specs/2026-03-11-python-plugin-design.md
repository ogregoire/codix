# Python Plugin Design

## Plugin Identity

- **Name:** `python`
- **Display name:** `Python`
- **Grammar:** `tree-sitter-python`
- **File extensions:** `.py`, `.pyi`

## Symbol Extraction

| Construct | Kind | Qualified Name | Notes |
|---|---|---|---|
| `class Foo:` | `class` | `Foo` | Top-level class |
| `class Inner:` (in class) | `class` | `Outer.Inner` | Nested class, `parent_local_id` points to outer |
| `def __init__(self):` | `constructor` | `Foo.__init__` | Special-cased as `constructor` kind |
| `def foo():` (module-level) | `function` | `foo` | |
| `def foo(self):` (in class) | `method` | `Foo.foo` | Includes `@staticmethod` and `@classmethod` — still kind `method` |
| `name: str` (class body) | `field` | `Foo.name` | Class-body annotations and assignments |

**Signatures:** `name()` always (no types in signature), excluding `self` and `cls` parameters. Example: `def foo(self, x: int, y: str)` → signature `foo(x,y)`.

**`type_text`:** Populated from type annotations where present. For fields: the annotation type (`str`, `List[int]`). For methods: the return type annotation if present.

## Package

Empty string — consistent with Rust, JS, Go plugins.

## Visibility

Determined by naming convention:

| Pattern | Visibility |
|---|---|
| `__name` (double underscore prefix, no trailing `__`) | `private` |
| `_name` (single underscore prefix) | `protected` |
| Everything else (including `__dunder__` methods) | `public` |

## Relationships

| Kind | Source | Target |
|---|---|---|
| `Extends` | class | Each base class in `class Foo(Bar, Baz)` |
| `FieldType` (field) | field | Type annotation on class attribute |
| `FieldType` (param) | method/function | Each annotated parameter type |
| `FieldType` (return) | method/function | Return type annotation |
| `AnnotatedBy` | any symbol | Decorator name (`@dataclass` → `dataclass`) |
| `Calls` | function/method | Function/method call expressions in body |

**`wildcard_imports`:** Returns empty vec (import resolution is out of scope for v1).

## Out of Scope (v1)

- `import` resolution for type references
- Lambda expressions as symbols
- `@property` changing kind to `property`
- `self.name = value` assignments in `__init__` as fields (only class-body declarations)
- Module-level variable assignments as symbols
