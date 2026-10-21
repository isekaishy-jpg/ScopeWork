# Proposal: a rename reaches every report without reopening work

Status: implemented 2026-10-21 (maintenance branch). Decision taken by the user: rewrite on rename.

## Problem

Reports cite other records as `` `Name` (`VA`) ``. When research later renames a function (for example `FUN_00679d90` becomes `DBCache__CreatureStats_C__DenyItem`), every report that cites it keeps the old name. Fixing the text by hand changes each report's hash, so every completed citing record and its ancestors reopen: one sweep on 2026-10-21 touched 98 completed chunks and took hours of re-completion. The user's requirement: a name change must reach its name everywhere, including every synthesis, without reopening completed work. Complaint: `2026-10-21-agent-rename-forces-recompletion-of-citing-chunks.md`.

## Research

- A rename is a `node batch` update with `name`. `apply_batch` re-renders the renamed record's report (its `# Name` header) and its parent's report (the source list shows child names). Both count as changed, so `reopen::mark_changed` reopens them and `reopen_affected` reopens their completed ancestors. A rename therefore already reopens the renamed record, its chunk and its architecture, before any citation is touched.
- Citing reports elsewhere are not touched at all, so they keep the stale name.
- `report_hashes::mentioning` keeps a persisted index of every 8-digit hexadecimal token in every report (reused while a report's size and modification time are unchanged). A function's VA is such a token, so the reports that could cite a renamed function are found without reading every report.
- The image base is part of the binary identity (`image base 0x00400000`), so VA = RVA + base.
- Precedents for mechanical rewrites that keep completion: layout migration and identity amendments move accepted hashes and prerequisite stamps from the old text to the new and re-pin annotation-ledger rows.

## Design

1. **What is rewritten.** For each function renamed in a batch whose VA is known, every exact citation `` `OldName` (`VA`) `` (VA as 8 hex digits, any case, optionally `0x`-prefixed) in any report's body or evidence becomes `` `NewName` (`VA`) ``. The renamed record's header and its parent's source list are re-rendered as today.
2. **Mechanical edits keep completion.** A report whose only changes come from renames (its own header, a child's name in its source list, rewritten citations) is a mechanical edit. Its record is not reopened: an accepted hash that matched the old text moves to the new text, prerequisite stamps that referenced the old text move too, and the review queue is not touched. Reports changed for structural reasons in the same batch (moves, creation, context or order changes, dependency resynchronisation, legacy link repair) reopen exactly as before, even if they also contain rewritten citations.
3. **Ledger pins follow.** The transaction re-pins annotation-ledger rows of the mechanically edited reports in place (only those reports), like an identity amendment.
4. **Visible result.** The batch result lists `citations` (each report rewritten with its count) and `unrewritten` (reports among the candidates that still mention an old name outside the exact citation form, with counts), so the remaining manual follow-up is explicit.

## Acceptance

Renaming a function in a completed project rewrites its citations in other completed chunks and their architecture syntheses, keeps every one of those records and the renamed record complete, re-pins their ledger rows, and lists the rewritten and unrewritten reports. A batch that moves a record still reopens what it reopened before.

## Tests

A small project with two completed chunks: chunk A holds function F; chunk B's function and chunk B's synthesis cite F. Renaming F keeps everything complete, rewrites both citations and the header and source list, re-pins a ledger row, and reports a bare mention as unrewritten. A batch that renames and moves at once reopens the moved record's chunks as before.

## Remaining and future work

Bare mentions of an old name without its VA are reported, not rewritten, because they cannot be identified safely. Citations of chunks or architectures (which have no VA) are not covered.

## Implementation notes

- `apply_batch` builds one `Rename` per renamed record whose name changed, with its citation and whole-word patterns compiled once per batch. Candidate reports come from the reference index (`report_hashes::mentioning` over the renamed VAs); only those are read in addition to the ones the batch already re-renders.
- Classification: everything the batch changed is mechanical unless it is structural. Structural is: created records and their parents, moved records with their old and new parents, records whose `parent` or `context_only` was updated and their parent, reordered parents, records whose dependency views resynchronise, reports that mention moved paths, and everything when `repair_legacy_links` is set or no rename is in the batch (unchanged behaviour). Structural records reopen and cascade as before; mechanical ones do not.
- Mechanical records: a renamed record's revision advances; an accepted hash equal to the old report hash moves to the new hash, and every prerequisite stamp `rev:hash` naming the old revision and hash is rewritten. The review queue is untouched.
- The journal carries `repin_ids` (the mechanical records) with `repin_ledger`, so the apply step and recovery re-pin only those reports' ledger rows; history keeps the field.
- The reference index now records `0x`-prefixed VAs too, under a new file name (`report-references-v2.json`); the first batch after the upgrade rebuilds it once.
- Tests: `tests/rename_citations.rs` (rename keeps completion and re-pins; rename with a move still reopens; rename with a creation still reviews the new member's parent, checked on the stored status because effective status already waits on the new member).
