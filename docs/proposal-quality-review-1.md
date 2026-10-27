# Proposal: app-wide code-quality review, round 1

Status: implemented 2026-10-27. The user asked for app-wide reviews in two parts, code quality and then bugs, each repeated until a round finds nothing. Fixes are the obvious ones only: no feature or behaviour changes.

## Method

- Read the core modules (`research.rs`, `cli.rs`, `catalog_batch.rs`) in full.
- Search all of `src` for repeated patterns: the current time, hashing, RVA parsing, report markers, JSON file reads.
- Search for stale references to removed features, and for public functions nothing calls.
- Long functions (for example the dashboard's 650-line `frame`) are noted but not split: splitting them is a refactor, not an obvious fix.

## Findings and fixes

### Stale or misplaced documentation
1. Doc comments still describe quarantine, which pools replaced: `add_directed`, `reopen`, `set_dependencies_directed`, the batch, journal and removal `directed_by` fields, `commit_metadata_with`, a removal comment, and the module doc of `reopen.rs`. The sets of pooled records the caller may not change are still named `sealed`. They are reworded, and the variables become `closed`.
2. "The lock name of a project's tool resource." now documents `SHARED` instead of `resource_lock_id`.
3. "Reports contain their editable source in Markdown…" sits above the caller thread-local it has nothing to do with; it moves to the report helpers.
4. In `cli.rs`, "The project IDs in `root`, sorted…" is an orphan above `agents`; it goes.

### Repeated code
5. **The current time in Unix seconds** is computed inline about fifteen times; `coordination.rs` has a private `now_unix`. One `research::unix_now()` replaces them.
6. **SHA-256 as hex** has four helpers (`research::hash`, `drift::hash`, `imports::digest`, `journal_history::sha256`). One `sha256_hex(bytes)` serves them, with `hash(text)` kept as its text form.
7. **Single RVAs** are parsed in five places. Three use `trim_start_matches`, which also accepts `0x0x12`. One `parse_rva` serves them, and `parse_rvas` uses it.
8. **A report's body and evidence** are split from the report text in three places (`catalog_batch::parts`, `edit_tokens::parts`, `Library::read`). `edit_tokens::parts` serves all three.
9. **The one-report `ReportBatch`** is built by hand three times (draft save, evidence save, reopen). A helper builds it.
10. **A `Library` with no projects** is written out field by field five times. One constructor makes it.
11. **Lock holder and ticket records** (pid, command, time) are built twice. One helper builds them.
12. In `cli.rs`:
    - JSON request files are read with the same two-step parse about ten times; a `read_json` helper does it.
    - The `claimed_by` answer is built three times; a helper builds it.
    - `run` rebuilds `home.join("projects")` although `root` holds it.

### Simplifications
13. `save_report` returns early for drafts, yet the rest still tests `complete` at every step. Those branches are dead and go.
14. `views` aliases `self` as `lib` for no reason.
15. `catalog_batch` wraps `==` in `same` and `layout::inside` in `contained`; the calls use the originals.
16. `set_dependencies_by_rva` is never called; it goes.
17. `commit_metadata_with` takes eight arguments under `#[allow(clippy::too_many_arguments)]`, a suppression the repository rules forbid. Its optional parts move into a small struct, and the allow goes.

## Acceptance

- No behaviour changes: the full suite passes unchanged, apart from tests of a removed function, of which there are none.
- Clippy (`-D warnings`) and fmt are clean.
- The next round re-reviews the result.
