# Method Call Resolution — Design Spec

> **For agentic workers:** This spec describes the design for method call resolution in codix. Use superpowers:writing-plans to create the implementation plan.

**Goal:** Resolve method call targets to qualified `ClassName.methodName` keys using receiver type inference from local scope data — replacing the current simple-name-only matching. Store rich data cheaply at index time, resolve at query time.

**Architecture:** The Java plugin builds a per-method scope map (fields, parameters, `this`) to resolve receiver types at call sites. Resolved targets are stored as `ClassName.methodName` in relationships. `find_callers`/`find_callees` match via method key at query time. Single-pass indexing, no cross-file lookups during extraction. The core remains completely language-agnostic.

## Current Problem

Method calls like `repo.save(person)` are stored with only the simple method name `"save"` as the target. The COALESCE fallback matches any `save` method in the entire codebase, producing false positives when multiple classes define methods with the same name.

## Richer Symbol Metadata

Add `type_text TEXT` column to the `symbols` table:

- **Fields:** the declared type, resolved via imports (e.g., `"com.foo.Repository"`)
- **Methods:** the return type, resolved via imports (e.g., `"com.foo.Person"`). NULL for void methods and constructors.
- **Classes/interfaces/enums:** NULL

Add `type_text: Option<String>` to `ExtractedSymbol`. The core treats this as an opaque nullable text field with no language-specific semantics.

## Scope-Based Receiver Resolution (Java Plugin)

During extraction of a method body, the Java plugin builds a scope map (`HashMap<String, String>`: identifier name → qualified type) from three sources:

1. **Fields** of the enclosing class — name → qualified type (resolved via imports)
2. **Parameters** of the current method — name → qualified type (resolved via imports)
3. **`this`** → enclosing class qualified name

For `repo.save(person)`: look up `repo` in scope → `com.foo.Repository` → emit `target_qualified_name = "com.foo.Repository.save"`.

For `this.save()` or unqualified `save()`: resolve against enclosing class → `"com.foo.MyClass.save"`.

For unresolved receivers (local variables, chained calls, `var`): leave as simple name `"save"` — existing COALESCE fallback handles it.

This only uses data available locally during extraction (fields, params, imports). No cross-file lookups. No two-pass indexing. Single pass.

## Query-Time Method Matching

**Method key format:** The method key is `parent_class_qualified_name + "." + method_name` — e.g., `"com.foo.Repository.save"`. Note this is NOT the method's own `qualified_name` (which includes the signature, e.g., `"com.foo.Repository.save(Person)"`). The method key is constructed at query time by joining through `parent_symbol_id` to get the parent class's `qualified_name`, then concatenating with the method's `name`.

**`resolve_relationships`:** Unchanged. COALESCE continues resolving all relationship kinds. For method-key targets like `"com.foo.Repository.save"`, COALESCE won't find a match (symbol qualified names include signatures), so `target_symbol_id` stays NULL — the method-key matching in `find_callers`/`find_callees` handles these. For simple-name targets like `"save"`, COALESCE's name fallback still sets `target_symbol_id`, preserving existing behavior.

**`find_callers(symbol_id)`:**

```sql
-- Given a method symbol_id, find its callers.
-- Step 1: construct the method key from the target symbol
-- Step 2: match against target_qualified_name in Calls relationships
SELECT s.id, s.name, ... FROM relationships r
JOIN symbols s ON s.id = r.source_symbol_id
JOIN files f ON s.file_id = f.id
WHERE r.kind = 'calls'
AND r.target_qualified_name = (
    SELECT parent.qualified_name || '.' || target.name
    FROM symbols target
    JOIN symbols parent ON parent.id = target.parent_symbol_id
    WHERE target.id = ?1
)
```

Falls back to the existing `target_symbol_id`-based query for unresolved calls (union with `WHERE r.target_symbol_id = ?1 AND r.kind = 'calls'`).

**`find_callees(symbol_id)`:**

```sql
-- Given a method symbol_id, find what it calls.
-- Join Calls relationships with method symbols via method key.
SELECT callee.id, callee.name, ... FROM relationships r
JOIN symbols parent ON parent.qualified_name || '.' || callee.name = r.target_qualified_name
JOIN symbols callee ON callee.parent_symbol_id = parent.id
    AND callee.kind IN ('method', 'constructor')
JOIN files f ON callee.file_id = f.id
WHERE r.source_symbol_id = ?1 AND r.kind = 'calls'
```

Falls back to the existing `target_symbol_id`-based query for unresolved calls.

**Overload handling:** If `Repository` has `save(Person)` and `save(String)`, querying callers of either returns ALL callers of both. Querying callees returns both overloads. Acceptable imprecision.

**Index requirement:** Add `CREATE INDEX idx_relationships_target_qname_kind ON relationships(target_qualified_name, kind)` for the new query pattern.

## Language-Agnostic Core

All changes to the core (model, store, indexer) are language-agnostic:

- `symbols` table: add `type_text TEXT` column. Schema migration: `ALTER TABLE symbols ADD COLUMN type_text TEXT` for existing databases, or drop and recreate (codix supports `codix index` for full rebuild).
- `ExtractedSymbol`: add `type_text: Option<String>` — opaque field
- `insert_symbols`: add `type_text` to the INSERT statement
- `find_callers`/`find_callees`: method key = `parent_qualified_name + "." + name` — string concatenation, no language syntax assumptions
- `resolve_relationships`: unchanged (COALESCE continues for all kinds; method-key targets stay unresolved, simple-name targets still work)
- Add index: `idx_relationships_target_qname_kind ON relationships(target_qualified_name, kind)`
- Indexer: unchanged (single pass)

All Java-specific logic lives in the plugin:

- Scope map construction (fields, params, `this`)
- Receiver resolution using scope map + import map
- Extracting `type_text` from Java AST nodes
- Formatting resolved targets as `ReceiverType.methodName`

## Testing Strategy

**Java plugin unit tests:**
- Field receiver: `repo.save()` → target = `"com.foo.Repository.save"`
- Parameter receiver: `void work(Repository r) { r.save(); }` → target = `"com.foo.Repository.save"`
- `this.save()` → target = `"com.foo.MyClass.save"`
- Unqualified `save()` → target = `"com.foo.MyClass.save"`
- Unresolved receiver (local var) → target = `"save"` (simple name fallback)
- `type_text` on fields: `private Repository repo` → `"com.foo.Repository"`
- `type_text` on methods: `Person findById(int id)` → `"com.foo.Person"`
- `type_text` NULL for void methods and constructors

**Store/query tests:**
- `find_callers`: matches via method key, returns correct callers
- `find_callees`: resolves method key to method symbols
- Unresolved calls still work via COALESCE fallback

**Integration tests:**
- `codix callers` finds caller via receiver-resolved method
- `codix callees` resolves targets correctly

**Test project:** Add files with cross-package field receivers and method calls.

## Out of Scope

- Chained method calls (`a.getB().doC()`) beyond one level
- `var` type inference (can be added later using stored `type_text` return types)
- Argument type matching for overload disambiguation
- Static method calls / `import static` resolution
- Local variable type tracking
