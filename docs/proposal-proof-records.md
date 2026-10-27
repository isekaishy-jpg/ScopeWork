# Proposal: proof records and a per-project proof requirement

Status: implemented 2026-10-27. Source: complaint `2026-10-25-arch2-no-gate-on-rust-proof.md` in the research home, and the user's decision of 2026-10-27.

## Research

The 3-3-5a project's AGENTS.md says "Every item is proven by recreating it in Rust and checking the work" in `wow-335a-rust`. Scopework records nothing about that proof. An agent completing a backlog in dependency order found nine records in the closure of `SMemAlloc`/`SMemFree`/`SMemReAlloc` whose reports name no Rust recreation, so a completed record could mean either "reproduced" or "only reviewed". To tell them apart, the agent wrote its own matcher, widened twice for three different spellings of the proof in report text.

Checking `wow-335a-rust` (2026-10-27): every one of the six named records has a native-state proof test committed on Oct 23–24, before the complaint. For example, `tests/storm_computer_header_frame.rs` proves `LogComputerInfoHeader`, and `tests/storm_stacktrace.rs` proves `sDbgHelpGetStackFrameInfo`. The proofs exist; the records just don't cite them anywhere a tool can query.

The implementation tracker (`research impl`) records where a function is implemented, but not how it is checked, and 3-3-5a does not use it. Report text is no place for the link: spellings drift, and it is not queryable.

Storage follows review flags and quarantine. Project-level state that older Scopework versions must not drop lives in a journaled sidecar beside the catalog, never as a new catalog field, because an older writer would drop an unknown field on its next save.

## Design

The user's decision: the durable check is a per-project requirement. When a project requires proof, its function records need a proof to be completed.

**Storage.** `proofs.json` beside the catalog (schema 1), a journaled sidecar like `review-flags.json`:
- `required`: whether completing a function in this project needs a proof;
- `repository`: optional path of the recreation's repository;
- `proofs`: by record ID, each with `recreation` (a path, optionally `::item`), `tests` (one or more test paths), `by` (`SCOPEWORK_SUBMITTER`, else the Windows user) and `at_revision`.

**Setting the requirement.** `research project set-proof PROJECT --required true|false [--repository PATH]`. The repository is an absolute path, because the coordinator runs in another folder. `project show` reports the policy.

**Recording a proof.**
- `research node proof PROJECT NODE --recreation PATH[::ITEM] --test PATH [--test PATH...]` records a proof for one function; `--clear` removes it.
- `research node proof-batch PROJECT --file F` takes `{"proofs":[{"id":N,"recreation":"...","tests":["..."]}]}`, so a project can backfill many records in one transaction.
- Only function records take a proof.
- When the project names a repository, each recreation and test path (the part before `::`) must exist under it, which catches typos and moved files at the time of writing. Whether the item named after `::` is still defined stays the project's own check.
- Proof writes are record operations: they never change status or report text, and they merge in the coordinator like review flags.

**The gate.**
- Completing a function in a project that requires proof fails without a proof, with `proof_required` and the command to record one.
- The record panel's completion blockers in the GUI list the missing proof.
- Publishing (saving a draft) stays free, so research can go up before the Rust exists.
- Chunks, architectures and the app synthesize; they take no proof.

**Records completed before the requirement.** Completed functions without a proof stay complete: nothing reopens and no completion cascades are lost. They show as **unproven**:
- `node show` gives `proof` and `unproven`;
- `research node unproven PROJECT [--complete]` lists functions without a proof;
- `status` counts `unproven_complete`;
- the record panel in the GUI shows the proof, or **Unproven** when the project requires one.

Clearing a completed record's proof makes it unproven, not reopened.

**Removal.** Deleting records drops their proofs from the sidecar in the same transaction, as it does for flags.

## Acceptance

- With the requirement off, completion behaves as today, and proofs can still be recorded and listed.
- With it on, completing a function without a proof fails with `proof_required`; recording a proof lets it complete; chunks complete without one.
- A proof whose file does not exist under the named repository is refused. Without a repository, paths are stored as given.
- Completed functions without a proof remain complete and appear in `node unproven --complete` and `status`.
- `proof-batch` records several proofs in one transaction and refuses the whole batch on any invalid entry.
- Removing a record drops its proof.
- The full test suite, Clippy and fmt are clean.
