# Proposal: history in the GUI, RVA and date filters, first-class tooling kinds

Status: implemented 2026-10-21 (maintenance branch).

## Problem

`research history show` merges catalog receipts and tooling entries, but only from the command line, filtered by node, revision range and kind. The researcher asks questions by function address and by day ("what changed 007FF480 last week?"). Catalog receipts carry no time at all. Tools append reviewed types, typed signatures, behaviour-artifact reviews and dependency-list changes as free-form kinds that the app does not know or check.

## Research

- `history.rs` reads `revision-N/journal.json` receipts newest first, parsing only summary fields, and the append-only `tools/workspaces/history/tooling.jsonl` (`at_unix` per entry). It already checks `journal.json` exists for every receipt, so a metadata read per receipt costs nothing extra.
- A receipt's `journal.json` is rewritten by `finish()` as the commit completes, so its modification time is the commit time. Only `research journal --compact-sidecars` rewrites receipts later, which would move that time.
- The live project's tooling log uses `coordinator-commits`, `coordinator-reads`, `coordinator-leases` (written by the app daily), `ledger-change` and `tooling-upgrade`; documentation also names `ghidra-apply` and `generated-artifact`.
- The GUI has one Research records window (classic style), opened from the Research menu, plus the write-queue window. Background work runs on a thread and reports through a channel, which `poll` drains.

## Design

1. **Times on catalog entries.** `finish()` writes `at_unix` into each new receipt. Older receipts use their `journal.json` modification time. `--compact-sidecars` stamps that time into a receipt it rewrites and restores the file's modification time, so dates survive compaction. Every catalog entry in `history show` gains `at_unix`.
2. **Filters.** `history show --rva HEX` selects the function with that RVA (an unknown RVA is an error, never an empty answer). `--from YYYY-MM-DD` and `--to YYYY-MM-DD` select entries by UTC day, both inclusive. They combine with the existing filters.
3. **First-class kinds.** The app knows these tooling kinds, with a label, a description and what an entry must name:

   | Kind | Must name |
   |---|---|
   | `reviewed-type` | a `--ref` (type name) |
   | `typed-signature` | a `--node` |
   | `behaviour-artifact-review` | a `--node` and a `--ref` (artifact path) |
   | `dependency-list` | a `--ref` (the list file) |
   | `ledger-change`, `ghidra-apply`, `generated-artifact`, `tooling-upgrade`, `coordinator-commits`, `coordinator-reads`, `coordinator-leases` | nothing |

   `history append` refuses a known kind without what it must name. Other kinds stay allowed. `research history kinds` lists the known kinds.
4. **GUI view.** View > Project history… opens a window that follows the dashboard's project. A filter row offers Kind (all, catalog, each known kind and any other kind present), Node, RVA, From and To, plus Refresh. The list shows the newest 200 entries, one line each: revision, UTC time, kind and a summary. The selected entry's details appear below, and Open record opens the record it concerns in Research records. Loading runs in the background, and errors show in the window.

## Acceptance

From the CLI, `--rva` and the date filters select the right catalog and tooling entries, and catalog entries carry `at_unix`. A known kind without its required node or ref is refused. Compaction keeps a receipt's time. The GUI window lists, filters and opens records without blocking the dashboard.

## Tests

CLI tests with a fixture project: RVA filter, an unknown RVA, date filters with a tooling entry and receipts (receipt times set through their modification time), required fields of known kinds, `history kinds`, and a compacted receipt keeping its time. A unit test covers the one-line summaries the GUI shows.

## Implementation notes

- `history.rs`: `HistoryFilter` gains `rva`, `from` and `to`; `KINDS` (with `drift`, which the coordinator also writes) and `kinds_value`; `day_start` parses UTC days; `entry_line` is the one-line summary the window shows (sidecar-only transactions such as review flags name their sidecar files).
- `finish()` writes `at_unix` into each new receipt. `journal --compact` stamps a rewritten receipt with its old modification time and restores that time on the file.
- `history_ui.rs`: the window state (`HistoryView`) lives on `ResearchUi`; reads use `Library::read_project` on a thread and never wait for writers. Opened from View > Project history….
- `examples/capture_panels.rs` adds `history-light` and `history-dark` shots.
- Tests: `tests/project_history.rs` (RVA and day filters, an old receipt dated by its modification time, compaction keeping the time, known kinds) and unit tests for `entry_line` and `day_start`.
