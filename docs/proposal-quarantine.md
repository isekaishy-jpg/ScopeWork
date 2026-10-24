# Proposal: quarantined records

Status: implemented 2026-10-21 on the maintenance branch; see Implementation notes at the end.

## Problem

Some research must stop and stay stopped. In `wow-335a-12340`, chunk 152 (ComSat voice session coordination) holds material that agents must not research further. The user wants it treated as finished and sealed. Today the only tools are review flags (which block completion, the opposite of what is wanted), `context_only` (which drops a node from the required inventory but leaves it fully usable) or simply leaving the chunk In Progress. The last is what happened: chunk 152 is still In Progress with an old audit flag, so architecture 14 can never complete. Agents also keep finding 152 in status listings as open work, and moves can still put unrelated functions into it.

The user's definition: a quarantine flag on a chunk or function "effectively marks it complete and quarantined and unusable".

## Proposed behaviour

A **quarantine** is a journaled marker on a chunk or function, with a reason, the revision it was set at and who set it. A quarantined chunk quarantines all of its functions.

1. **Counts as complete.** The effective status of a quarantined record is Complete for every purpose: parent completion gating, meters, `status` counts, `ready_for_review` (excluded) and dependency satisfaction. Report hashes are not compared, so nothing about its text can reopen it. Its parent can complete over it.
2. **Unusable (sealed).** The catalog refuses changes that would put work into it or take work out of it:
   - moves into or out of a quarantined chunk, and moves of a quarantined function, unless the request carries a human direction (item 6); functions moved in become quarantined with the chunk;
   - renames, reorders and context-only changes inside it;
   - new prerequisites on or from a quarantined record (`deps set`);
   - completion, reopen, review flags and claims on it (none are meaningful).
   Report text stays editable (decision 2): saves are allowed and never reopen the record. Each refusal names the quarantine, its reason, and how to proceed when the human has directed the work. Reads are unaffected, so agents and tools can still see what it is.
3. **Visible but never offered as work.** `node show`, `node list`, `status` and `read-batch` return a `quarantine` object (`origin`, `reason`, `at_revision`, `by`). `status --summary` reports a quarantined count. Quarantined records never appear as open or ready work (they count as complete), so agents do not pick them up; `research node quarantined <project>` lists them. Parent reports list the child as "(quarantined)" the way context inventory is listed today. The GUI shows a marker on the record in the browser, adds a "Quarantined" filter, and the editor shows the reason read-only. Meters count it as Complete and keep the approved meter design.
4. **Set and lift.** `research node quarantine <project> <node> --origin human|cyber-stop --reason "..."` and `research node unquarantine <project> <node> --reason "..."`. A quarantine has one of two origins: the human asked for it, or Claude issued a cyber (safety) stop while researching the record. The origin and the submitter (`SCOPEWORK_SUBMITTER`) are recorded. Lifting restores the stored status the record had before (status is kept untouched underneath). Every change is a catalog transaction recorded in the journal and the project history.
5. **Interaction with existing state.** Quarantining clears the record's review flag, claim and review-queue entry in the same transaction (keeping them in the history). Its report text and evidence are kept byte-for-byte.
6. **Human-directed work.** The human personally directs any agent that works on a quarantined chunk. Writes the seal refuses (moves in or out, renames, reorders, context changes, new prerequisites) are accepted only with `--directed-by "<the human's instruction>"`, which is stored with the change in the journal and history. Astra may take quarantined work on its own only when no other work is available, for completeness (rare); that is workflow policy for Astra, not something the app grants by name. Without the option, every such write is refused.

## Storage

A new journaled sidecar, `quarantine.json` beside `project.json`: `{node id: {origin, reason, at_revision, by}}`. Using a sidecar instead of a `Node` field keeps the catalog format and every existing tool reading `project.json` unchanged, and follows the flag precedent (journaled, drift-checked by the coordinator). Deleting a node drops its entry.

## Implementation outline

- `research.rs`: load and save the sidecar; `quarantined(p, id)` (the node or its chunk); effective status Complete for quarantined records in `effective`, `effective_all` and `completion_states` (`catalog_index::states_with` treats them as Complete and skips their hashes); refusals in `save_reports`, completion and flag paths.
- `catalog_batch.rs`: refuse updates that move into, out of, rename, reorder or change context of quarantined records, and dependency edges touching them, unless the batch carries `directed_by`; quarantine functions moved into a quarantined chunk.
- `cli.rs`: `node quarantine` / `node unquarantine` / `node quarantined`, `--directed-by` on `node batch`, `node save-batch`-adjacent structural commands and `deps set`, the `quarantine` field in node, list, status and read-batch output, and the summary count.
- `catalog_index.rs` `render`: "(quarantined)" suffix in parent source-report lists.
- GUI (`records_ui.rs`, `catalog_browser.rs`): marker, filter, read-only reason.
- Coordinator drift: add the sidecar to the journaled sidecar set.
- `history.rs`: quarantine and lift entries.
- Docs: `docs/cli.md` section, CHANGELOG, release notes.

## Tests

Regression tests in a small temporary project: a quarantined chunk with an incomplete function lets its architecture complete; moves in/out, renames and dependency edges are refused with the reason and accepted with `--directed-by` (recorded); report saves are accepted and do not reopen; a quarantined record's status is unchanged by editing other records; lifting restores the stored statuses; deleting a node drops its quarantine; journal recovery replays quarantine transactions; status counts and `node show` output include the field.

## Decisions (user, 2026-10-21)

1. **Lifting:** reversible with a reason; the previous status is restored.
2. **Report text:** editable; edits never reopen a quarantined record.
3. **Existing prerequisites** on a quarantined function count as satisfied; new edges to or from quarantined records are refused.
4. **Authority:** a quarantine comes from the human's verbal request or from a Claude cyber stop, recorded as its origin. The human personally directs any agent working on a quarantined chunk (`--directed-by`); Astra may choose such work only when no other work is available (rare). No standing collector list.

## Amendment (user, 2026-10-24): seal only structure

Decision 3 is narrowed. Some agents must work with sealed records, and later products (behaviour artifacts, the implementation tracker, specifications) build on them. A prerequisite **on** a quarantined record may now be added or removed without direction; changing the prerequisites **of** a quarantined record still needs `--directed-by`. Removal is sealed too: `node delete` of a quarantined record, or of a function in a quarantined chunk, needs `--directed-by`, and `node prune` passes over quarantined records without it. See [proposal-review-fixes.md](proposal-review-fixes.md), sections 2 and 3.

## Implementation notes (2026-10-21)

- Implemented as designed: the `quarantine.json` sidecar (records plus a `lifted` log), effective status Complete through the catalog index, the seal in batches, creation-only imports, `node create` and `deps set` with `--directed-by` stored on the transaction and shown by `history show`, refusals for completion, flags and claims, clearing of flags, claims and queue entries, the CLI commands and fields, the GUI marker, filter and editor notice, and the coordinator's drift watch.
- **Deviation:** parent reports do not list a child as "(quarantined)". Report text is rendered by several commit paths; annotating some renders and not others would change report bytes back and forth and reopen completed parents. Quarantine is shown through the CLI and GUI instead.
- **Detail:** the recorder's name is an explicit `--by` (the coordinator runs commands in its own process, where the caller's `SCOPEWORK_SUBMITTER` is not visible).
- **Regression tests:** `tests/quarantine.rs` (completion over a quarantined chunk, the seal with and without direction for create, move, prerequisites and import, editable text, refused flags and completion, lifting, deletion, and the coordinator path) and unit tests for the effective set and the browser filter.
