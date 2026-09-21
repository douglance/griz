# griz agent guidance

## Product

- griz edits code as composable primitives: `read`, `find`, `plan`, `select`, `diff`, `apply`, `undo`, `log`, `get`. Write `griz` lowercase in user-facing text.
- Every command is one primitive whose output feeds another. Keep outputs as plain JSON carrying ids, byte ranges, and fingerprints; never make a caller parse prose.
- Serve MCP with direct discovery so each command is its own tool. A host's Code Mode, such as apoc's, is where primitives compose; griz does not embed its own Code Mode or any model call.
- The transform lives in the caller's program. griz never decides which match matters or what an edit should say.

## Guarantees

- Never write a source file outside `apply` and `undo`. `griz-core` only reads files it is handed; `cargo xtask check` rejects write, spawn, or database calls there.
- Every returned match and every planned file carries the fingerprint of the text it was computed against. `apply` writes nothing unless every file still has it, except through a clean three-way merge the caller opted into.
- Stage every file before journaling an operation as `applying`, and journal before the first rename. Recovery on open must be total: roll files at their old fingerprint forward, leave files at neither fingerprint alone and list them.
- A tolerant match is never `machine` confidence. Keep the ladder order exact, trailing whitespace, indentation, trimmed: every indentation match is also a trimmed match, so the stricter rung must run first.
- A structural pattern match is exact identity on the parse tree, machine confidence, never a tolerant rung.
- An ambiguous anchor is an error listing every candidate. Never take the first match.
- Undo restores only files still at the operation's written fingerprint. An undo is an operation and can be undone.
- Mutations take `purpose` and `idempotency_key`, scoped per command. Same key and input replays; different input is `IDEMPOTENCY_CONFLICT`. Release a key only when the command failed validation before changing anything.
- Answer mutations with `{id, outcome}` using `passed`, `failed`, `error`. Raising verbosity only adds keys; `trace` returns the full record and still carries `outcome`. A verdict other than `passed` exits nonzero.

## Engineering

- Dependency direction is `griz → griz-store → griz-core`. `cargo xtask check` enforces it.
- Every crate inherits workspace lints. Rust files have at most 300 physical lines; functions and closures at most 60 code lines. Complexity is limited to 10, nesting to 3, and function arguments to 5. Suppress lints only narrowly and only in tests.
- The nesting limit counts an `impl` block and a struct literal as levels of their own, so a loop with an `if` inside a method already trips it. Move the work to a free function, give the type a small constructor, and combine conditions instead of nesting them. Leave headroom under 300 lines in a file two changes might both grow.
- Tests never `unwrap`, `expect`, or `expect_err`: return `TestResult` and use `?`, or `let ... else { panic!(...) }`.
- Keep tests in one test crate per package (`tests/<name>/main.rs` with modules) so shared fixtures never trip unused-code lints.
- Isolate `GRIZ_HOME` and `XDG_DATA_HOME` in every test that runs the binary. The apoc composition test also isolates `APOC_HOME`, `APOC_MCP_ROOT`, and `APOC_RUNTIME_DIR`, and stops its daemon on drop.
- Test a guarantee by breaking it: drop the fingerprint guard, take the first match, mark fuzzy matches `machine`, skip recovery or locks, replay a key as new. Each must turn a named test red. After restoring a mutated file, touch it so cargo rebuilds it.
- Run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `cargo xtask check` before committing.
- Use conventional commits.
