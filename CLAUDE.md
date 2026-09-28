# Gradual: architecture, decisions and trade-offs

Read this before changing anything. It records *why* the code is the way it is, including the
alternatives that were tried or rejected, so you don't have to guess or re-litigate them.

## What this is

A Rust CLI (`gradual`, distributed on npm as `@julienguillot/gradual`) that lets a team turn on strict
TypeScript / ESLint rules without fixing everything first. It records existing findings in a baseline
and fails only on **new** ones ("ratchet"). Commands: `init`, `check`, `update [--force --yes]`, `status`.

It is a replacement for Betterer with one defining difference: **no merge conflicts on the baseline, ever.**
Betterer keeps one mutable results file, which conflicts constantly on active teams. Gradual keeps an
append-only directory of small, immutable event files, so two branches never write the same file.

## Philosophy (use these to settle ambiguous decisions)

1. **No merge-sensitive shared state.** Anything that two branches could both edit or that must agree
   across branches is out. Events are immutable, uniquely named files; the baseline is *derived* by folding
   them. Any design that needs a coordinated counter, a shared lockfile, or a mutable snapshot is wrong.
2. **Handle infrastructure edge cases silently, but never weaken the ratchet silently.** The tool should
   not bother people with rebase/merge/format noise (see #6 for the friction that *is* intended). But when
   a case can't be resolved without risking a missed regression, fail *loudly* and explain, rather than
   pass quietly. A false failure is annoying; a silently accepted regression breaks the tool's promise
   ("old errors can never be reintroduced once fixed").
3. **Narrow on purpose.** Only `tsc` (or `tsgo`) and ESLint, hardcoded. No plugin system, no daemon.
   A third analyzer, if ever needed, is a first-class built-in.
4. **Correctness over speed** for the gate (e.g. no `tsc --incremental`, see `analyzers/typescript.rs`).
5. **Content decides identity, not position.** Findings must survive line shifts, reformatting, and
   unrelated edits, and must change when the flagged code itself is edited ("you touched it").
6. **Intentionally a bit painful to ignore findings.** The point is not a permanent baseline, it is that
   developers fix existing findings *when they touch that code*. So accepting or hiding a finding must cost
   something, while fixing one must be easy and rewarded:
   - Editing a flagged line makes it a "new" finding on purpose: you touched it, you fix it. Do not add
     fuzzy matching that lets a finding follow an edited line (this is also why the id is content-based).
   - Accepting regressions is deliberately awkward: `update --force`, a `y` confirmation on a TTY,
     `--yes` off a TTY, and a non-empty `changes` list with positive deltas that reviewers see in the diff.
   - Do **not** add ways to make findings go away cheaply: no ignore/suppress/allowlist commands, no
     "accept all" or auto-update mode, no baseline auto-repair that silently absorbs new findings, no
     config to lower the bar per file, no quieter defaults. If a feature makes ignoring easier than fixing,
     it is wrong for this tool.
   - The friction targets *ignoring debt*, never *tool noise*: false failures from merges, rebases,
     formatting or line shifts are bugs (#2). Loud messages should explain the fix, not offer a bypass beyond
     the existing explicit `--force` path.

## Pipeline

```
analyzers::run_all ──> RawFinding {rule, file(abs), line, column, message}
      │
identity::build_findings ──> Finding {id, rule, file(rel), line, message}   (sorted, deterministic)
      │                          id = content hash; identical findings share an id
events::diff::group_by_id ──> HashMap<id, Vec<Finding>>       (current state)
events::reader + fold ──────> HashMap<id, Entry{count,rule,file}>   (baseline = sum of all events)
      │
events::diff::diff(current, baseline) ──> Vec<KeyDelta>   (per id: current count − baseline count)
      │
check: fail if any delta > 0 · update: write one event containing the non-zero deltas
```

`commands::analyze` runs this and returns `AnalysisResult`. It reads the baseline **before** running
analyzers so an unsupported baseline format fails fast.

## Module map

| Path | Role |
|---|---|
| `src/main.rs` | clap CLI only. Re-declares the modules with `mod` (see "lib vs bin"). |
| `src/lib.rs` | Exposes `analyzers, config, events, git, identity` so integration tests can reach them. `commands` is **not** in the lib. |
| `src/commands/` | `check`, `update`, `init`, `status`; `mod.rs` has `analyze` and `describe` (git-aware output). Keep them thin; logic lives in the lib modules so it is testable. |
| `src/analyzers/` | `typescript.rs` (parses `tsc --pretty false` output), `eslint.rs` (JSON output, v8/v9 detection), `mod.rs` (`run_all` runs them in parallel threads, `run_command` with timeout, `find_binary`: `node_modules/.bin` then `PATH`). |
| `src/identity/hasher.rs` | The id algorithm: `compute_block_id`, `line_block`, `normalize_message`. |
| `src/identity/mod.rs` | `build_findings`: path filter, hashing, deterministic sort. |
| `src/events/types.rs` | `Finding` (current, not persisted), `Change`/`DeltaEvent` (persisted, `EVENT_VERSION = 2`), `Entry` (baseline). |
| `src/events/fold.rs` | Sums deltas per id. |
| `src/events/diff.rs` | `diff`, `KeyDelta`, output formatting (`describe_new_with`), `locate_new`, `repeatedly_fixed_ids`, `concurrent_fix_hint`. |
| `src/events/reader.rs`, `writer.rs` | Read all event files; write one sharded file. |
| `src/git.rs` | repo root, SHAs, and `git diff`-based "added lines" helpers. |
| `src/config.rs` | `gradual.json` (`tsconfig`, `eslint_config?`, `events_dir`, `include`, `exclude`); `node_modules` is always excluded. |

## Finding identity (`src/identity/hasher.rs`)

```
id = xxh3_128( rule \0 repo-relative-path(forward slashes) \0 normalized_message \0 line_block )
     formatted as 32 lowercase hex chars
```

- `line_block` = the finding's **own line only**, whitespace runs collapsed to one space. `str::lines`
  also strips `\r`. A line past EOF hashes as `""`; line `0` clamps to the first line.
- `normalize_message` strips the absolute repo-root prefix, collapses `…/node_modules/` to
  `node_modules/`, and collapses whitespace, so ids are identical across checkouts.
- **Column is not hashed.** Nor are neighbouring lines.
- Consequences (all pinned by tests): stable under inserting/deleting lines elsewhere, adding statements
  right after the finding, editing the line just before it, re-indenting, tabs↔spaces, trailing
  whitespace, CRLF↔LF, reordering declarations, editing other files, moving the checkout. Changes when the
  finding line is edited (including `a + b` → `a+b`, and formatter line-wrapping), the file is renamed or
  moved, or the message or rule changes.

### Twins: identical findings are counted, never numbered

Findings with the same id (same rule, file, message, line text: e.g. two `// @ts-ignore` lines or two
`logger.debug(...)` calls) are a *twin group*. The baseline stores **how many** are accepted; `check`
fails when the current count exceeds it. There is no `:n` suffix and no attempt to say which twin is which
in the data.

## Baseline and events (format version 2)

Events live in `.gradual/events/YYYY/MM/DD/HH-MM-SS-<sha7>.json`, written once, never edited:

```json
{ "version": 2, "commit": "...", "parent": "...", "timestamp": "...",
  "changes": [ { "id": "<32 hex>", "rule": "ts:2304", "file": "src/app.ts", "delta": -1 } ] }
```

- A change is a **signed count delta** for one id. `fold` sums them per id, so the result does **not depend on
  event order**, and parallel branches add up correctly. Totals `<= 0` are dropped (a negative total, e.g. two
  branches fixing the same single finding, is clamped to "absent").
- `line` and `message` are deliberately **not** stored in the baseline (they go stale after the first edit;
  `check`/`update` print them from the current findings).
- `update` records `current − baseline` per id. New findings (positive deltas) need `--force`, plus a `y`
  confirmation on a TTY or `--yes` off-TTY. Negative deltas are always allowed. `init` refuses if any event
  file exists.
- **Versioning:** the reader treats any `version != 2` as a **hard error** with migration instructions
  (delete the events dir on a clean branch, re-run `init`). Silently skipping unknown versions would produce a
  wrong baseline. Malformed/unreadable files are still warned about and skipped. Version 1 (positional
  `base:n` ids with `added`/`removed`) is unsupported.

## Reporting which finding is new

Content cannot say which of several identical findings is the new one (inserting a twin before or after an
existing one gives byte-identical files). `check`/`update` therefore print, per group with `0 < excess < size`:

1. If `git diff` (working tree vs `HEAD`, then vs the branch point from the default branch) shows exactly
   `excess` of the group's lines as added → point at those lines ("in your diff; identical findings also
   at lines …"). Tiers are tried tightest first; failures/empty tiers just fall through.
2. Otherwise list the whole group: `(N new among M identical findings, lines …)`.

This is display only. It never affects counts or the exit code, stores nothing, and never fails.
Known blind spot: a twin that starts failing because of a change elsewhere (a type change) isn't in the diff.
A re-indented existing twin can be a decoy, but only if the counts also match exactly.

## Merge and parallel-branch semantics

Count deltas are commutative, so parallel branches compose:

| Baseline 2 twins | Branch X | Branch Y | Merged |
|---|---|---|---|
| fix different twins | −1 | −1 | 0 ✔ (reintroducing one is caught) |
| fix one / accept one | −1 | +1 | 2 ✔ |
| accept a twin each | +1 | +1 | 4 ✔ |

**Known limitation (deliberate):** if both branches fix the *same* twin, the deltas sum to −2 and the baseline
drops below the code, so `check` fails on the merge commit. "Same twin fixed twice" and "two different twins
fixed" produce *identical* events, so no count-based scheme can resolve both silently. We chose the loud,
safe direction. `check` prints a note (`concurrent_fix_hint`, based on `repeatedly_fixed_ids`) pointing at
`gradual update --force`, which repairs the baseline. The note is heuristic: it can also appear for a genuine
regression of a twin group that was fixed more than once, hence the hedged wording.
(Single non-twin findings fixed by both branches are fine: the clamp handles it.)

## Decision log (what we tried and why it's gone)

| Decision | Why |
|---|---|
| **Delta events, not snapshots** | Deltas are invariant under rebase; snapshots go silently wrong. |
| **Rust, single binary, npm as a thin launcher** (`npm/app/index.ts` resolves a per-platform optional dependency) | No Node runtime needed; correctness-critical fold logic benefits from the type system. |
| **Dropped tree-sitter/AST identity** | Heavy, and unnecessary: a content hash of the finding's own line is stable enough and analyzer-agnostic. |
| **Dropped the "forward context block"** (hash of the next N non-whitespace chars) | Editing code just *after* a finding changed its id (e.g. adding a property below). Line-only was chosen so downstream edits never matter. |
| **Dropped positional `:n` counters on twins** | They are positions, not identities: parallel branches reuse/remove the same `:n` for different lines, corrupting the merged baseline (a silent hole where a reintroduced finding passes). Reproduced by tests `f01`–`f03`; fixed by counting. |
| **Rejected context signatures / line-map diff / baseline-relative matching** to name the new twin | Adds persistent state that drifts and is merge-sensitive; needs thresholds that give false positives on refactors or miss swaps; and the adjacent-insertion case is unresolvable from content anyway (verified with `diff`/`difflib`). |
| **Rejected "tolerate concurrent overlap"** for the same-twin case | Silent, but lets a regression through once in the different-twins case. Violates philosophy #2. |
| **Rejected git-history-based baseline** | Needs per-finding baseline commits, a clean tree and git plumbing; git is used only for *display* refinement. |
| **Breaking format change is fine** | Ship a new major/minor and tell users to rebuild the baseline from a clean branch. That's why the reader hard-errors on old versions. |
| **No `--incremental` for `tsc`** | It can report a different diagnostic set than a full check, breaking "no-change check reproduces the baseline". Use `tsgo` for speed. |

## Testing

`cargo test` and `cargo clippy --all-targets` must both be clean (clippy `pedantic` is on; a few lints are
allowed in `Cargo.toml`). `unsafe` is forbidden.

**Every integration test file must be registered in `Cargo.toml` as a `[[test]]`** (they live in
subdirectories, so they are not auto-discovered).

| File | Purpose |
|---|---|
| `tests/identity/edit_scenarios_tests.rs` | **The behavioral contract.** Scans a repo "before" and "after" a realistic edit and asserts on added/removed counts through the production `build_findings` + `diff`. Sections: A stability (must not change), B sensitivity (must change), C twins, D edge cases, E **golden pins**, F concurrent branches. |
| `tests/identity/hasher_tests.rs` | Low-level hash properties on fixtures. |
| `tests/events/fold_tests.rs` | fold (sum, order independence, clamp), diff/describe, reader (version errors), writer. |
| `tests/git/added_lines_tests.rs` | Hunk parser, `locate_new`, output formats, and real temp git repos. |
| `tests/analyzers/*` | Parsing of tsc / ESLint output. |

**Golden pins (section E) are the on-disk contract.** Ids are stored in committed baselines. If a golden test
fails, the identity algorithm changed and every existing baseline would report all findings as new. Only
update those literals as a deliberate, released, baseline-breaking change (with a migration note).

When changing identity: add/adjust a scenario in `edit_scenarios_tests.rs` first; decide explicitly whether
the behavior change is intended; treat it as breaking if any golden value moves.

### Manual UX playground

`examples/demo/` is a tiny TS project plus `play.sh` that builds a throwaway git repo and plays scenarios
(`insert-twin`, `insert-twin-committed`, `add-import`, `edit-finding`, `fix-one`, `parallel-fixes`,
`same-twin`, or `all`) against the debug binary. Setup: `cargo build`, `(cd examples/demo/project && npm install)`.
Use it to judge output wording after changing anything user-facing.

## Conventions and gotchas

- **lib vs bin:** `main.rs` declares `mod analyzers; mod commands; …` itself, so modules are compiled twice
  (once as the `gradual` lib for tests, once into the binary). Items only used by tests need
  `#[allow(dead_code)]` (see `describe_new`). Pure logic goes in the lib modules; `commands/` stays thin and
  untested directly.
- `clippy::implicit_hasher` is silenced with `#[allow]` on functions taking `HashMap`/`HashSet`; don't
  generalize over hashers.
- Analyzer output order is nondeterministic. `build_findings` sorts by (file, line, column, rule, message);
  `diff` sorts by (file, first line, id). Keep all output deterministic.
- Event files are named by second + commit sha7. Two updates in the same second from the same commit collide
  (the demo script sleeps between them). Not a real-world concern, but don't rely on unique names in tests.
- `.gradual/cache/` is git-ignored by `init`; events are committed.
- Do not add `:n`-style suffixes, per-finding stored line/context, or any cross-branch shared state
  (see philosophy #1), and do not add easy escape hatches for findings (see #6).

## Release

`node release.js <version>` bumps `Cargo.toml`, `Cargo.lock` and `npm/app/package.json` (including the
platform `optionalDependencies`), commits `chore: release vX.Y.Z` and tags `X.Y.Z`. Pushing the tag triggers
`.github/workflows/release.yaml` (builds per-platform binaries, publishes the platform packages built from
`npm/package.json.tmpl`, the base package, then a GitHub release). The v2 baseline format is a breaking change
for 0.1.x users: pick the version bump accordingly and keep the README "Migrating" section.

## Possible follow-ups (not built)

- A `gradual` post-merge/CI auto-repair for the same-twin case.
- Daemon mode / warm TypeScript programs for faster hooks (explicitly out of scope so far).
