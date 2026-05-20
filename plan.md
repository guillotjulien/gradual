# Implementation Plan: `gradual` (working title)

A conflict-free TypeScript + ESLint gradual improvement tool.
Built as a replacement for Betterer, using an append-only delta event log stored in git.

**Stack:** Rust · tree-sitter (native crate) · xxHash 128-bit · clap · serde_json · anyhow

---

## Why this tool exists
 
[Betterer](https://github.com/phenomnomnominal/betterer) solves a real problem: it lets teams
adopt stricter TypeScript or ESLint rules gradually, ratcheting the number of violations down
over time rather than fixing everything at once. The issue is how it stores state. Betterer
commits a single results file to the repository that records the current number of violations
per rule. On an active team, this file becomes a constant source of merge conflicts: two
developers each improve the codebase on separate branches, both modify the results file, and
neither can merge without rebasing, waiting for CI, and hoping nobody else merges in the
meantime. In practice this means hours of lost time per week on a large team.
 
`gradual` solves the same problem without the conflicts. Instead of a single mutable results
file, it maintains an append-only log of delta events — one small JSON file per commit,
never edited after creation. Because no two commits ever write to the same file, merge
conflicts on the baseline state are structurally impossible by design. The tool is also
deliberately narrow: it only supports TypeScript type checking and ESLint, which lets it
discard all the generality that makes Betterer slow and complex.
 
---
 
## Key design decisions
 
**Append-only delta event log, not a snapshot file.**
Each commit that changes the finding baseline writes a new file to
`.gradual/events/YYYY/MM/DD/<timestamp>-<sha>.json`. Files are never edited after creation.
Two branches working in parallel always write files with unique names, so git merges are
trivially clean set unions — no conflict is possible. The current baseline state is derived
by replaying ("folding") all delta files in order, which is fast enough at any realistic
event log size that no caching is needed for v1.
 
**Deltas, not snapshots.**
Each event records only what *changed* — findings added and findings removed — not the full
current state of the codebase. This is critical for correctness under rebase. If a branch
records a full snapshot ("as of this commit, file X has these findings"), rebasing that
branch onto new work can make the snapshot silently wrong: the rebased snapshot overwrites
findings introduced by the base branch. A delta only asserts what *this commit* changed,
which is invariant under rebase. Two branches that touch different findings compose
correctly regardless of merge order.
 
**Delta operations are idempotent set operations.**
"Add finding F" means "ensure F is present"; "remove finding F" means "ensure F is absent."
If two branches independently remove the same finding (because two developers fixed the same
issue), both removals are recorded and the fold produces the correct result — F is absent —
without error or conflict. Same for two branches independently introducing the same finding.
The fold always produces a deterministic result regardless of the order parallel branches land.
 
**Stable finding identity via tree-sitter + xxHash.**
Findings from tsc and ESLint are reported as `(file, line, column)`. Line numbers are
unstable — adding an import shifts every finding below it. Instead, we use tree-sitter to
look up the AST node at the finding's position and derive a composite identity from:
the rule ID, the repo-relative file path, the named scope path (e.g. `AuthService.validateToken`),
the enclosing AST node kind and normalized text, and an occurrence index for disambiguating
identical nodes in the same scope. This identity is stable across reformatting, line shifts,
and unrelated edits, but changes when the flagged code itself is edited — which is the
intended behavior. The composite is hashed with xxH3-128 (fast, non-cryptographic,
negligible collision probability). Tree-sitter is used as a uniform position-to-node
mapper for all tools, so adding a new analyzer in the future requires no changes to the
identity layer.
 
**`update` fails on regressions; `--force` is the explicit escape hatch.**
`gradual update` records the current finding state as a new delta event, but refuses to
write if the delta contains any new findings (regressions). This enforces the ratchet: the
baseline can only move forward. When a regression is genuinely intentional (a necessary
refactor that temporarily introduces violations), `gradual update --force` accepts it after
an explicit interactive confirmation. The `--force` flag also makes regressions visible in
code review, because the committed delta file will contain a non-empty `added` array that
reviewers can see in the diff.
 
**Rust, not TypeScript or Node.**
The tool is written in Rust for three reasons. First, tree-sitter is a Rust-native library —
using it from Rust means no cgo, no WASM, no native addon compilation, and true static
binaries. Second, a compiled binary has no runtime dependency: developers don't need Node,
npm, or any particular Node version installed to use the tool. Third, Rust's type system
makes the fold algorithm and event serialization logic difficult to get subtly wrong, which
matters because silent correctness failures (lost findings, phantom regressions) are the
worst possible failure mode for this tool. The binary is distributed as a single executable
and invoked directly from the git hook.
 
**No daemon mode in v1.**
A long-lived daemon that keeps TypeScript programs warm in memory would make the pre-commit
hook dramatically faster. It is deliberately out of scope for v1. The current Betterer hook
takes 20–30 seconds and developers are accustomed to it, so matching that speed is
acceptable. Daemon mode is the primary v2 investment once the core correctness story is
proven.
 
**No plugin system.**
Betterer's generality (supporting arbitrary "test" functions via a plugin architecture) is
the root cause of its complexity and much of its slowness. `gradual` hardcodes TypeScript
and ESLint as the only analyzers. This is not a limitation to be designed around later —
it is a deliberate constraint that keeps the tool simple, fast to build, and easy to reason
about. If a third analyzer is ever needed, it will be added as a first-class built-in, not
via a plugin API.

---

## Architecture recap (what we're building)

- **Append-only event log** in `.gradual/events/YYYY/MM/DD/<timestamp>-<sha>.json`
- **Delta events** (not snapshots): each commit records `added` and `removed` finding IDs
- **Stable finding IDs** via tree-sitter AST node lookup from `(file, line, col)`, hashed with xxH3-128
- **Fold algorithm** replays deltas to produce current baseline state
- **Two commands**: `check` (read-only gate) and `update` (write delta, fails on regressions)
- **Git hook** integration via a simple shell script calling the compiled binary

---

## Repository structure

```
gradual/
├── Cargo.toml
├── Cargo.lock                        # commit this — it's a binary, not a library
├── src/
│   ├── main.rs                       # Entry point, clap command routing
│   ├── config.rs                     # Config loader (gradual.toml or gradual.json)
│   ├── analyzers/
│   │   ├── mod.rs
│   │   ├── types.rs                  # Shared RawFinding type
│   │   ├── typescript.rs             # Run tsc/tsgo, parse output → Vec<RawFinding>
│   │   └── eslint.rs                 # Run ESLint, parse output → Vec<RawFinding>
│   ├── identity/
│   │   ├── mod.rs
│   │   ├── parser.rs                 # tree-sitter parser setup, parse cache
│   │   ├── walker.rs                 # find_meaningful_enclosing, collect_named_scope_path
│   │   └── hasher.rs                 # compute_finding_id(RawFinding) → String
│   ├── events/
│   │   ├── mod.rs
│   │   ├── types.rs                  # DeltaEvent, Finding structs
│   │   ├── reader.rs                 # Read + sort all event files from disk
│   │   ├── fold.rs                   # fold(events) → HashMap<FindingId, Finding>
│   │   └── writer.rs                 # Serialize + write delta event file
│   ├── commands/
│   │   ├── mod.rs
│   │   ├── check.rs
│   │   ├── update.rs
│   │   ├── init.rs
│   │   └── install_hook.rs
│   └── git.rs                        # get_current_sha, get_parent_sha
└── tests/
    ├── identity/
    │   ├── hasher_tests.rs           # Edit-stability test suite (critical)
    │   └── fixtures/                 # .ts files used as hasher test inputs
    ├── events/
    │   ├── fold_tests.rs
    │   └── fixtures/                 # Sample delta event JSON files
    └── analyzers/
        ├── typescript_tests.rs
        └── eslint_tests.rs
```

---

## Cargo.toml dependencies

```toml
[package]
name = "gradual"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "gradual"
path = "src/main.rs"

[dependencies]
# CLI
clap = { version = "4", features = ["derive"] }

# Error handling — use anyhow everywhere, no raw unwrap() outside tests
anyhow = "1"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Hashing
xxhash-rust = { version = "0.8", features = ["xxh3"] }

# Tree-sitter — native Rust, no cgo, no WASM
tree-sitter = "0.22"
tree-sitter-typescript = "0.21"

# Filesystem
walkdir = "2"

# Time (for event file naming)
chrono = { version = "0.4", features = ["serde"] }

[dev-dependencies]
# No extra test framework needed — use built-in #[test] and #[cfg(test)]
# For fixture file loading, use std::fs directly
tempfile = "3"   # for write tests that need a temp directory
```

**Why no async / tokio:** v1 shells out to tsc and ESLint synchronously. Async adds
complexity with no benefit until daemon mode. Add tokio in v2.

**Why no `thiserror`:** `anyhow` is sufficient for a binary. `thiserror` is for libraries
that need typed errors consumers can match on. Use `anyhow::bail!` and `anyhow::Context`
throughout.

---

## Phase 1 — Project scaffolding and types

**Goal:** empty project that compiles, all shared types defined, `cargo test` passes.

### Tasks

1. `cargo new gradual --bin`, set edition 2021, add all dependencies above.

2. Define `RawFinding` in `src/analyzers/types.rs`:
   ```rust
   #[derive(Debug, Clone)]
   pub struct RawFinding {
       pub tool: Tool,
       pub rule: String,      // "ts:2304" or "eslint:no-unused-vars"
       pub file: PathBuf,     // absolute path
       pub line: u32,         // 1-indexed
       pub column: u32,       // 1-indexed
       pub message: String,
   }

   #[derive(Debug, Clone)]
   pub enum Tool {
       TypeScript,
       Eslint,
   }
   ```

3. Define event types in `src/events/types.rs`:
   ```rust
   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct Finding {
       pub id: String,
       pub rule: String,
       pub file: String,      // repo-relative path, always forward slashes
       pub line: u32,         // display only, not part of identity
       pub message: String,
   }

   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct DeltaEvent {
       pub version: u32,      // always 1 for now
       pub commit: String,
       pub parent: String,
       pub timestamp: String, // ISO 8601
       pub added: Vec<Finding>,
       pub removed: Vec<String>, // IDs only
   }
   ```

4. Define `GradualConfig` in `src/config.rs`:
   ```rust
   #[derive(Debug, Deserialize)]
   pub struct GradualConfig {
       pub tsconfig: String,
       pub eslint_config: Option<String>,
       #[serde(default = "default_events_dir")]
       pub events_dir: String,   // default: ".gradual/events"
   }
   ```
   Support `gradual.json` — check for it, error if it doesn't exists.
   Use `serde_json` for JSON;

5. Stub all modules so `cargo build` succeeds with no warnings.

**Done when:** `cargo build` and `cargo test` pass on empty stubs.

---

## Phase 2 — Finding identity (the most critical piece)

**Goal:** given any `RawFinding`, produce a stable composite ID.

### Tasks

1. **`src/identity/parser.rs`** — tree-sitter parser setup and parse cache:
   ```rust
   pub struct ParseCache {
       // keyed by (file path, content hash) → tree-sitter Tree
       // content hash avoids re-parsing unchanged files across multiple findings
       cache: HashMap<(PathBuf, u64), tree_sitter::Tree>,
   }

   impl ParseCache {
       pub fn parse(&mut self, path: &Path) -> anyhow::Result<&tree_sitter::Tree> {
           let source = fs::read_to_string(path)?;
           let content_hash = xxh3_64(source.as_bytes()); // fast hash for cache key
           // if cache hit, return cached tree
           // else: pick grammar by extension (.ts vs .tsx), parse, cache, return
       }
   }
   ```
   Grammar selection:
   - `.ts` / `.js` → `tree_sitter_typescript::language_typescript()`
   - No need for TSX support for now
   - Other extensions → error (should not happen given analyzer output)

2. **`src/identity/walker.rs`** — AST helpers:

   `find_meaningful_enclosing(node: Node) -> Node`
   - Walk up from the token at the finding position
   - Stop at these node kinds (start with this set, grow as you encounter edge cases):
     ```
     function_declaration, method_definition, arrow_function,
     variable_declarator, class_declaration, call_expression,
     binary_expression, return_statement, assignment_expression,
     type_alias_declaration, interface_declaration, export_statement
     ```
   - If an `ERROR` node is encountered anywhere in the walk, skip past it to its parent
   - If we reach the root without finding a meaningful node, return the root

   `collect_named_scope_path(node: Node, source: &[u8]) -> Vec<String>`
   - Walk up the tree collecting names from named declaration nodes
   - Node kinds with names: `function_declaration`, `method_definition`,
     `class_declaration`, `variable_declarator`, `function` (anonymous assigned to var)
   - For each, use `node.child_by_field_name("name")` and extract `.utf8_text(source)`
   - Returns e.g. `["AuthService", "validateToken"]`
   - Returns empty vec for top-level code outside any named scope

   `compute_occurrence_index(node: Node, source: &[u8]) -> usize`
   - Among the node's parent's children with the same `kind` AND same normalized text,
     what is this node's 0-based position?
   - Handles the "two identical `return null` in the same function" case

   `normalize_whitespace(text: &str) -> String`
   - Collapse all whitespace runs to a single space
   - Trim leading/trailing whitespace
   - Do NOT strip identifiers, punctuation, or type annotations

3. **`src/identity/hasher.rs`** — the composite ID:
   ```rust
   pub fn compute_finding_id(
       finding: &RawFinding,
       repo_root: &Path,
       cache: &mut ParseCache,
   ) -> anyhow::Result<String> {
       let source = fs::read(finding.file.as_path())?;
       let tree = cache.parse(&finding.file)?;
       let root = tree.root_node();

       // tree-sitter is 0-indexed; findings are 1-indexed
       let pos = tree_sitter::Point {
           row: (finding.line - 1) as usize,
           column: (finding.column - 1) as usize,
       };
       let token = root.descendant_for_point_range(pos, pos)
           .context("no node at finding position")?;

       let meaningful = find_meaningful_enclosing(token);
       let scope_path = collect_named_scope_path(meaningful, &source);
       let occurrence = compute_occurrence_index(meaningful, &source);

       // file path must be repo-relative and use forward slashes (cross-platform stability)
       let rel_path = finding.file
           .strip_prefix(repo_root)?
           .to_slash_lossy();  // use the `path-slash` crate for Windows safety

       let components = [
           finding.rule.as_str(),
           rel_path.as_ref(),
           &scope_path.join("."),
           meaningful.kind(),
           &normalize_whitespace(meaningful.utf8_text(&source)?),
           &occurrence.to_string(),
       ];
       let input = components.join("\0");

       // xxH3-128: fast, non-cryptographic, excellent distribution
       let hash = xxh3_128(input.as_bytes());
       Ok(format!("{:032x}", hash))
   }
   ```
   Add `path-slash = "0.2"` to Cargo.toml for the `.to_slash_lossy()` call —
   ensures IDs are identical on Windows and Unix even though path separators differ.

4. **`tests/identity/hasher_tests.rs`** — edit-stability test suite.

   This is the most important test file in the project. Structure each test as:
   - Load a fixture `.ts` file (or two variants of one)
   - Synthesize a `RawFinding` pointing at a known location
   - Assert the ID is/isn't stable across the edit

   | Edit | ID should... |
   |------|-------------|
   | Add an import at top of file | stay the same |
   | Reformat with prettier (whitespace only) | stay the same |
   | Move finding's function to a different position in the file | stay the same |
   | Add an unrelated function above the finding | stay the same |
   | Add a blank line inside the enclosing function | stay the same |
   | Rename a variable on the flagged line | change |
   | Edit the flagged expression itself | change |
   | Move the finding to a different file | change |
   | Rename the enclosing function | change |
   | Duplicate the flagged line (test occurrence index) | be distinct for each copy |
   | ERROR node in file (partial parse) | not panic, use fallback |

   Write fixture files in `tests/identity/fixtures/`:
   - `base.ts` — the original version of a file with a synthetic "finding"
   - `with_import.ts` — base + added import at top
   - `reformatted.ts` — base with whitespace-only changes
   - `function_moved.ts` — base with the enclosing function moved lower
   - `variable_renamed.ts` — base with the identifier on the flagged line renamed
   - `expression_edited.ts` — base with the flagged expression changed
   - `duplicated_line.ts` — base with the flagged line duplicated

   These fixture files are plain TypeScript that doesn't need to compile —
   they just need to be parseable by tree-sitter.

**Done when:** all hasher tests pass. This is the go/no-go gate for the entire project.

---

## Phase 3 — Analyzers

**Goal:** shell out to tsc and ESLint, parse their output into `Vec<RawFinding>`.

### Tasks

1. **`src/analyzers/typescript.rs`**:

   Shell out: `tsc --noEmit --pretty false -p <tsconfig> 2>&1`

   Parse tsc's non-pretty output line by line. Format:
   ```
   src/foo.ts(42,5): error TS2304: Cannot find name 'bar'.
   ```
   Regex (use the `regex` crate): `^(.+)\((\d+),(\d+)\): (?:error|warning) TS(\d+): (.+)$`

   Map to `RawFinding`:
   - `tool`: `Tool::TypeScript`
   - `rule`: `format!("ts:{}", code)` e.g. `"ts:2304"`
   - `file`: resolve against tsconfig's directory to get absolute path
   - `line`, `column`: parse as `u32`, they are already 1-indexed
   - `message`: the message text

   tsgo detection:
   ```rust
   fn tsc_binary() -> &'static str {
       // check once at startup: if `tsgo` is on PATH, use it, else `tsc`
       // cache the result in a OnceCell
   }
   ```

   Error handling: tsc exits nonzero when there are errors — that's normal.
   Distinguish "tsc ran but found errors" (exit 1, parse stdout) from
   "tsc failed to run" (e.g. tsconfig not found — exit 1 with no parseable output).
   If zero findings were parsed but exit code is nonzero, surface tsc's raw output
   as an `anyhow::Error`.

2. **`src/analyzers/eslint.rs`**:

   Shell out: `eslint --format json -c <config> <root>/**/*.{ts,tsx} --no-eslintrc`

   Parse JSON output. ESLint's JSON format:
   ```json
   [
     {
       "filePath": "/abs/path/to/file.ts",
       "messages": [
         {
           "ruleId": "no-unused-vars",
           "severity": 2,
           "message": "...",
           "line": 10,
           "column": 5
         }
       ]
     }
   ]
   ```

   Map to `RawFinding`:
   - `tool`: `Tool::Eslint`
   - `rule`: `format!("eslint:{}", rule_id)` — skip messages where `ruleId` is null
     (parse errors); log them as warnings but don't include in findings
   - `file`: `filePath` is already absolute
   - `line`, `column`: already 1-indexed

   ESLint version detection: check for `eslint.config.js` / `eslint.config.mjs` (v9 flat
   config) vs `.eslintrc.*` (v8 legacy). For v8, add `--no-eslintrc -c <config>`.
   For v9, add `--no-config-lookup --config <config>`. Make this transparent.

3. **Running both analyzers concurrently** (easy win, no async needed):
   ```rust
   use std::thread;

   let ts_handle = thread::spawn(|| run_typescript(&config));
   let eslint_handle = thread::spawn(|| run_eslint(&config));

   let ts_findings = ts_handle.join().unwrap()?;
   let eslint_findings = eslint_handle.join().unwrap()?;

   let all_findings = [ts_findings, eslint_findings].concat();
   ```
   Both analyzers are CPU-bound in subprocess space. Two threads, each blocking on
   a child process. Clean, no async runtime needed, saves the full runtime of
   whichever analyzer is faster.

4. **`tests/analyzers/`** — smoke tests with fixture files:
   - A tiny `fixture.ts` with one known TS error (e.g. `const x: number = "hello"`)
   - A tiny `.eslintrc.json` with one rule enabled
   - Assert the analyzer returns a `RawFinding` with the correct `(file, line, rule)`
   - Don't over-test; correctness is guaranteed by the hasher tests, not here

**Done when:** both analyzers return correct `Vec<RawFinding>` on fixture files.

---

## Phase 4 — Event log: fold and write

**Goal:** read the event log, fold into current state, write new delta events.

### Tasks

1. **`src/git.rs`**:
   ```rust
   pub fn get_current_sha(repo_root: &Path) -> anyhow::Result<String>
   pub fn get_parent_sha(repo_root: &Path) -> anyhow::Result<String>
   ```
   Shell out to `git rev-parse HEAD` and `git rev-parse HEAD^`.
   Return clear errors if not in a git repo or if HEAD has no parent (initial commit).
   Don't use `git2` (libgit2) for this — shelling out is simpler and avoids a
   heavy dependency for two trivial operations.

2. **`src/events/reader.rs`**:
   ```rust
   pub fn read_all_events(events_dir: &Path) -> anyhow::Result<Vec<DeltaEvent>>
   ```
   - Use `walkdir` to recursively find all `.json` files under `events_dir`
   - Parse each with `serde_json::from_str::<DeltaEvent>`
   - Skip files that fail to parse with a `eprintln!` warning — don't crash
   - Validate `version == 1` — skip files with unknown versions with a warning
   - Sort ascending by `(timestamp, commit)` — timestamp string comparison works
     because ISO 8601 is lexicographically sortable

3. **`src/events/fold.rs`**:
   ```rust
   pub fn fold(events: &[DeltaEvent]) -> HashMap<String, Finding>
   ```
   ```
   state = HashMap::new()
   for event in events (already sorted ascending):
       for id in event.removed:
           state.remove(&id);   // idempotent: remove is no-op if absent
       for finding in event.added:
           state.insert(finding.id.clone(), finding.clone()); // idempotent: last write wins
   return state
   ```
   This is ~15 lines. Keep it exactly this simple.

4. **`src/events/writer.rs`**:
   ```rust
   pub fn write_delta_event(
       events_dir: &Path,
       event: &DeltaEvent,
   ) -> anyhow::Result<PathBuf>
   ```
   - Parse timestamp to extract `YYYY/MM/DD` for the directory path
   - Build filename: `<HH-mm-ss>-<first 7 chars of commit sha>.json`
   - `fs::create_dir_all` for the date-sharded directory
   - Serialize with `serde_json::to_string_pretty` (2-space indent, human-readable git diffs)
   - Write atomically: write to `<filename>.tmp`, then `fs::rename` to final path
     (rename is atomic on POSIX; prevents a half-written file being read by a concurrent run)
   - Return the final file path

5. **`tests/events/fold_tests.rs`**:
   Test cases (use fixture JSON files in `tests/events/fixtures/`):
   - Empty event list → empty state
   - Single add event → finding present in state
   - Add then remove → finding absent
   - Two adds of same ID → finding present exactly once (idempotent)
   - Two removes of same ID → no panic, finding absent (idempotent)
   - Rebase scenario: add F in event 1, add F again in event 2 → F present once
   - Rebase scenario: add F in event 1, remove F in event 2 → F absent
   - Mixed: add F and G in event 1, remove F in event 2, add H in event 3 → {G, H}

**Done when:** fold tests pass including all idempotency and rebase scenarios.

---

## Phase 5 — Commands

**Goal:** wire everything into working `check` and `update` commands.

### Tasks

1. **`src/commands/check.rs`**:
   ```
   1. Load config from gradual.toml / gradual.json
   2. Get repo root (walk up from cwd to find .git/)
   3. Run analyzers concurrently → Vec<RawFinding>
   4. Compute finding IDs → Vec<Finding>  (uses ParseCache)
   5. Read + fold event log → HashMap<id, Finding>  (baseline)
   6. Diff:
        added   = current findings where id NOT IN baseline
        removed = baseline findings where id NOT IN current
   7. If added is non-empty:
        eprintln each added finding as  "file:line  rule  message"
        exit process with code 1
      Else:
        println "✓ No regressions ({} findings removed)" removed.len()
        exit 0
   ```
   `check` is read-only. It never touches disk beyond reading.
   Print removed findings as informational output (not a failure).

2. **`src/commands/update.rs`**:
   ```
   1-5. Same as check
   6. Diff → added[], removed[]
   7. If added is non-empty && !force:
        eprintln each added finding
        eprintln "Run with --force to accept regressions"
        exit 1
   8. If added is non-empty && force:
        eprintln "⚠ Accepting {} new findings:" added.len()
        eprintln each added finding
        if process is a TTY (std::io::stdin().is_terminal()):
            prompt "Are you sure? [y/N]: "
            read line; if not "y" or "Y": exit 1
        else if !yes_flag:
            eprintln "--yes required to accept regressions in non-TTY mode"
            exit 1
   9. Build DeltaEvent:
        commit  = get_current_sha()
        parent  = get_parent_sha()
        timestamp = Utc::now().to_rfc3339()
        added   = the added Vec<Finding>
        removed = removed IDs Vec<String>
   10. write_delta_event(events_dir, &event)?
   11. println "Written: {path}"
       println "Stage and commit this file to record the baseline update."
   ```

3. **`src/main.rs`** — clap CLI:
   ```rust
   #[derive(Parser)]
   #[command(name = "gradual", about = "Gradual TS/ESLint improvement tool")]
   enum Cli {
       Check,
       Update {
           #[arg(long)]
           force: bool,
           #[arg(long)]
           yes: bool,
       },
       Init,
       InstallHook,
   }
   ```
   Route to the appropriate command module. Use `anyhow::Result<()>` throughout
   and convert the top-level error to a user-friendly message:
   ```rust
   fn main() {
       if let Err(e) = run() {
           eprintln!("error: {:#}", e);  // {:#} gives anyhow's full error chain
           std::process::exit(1);
       }
   }
   ```

**Done when:** `gradual check` and `gradual update` work end-to-end on a real repo.

---

## Phase 6 — Git hook integration

**Goal:** one-command hook installation, zero manual steps for new developers.

### Tasks

1. **`src/commands/install_hook.rs`**:
   - Target: `.git/hooks/pre-commit`
   - Check if the hook already exists:
     - If it doesn't: write it, `chmod +x`, done
     - If it exists and already contains `gradual check`: print "already installed", done
     - If it exists with other content: print a warning and instructions to add manually,
       don't overwrite (never clobber another tool's hook)
   - Hook content:
     ```sh
     #!/bin/sh
     # Added by gradual. Do not edit this line.
     gradual check
     ```

2. **`src/commands/init.rs`**:
   - Check `events_dir` — if it already has `.json` files, print a warning and exit 1
     ("already initialized, run `gradual update` to record changes")
   - Run both analyzers → all current findings are the genesis `added` set
   - Write the genesis delta event (all findings as `added`, empty `removed`)
   - Create `.gradual/.gitignore` with contents:
     ```
     cache/
     ```
   - Print:
     ```
     Initialized. Found N findings across M files.
     Next steps:
       git add .gradual/
       git commit -m "chore: initialize gradual baseline"
       gradual install-hook
     ```

3. **Binary installation story** (document in README):
   ```
   # Option A: cargo install (from source)
   cargo install --path .

   # Option B: download pre-built binary (future GitHub releases)
   curl -sSfL https://github.com/you/gradual/releases/latest/download/install.sh | sh
   ```
   The binary has zero runtime dependencies. Developers don't need Rust installed
   to use it — just download the binary for their platform.

**Done when:** `gradual init && gradual install-hook` sets up a fresh repo correctly,
and the pre-commit hook fires and gates commits.

---

## Phase 7 — Hardening

**Goal:** safe to adopt in a large production repo.

### Tasks

1. **Error messages**: every `anyhow::bail!` includes what went wrong AND what to do.
   Bad: `"tsc failed"`. Good: `"tsc exited with code 1 but produced no parseable output.
   Check that your tsconfig path is correct: {path}"`.

2. **Missing tool detection**: before shelling out, check that `tsc` (or `tsgo`) and
   `eslint` are findable via PATH or `node_modules/.bin/`. Print install instructions
   if missing.

3. **Repo root detection**: walk up from cwd to find `.git/` directory. Error clearly
   if not in a git repo.

4. **Deleted files**: handled automatically — if a file is deleted, its findings no
   longer appear in analyzer output, so the diff puts their IDs in `removed`. No
   special case needed. Add an explicit test to confirm.

5. **`--timeout` flag** on `check` and `update`: kills the analyzer subprocesses if
   they exceed N seconds. Useful for enforcing a maximum acceptable CI/hook time.
   Implement via `std::process::Child::wait_timeout` (from the `wait-timeout` crate)
   or by spawning a watchdog thread.

6. **`gradual status`** command (nice to have for v1):
   - Fold the event log
   - Print finding counts grouped by rule, sorted descending
   - Print total removed since genesis (trend line)
   - Useful as a team dashboard: `gradual status` in CI to track progress

---

## Key decisions to make before starting

1. **Binary name**: `gradual`

2. **Config format**: `gradual.json` is more familiar to JS developers (your users). 

3. **tsgo vs tsc**: auto-detect tsgo on PATH, fall back to tsc. Log which one was used.

4. **ESLint version**: detect by presence of `eslint.config.js` (v9) vs `.eslintrc.*`
   (v8). Adjust CLI flags accordingly. Fail clearly if neither is found.

---

## What to build first (suggested order for Claude Code)

Start Phase 2 — the hasher is the riskiest piece and gates everything else.

```
Phase 2 → Phase 4 → Phase 3 → Phase 5 → Phase 6 → Phase 7
```

Rationale: prove the identity logic before building the pipeline around it. If the
hasher has a design flaw, you want to discover it before writing 600 lines of command
code that depends on it.

### Good first prompt for Claude Code

> "Implement `src/identity/` as described in the plan. Start by creating the fixture
> `.ts` files in `tests/identity/fixtures/` and writing the test cases in
> `tests/identity/hasher_tests.rs`, then implement the hasher to make them pass.
> Use the `tree-sitter` and `tree-sitter-typescript` crates (native Rust, no WASM).
> Use `xxhash-rust` with the `xxh3` feature for 128-bit hashing.
> Use `anyhow` for error handling throughout."

### Second prompt (after hasher tests pass)

> "Implement `src/events/` — the `DeltaEvent` types, `read_all_events`, `fold`, and
> `write_delta_event` — as described in the plan. Write `tests/events/fold_tests.rs`
> first, then implement to make them pass. Pay particular attention to the idempotency
> test cases."

---

## What this is explicitly NOT (v1 scope boundary)

- No daemon mode / watch mode
- No file-scoped incremental analysis (always full codebase for v1)
- No SQLite cache for fold results
- No compaction tooling (manual only, CLI command when needed)
- No editor integration
- No plugin system (TypeScript + ESLint hardcoded — this is a feature, not a limitation)
- No monorepo multi-tsconfig support (single tsconfig target)
- No remote state / team dashboard
- No Windows path testing (add in v2 — `path-slash` crate handles the separator
  normalisation but end-to-end Windows testing is out of scope for v1)