# Inner/Nested Classes and Annotation Usage — Design Spec

> **For agentic workers:** This spec describes the design for inner class support and annotation usage tracking in codix. Use superpowers:writing-plans to create the implementation plan.

**Goal:** Extract inner/nested classes as symbols with proper qualified names and parent linkage, and track annotation usage on symbols as `AnnotatedBy` relationships.

**Architecture:** The Java plugin's `extract_members` becomes recursive — nested type declarations inside class bodies are extracted as symbols and their members are recursively processed. Annotation nodes on any symbol emit `AnnotatedBy` relationships resolved via the import map. The core stays language-agnostic: only `AnnotatedBy` is added to `RelationshipKind`.

## 1. Inner/Nested Classes

### Current Problem

`extract_type_declaration` only runs on direct children of the root node. `extract_members` only extracts methods, constructors, and fields — it silently ignores nested type declarations inside class bodies. Inner classes like `Outer.Inner` are invisible to the index.

### Design

Make `extract_members` recursive. When it encounters a type declaration (`class_declaration`, `interface_declaration`, `enum_declaration`, `record_declaration`, `annotation_type_declaration`) inside a class body, it:

1. Extracts the nested type as a symbol with `parent_local_id` pointing to the enclosing type
2. Constructs `qualified_name` as `parent_qualified_name + "." + name` (e.g., `com.foo.Outer.Inner`)
3. Recurses into the nested type's body to extract its members (methods, fields, and further nested types)

The body kind matching in `extract_members` must also handle `annotation_type_declaration` → `annotation_type_body`.

The top-level loop in `extract_symbols` stays unchanged — it only handles root-level types. All nesting is handled by `extract_members`.

**Relationship extraction must also recurse.** `extract_type_relationships` / `extract_body_relationships` currently only run on top-level types. They must also recurse into nested type declarations to extract their relationships (extends, implements, field-type, calls). Without this, inner class symbols would be extracted but their relationships silently dropped.

### Scope Map Impact

The receiver resolution scope map already works per-method. For methods inside inner classes, the scope map uses the inner class's fields and the inner class as `this`. Outer class fields are not included in the inner class scope — this is a known limitation (acceptable imprecision, same as not resolving local variables).

### Relationship Resolution

Inner class qualified names like `com.foo.Outer.Inner` follow the same pattern as top-level types, so `resolve_relationships` works unchanged.

### Out of Scope

- Anonymous inner classes (no name to index)
- Local classes inside method bodies

## 2. Annotation Usage

### Current Problem

`SymbolKind::Annotation` exists and `annotation_type_declaration` (`@interface`) is extracted at the top level, but `@` annotation usage (e.g., `@Override`, `@Deprecated` on methods) is completely unhandled.

### Design

Add `AnnotatedBy` to `RelationshipKind` (serialized as `"annotated-by"`).

During symbol extraction, for each symbol node (type, method, constructor, field), check for annotation child nodes in the tree-sitter AST. Tree-sitter-java uses two node kinds: `marker_annotation` (e.g., `@Override`) and `annotation` (e.g., `@SuppressWarnings("unchecked")`) — both must be handled. For each annotation found:

- Extract the annotation name (e.g., `Override`, `Deprecated`)
- Resolve it via the import map (e.g., `Deprecated` → `java.lang.Deprecated`)
- Emit `ExtractedRelationship { source = symbol_local_id, target_qualified_name = resolved_name, kind = AnnotatedBy }`

For formal parameters (which are not symbols), annotations are attached to the enclosing method. This means `@NotNull` on a parameter shows up as the method being related to `NotNull`.

### Querying

`codix refs Override` finds all symbols annotated with `@Override` via `find_references`, since `AnnotatedBy` relationships are resolved by `resolve_relationships` like any other kind. `find_references` returns all relationship kinds together — this is acceptable since annotation types are rarely also used as supertypes or field types.

### Out of Scope

- Annotation arguments/values
- Meta-annotations
- Annotations on enum constants (enum constants are not symbols)

## 3. Core Changes (Language-Agnostic)

- Add `AnnotatedBy` to `RelationshipKind` enum + `as_str()` → `"annotated-by"`
- No schema migration needed (`kind` is stored as text)
- No store/query changes (`resolve_relationships` handles `AnnotatedBy` via COALESCE, `find_references` returns all relationship kinds)

All Java-specific logic stays in the plugin.

## 4. Testing Strategy

**Java plugin unit tests:**
- Nested class: `Outer.Inner` extracted with correct qualified name and `parent_local_id`
- Deeply nested: `Outer.Middle.Inner` works recursively
- Inner class members: methods/fields inside inner classes are extracted with correct parent
- Annotation on class: `@Deprecated public class Foo` → `AnnotatedBy` relationship
- Annotation on method: `@Override void save()` → `AnnotatedBy` relationship
- Annotation on constructor: `@Inject public Svc()` → `AnnotatedBy` relationship
- Annotation on field: `@Inject private Repo repo` → `AnnotatedBy` relationship
- Annotation on parameter (attached to method): `void save(@NotNull String s)` → method has `AnnotatedBy` relationship to `NotNull`
- Annotation resolved via imports: `@com.foo.Custom` or imported `@Custom` → qualified target

**Integration tests:**
- `codix symbols` shows inner classes
- `codix refs AnnotationName` finds annotated symbols
