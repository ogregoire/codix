# TODO

## Import Handling

- [x] Extract `import` declarations from Java files (both single-type and wildcard imports)
- [x] Use imports to resolve simple type names to fully qualified names during relationship extraction
- [x] Handle wildcard imports (`import com.foo.*`) by resolving against known symbols in the package
- [x] Same-package implicit resolution (classes in the same package reference each other without imports)
- [ ] Handle `import static` for method call resolution
- [ ] Handle `java.lang.*` implicit imports (String, Object, etc.)

## Method Call Resolution

- [x] Improve method call resolution beyond simple name matching — use field types and imports to narrow down which method is being called
- [x] Track receiver type for method invocations (e.g. `repo.save()` → resolve `repo` field type to find `Repository.save()`)

## Language Support

- [ ] Add Kotlin plugin
- [x] Add TypeScript/JavaScript plugin
- [x] Add Python plugin
- [x] Add Go plugin

## Ecosystem Plugins

- [ ] Maven plugin (parse `pom.xml` for module structure and dependencies)
- [ ] Gradle plugin (parse `build.gradle` / `build.gradle.kts`)

## Index Quality

- [ ] Add `.codixignore` support for excluding directories/files
- [x] Handle inner/nested classes (currently only top-level types are parents)
- [x] Track annotation usage as relationships
- [ ] Track generic type parameters
- [ ] Handle `throws` clause on methods
- [x] Extract return types and parameter types as relationships

## CLI Improvements

- [x] Add `--verbose` / `--debug` flag for diagnostics (show what files were reindexed, timing info)
- [x] Add `codix status` command (show index stats: file count, symbol count, stale files)
- [ ] Add `codix plugins` command (list registered plugins with name, display name, and description)
- [ ] Add `codix tree <symbol>` for transitive dependency graphs
- [ ] Support multiple patterns in a single query
- [ ] Allow plugins to access their own config via `plugin.<language>.<key>` namespace (e.g. `plugin.java.source-level 17`)

## Verification

- [ ] Verify the quality of the code
- [ ] Verify that the output of the help messages are still correct and relevant

## Performance

- [ ] Parallel file parsing during indexing (rayon)
- [ ] Benchmark incremental reindex on large projects (10k+ files)
