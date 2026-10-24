# Proposal: fixes from the 2026-10-24 code review

Status: implemented 2026-10-24. Research is the review itself: a full read of `src` by two independent reviewers, with three findings reproduced on a throwaway research home. The user asked for every finding to be fixed, including those that were cut from the ranked list or rated only plausible, so that no known bug stays open.

Each section below gives the problem, the evidence and the design. The numbers are those of the ranked review.

## 1. A batch that restates current values reopens completed records

**Problem.** `apply_batch` adds every updated record, and its parent, to the changed set whether or not anything changed. With no real rename in the batch, every changed record counts as structural (`renames.is_empty()`). A `parent` or `context_only` field counts as structural just for being present, and so does a reorder of a parent. Structural records are marked changed: their acceptance is cleared, Complete becomes In Progress, their complete ancestors reopen, and they are queued for review.

**Evidence.** A schema-2 batch renaming the Complete function F1 to "F1" answered `reopened: [3]`, and F1 became In Progress, revision 4 to 5, queued "Reopened by a membership, order or name change".

**Design.** An update changes only the fields whose values differ: a `parent` equal to the current parent, a name equal to the current name (after trimming) or a `context_only` equal to the current value is ignored. A record enters the changed set, and the structural set, only for a real change. A reorder counts only if the order differs. A batch with nothing left to change commits nothing and answers as today's empty batch does. The schema-2 `expect_node_revision` check still applies to every listed record.

## 2 and 3. The quarantine seal: prerequisite spellings and removals

**Problem.**
- `set_dependencies_directed` reads the RVA list twice with different rules. The seal check splits on commas only and strips only `0x`. The edge builder also splits on whitespace and strips `0X`. Reproduced: `0x2000` (a quarantined function) is refused, `0X2000` is accepted without `--directed-by`.
- `remove_nodes` (delete and prune) never checks the seal, so a quarantined record, or a function of a quarantined chunk, can be removed without the human's direction.

**Decision (user, 2026-10-24): seal only structure.** Some agents must work with sealed records, and later products (behaviour artifacts, the implementation tracker, specifications) build on them. Therefore:
- a prerequisite **on** a quarantined record may be added or removed without direction;
- changing the prerequisites **of** a quarantined record (it is the dependent) still needs `--directed-by`;
- removing a quarantined record, or taking a function out of a quarantined chunk by removal, needs `--directed-by`.

This amends decision 3 of [proposal-quarantine.md](proposal-quarantine.md), which refused new edges in both directions.

**Design.**
- One RVA parser serves the whole command: commas or whitespace separate entries, `0x` or `0X` is optional, and an invalid entry is an error. The seal check covers only the dependent record.
- `node delete` takes `--directed-by` (and the request file's `directed_by`). Without it, a removal set that holds a quarantined record is refused, naming it. `node prune` never selects a quarantined record without direction: it is excluded with the reason `quarantined`, so a prune preview still works. The direction is stored on the transaction, so history shows it.

## 4. The coordinator holds its server-wide lock while waiting for one project

**Problem.** `Server::project()` loads a project's first snapshot while holding the mutex over all projects, and loading waits up to 120 s for that project's file lock. Every request for every project waits behind it.

**Design.** Look up the project under the mutex. If it is missing, release the mutex, load the snapshot, then take the mutex again and insert it unless another request inserted the project meanwhile; in that case use theirs and drop this snapshot. The writer and drift threads start only for the inserted state.

## 5. One unreadable project blanks the whole library

**Problem.** `Library::open` (used by `project list` and project creation) and the dashboard's refresh return the first error from any project that is not "busy". One stray folder, such as a copy named `wow-335a-12340 - Copy`, or a corrupt catalog, empties the dashboard and fails `project list`.

**Design.** Both skip a project that cannot be read and report it:
- `Library` gains `unreadable: Vec<(id, error)>`;
- `project list` lists the readable projects and writes one JSON warning line per unreadable project to standard error, kind `unreadable_project`. The exit status stays 0;
- the dashboard keeps a previously loaded catalog for such a project, as it does for a busy one, and shows "Project X could not be read: ..." in the Research records status bar when the set of unreadable projects changes.

## 6. The coordinator's resident catalog skips import recovery

**Problem.** Between batches, the coordinator's writer keeps its catalog in memory and reuses it after re-taking the lock, if no catalog journal is pending and `project.json` is unchanged. It does not check for an interrupted import publication (`publication/pending.json`) or a legacy pending journal, which `open_project` would recover first. A batch committed on that catalog makes the publication's later recovery fail with `recovery_conflict`.

**Design.** The resident catalog is reused only when nothing needs recovery: no pending journal in either transaction folder and no pending import publication (`imports::needs_recovery`). Otherwise the writer re-opens the project, which recovers.

## 7. Record names can break the report envelope

**Problem.** Names are only checked for emptiness. A name holding a newline and `<!-- research-content -->` is rendered ahead of the real marker in its own report and in its parent's source list, and `parts()` then splits at the wrong place. Both reports become `malformed_report` until the name is changed.

**Design.** One rule for record names, used by `node create`, batch create and update, and imports: trimmed, one line (no control characters), not empty, and not containing `<!-- research-`. Existing records are not checked, so nothing that opens today stops opening.

## 8. `job run` ends a `research session`

**Problem.** `run()` prints the `job run` answer and calls `std::process::exit`. Inside a session, that ends the session after one pretty-printed answer, and later lines are never answered. Reproduced: 1 of 3 lines answered. `serve` is not refused inside a session either, and would never return.

**Design.** `run()` returns the `job run` answer like any other command. `main` exits with the answer's `exit` code, so the standalone command behaves as before, and a session answers it as one JSON line. A session refuses `serve` the way it refuses `gui`.

## 9. A cancelled shutdown leaves the coordinator refusing work

**Problem.** When Windows asks whether the session may end, the coordinator stops admitting work. If the shutdown or logoff is then cancelled (`WM_ENDSESSION` with `wParam` 0), nothing re-admits it. Every write falls back to direct mode and leases are refused, and with active clients it never goes idle and exits.

**Design.** A cancelled end of session re-admits work, unless the coordinator is already draining because it was stopped or is retiring. A `draining` flag set by `drain_and_exit` decides that.

## 10. A landed change is reported as `commit_pending`

**Problem.** If `finish` fails after renaming the pending folder to its receipt (when writing the receipt's final journal), the retry finds the receipt and answers "Batch receipt already exists" for 20 s, then `commit_pending`. But no pending journal remains, and the change has fully landed.

**Design.** When the receipt exists and the pending folder no longer does, a retry treats the rename as done and goes on to write the receipt's journal. The caller gets the ordinary success.

## 11. A read just before the writer publishes an outside commit

**Problem.** The writer raises its `writing` flag (readers then skip their checks for outside commits) before it publishes a catalog that another process committed. A read in between answers from the old catalog against changed files.

**Design.** The writer publishes an outside commit before it raises the flag. It already holds the project lock at that point, so nothing else can change the files.

## 12. Drift records Scopework's own ledger writes

**Problem.** Ledger changes are excused only by receipts with `"ledger":true`. Re-pinning (`repin_ledger`, for example a rename batch that keeps completion) also rewrites the ledger. Declared files are also checked while a transaction is in flight, before its receipt exists. Both cases can record a permanent false drift entry for `tools/profiles/annotations.json`, which both live projects declare.

**Design.**
- Receipts with `"repin_ledger":true` excuse ledger changes too.
- While a transaction is in flight, declared files are not checked and their stamps are not advanced. The next poll after it lands sees the change together with its receipt, and the lease window keeps the earlier start.
- The two regular expressions for the catalog head are compiled once, not on every poll.

## 13. A tracker problem blocks record loads

**Problem.** Loading a record in Research records also asks the implementation tracker where it is implemented. Any tracker read error fails the whole load. Every click also reads every tracker shard.

**Design.** The record loads whatever the tracker answers, and an unreadable tracker shows "unavailable" in the Implemented in field. The answer comes from a placement map built once per project revision and cached by the process. Tracker changes raise the revision, so the cache never serves an outdated placement.

## 14. A GUI group change drops projects it has not loaded

**Problem.** Set group and Ungroup prune the settings to the dashboard's last loaded project list, while the CLI uses the folders on disk. A project created since the last refresh, or one the refresh could not read, silently leaves its group.

**Design.** Both use one function that lists the project folders holding a `project.json`. The History window also discards an answer that arrives after it switched to another project, instead of showing it under the new project's name.

## 15. A busy project makes every refresh re-read every catalog

**Problem.** While any project is busy, the refresh clears its fingerprint so that the busy project is retried. The next poll then re-reads and re-parses every catalog (about 40 MB for `forever`) and recomputes every meter, every 2 s.

**Design.** The refresh keeps a signature per project: its `project.json` stamp plus the stamps of its completed reports and the selected record. A project whose signature is unchanged since a successful read reuses its catalog, meters and browser data, and only busy or changed projects are read again. Busy projects reuse their previous data without copying the catalog. **Refresh research progress** still re-reads everything.

## Smaller findings

- **Temporary files.** `atomic` removes its temporary file when the write or sync fails. A waiter ticket whose lock could not be taken is removed instead of left behind for a second.
- **Waiter tickets after a clock change.** A ticket dated in the future (the clock moved back) is no longer treated as fresh forever. It is judged by its lock alone, like any ticket older than a second.
- **Lock files for missing projects.** A read-only open of a project that does not exist takes no lock and creates no lock file. `research status typo` no longer leaves `.project-locks/typo.lock` behind.
- **The GUI queue poll.** The direct-mode queue view no longer probes for shared readers. That probe briefly held the project lock exclusively every 2 s and could make a `--wait 0` caller report `busy`. `research locks` still reports readers.
- **Claim leases.** `--lease-hours` too large to represent is refused instead of overflowing (a panic in debug builds, an expired claim in release builds).
- **Import routing.** One rule decides whether a batch is a background import. The coordinator's routing ignored `repair_legacy_links`, so such a batch ran outside the queue.
- **Citation index.** Hexadecimal tokens of 8 to 16 digits are indexed, so renames whose VA has more than eight digits (image bases at or above 4 GiB) reach the reports that cite them. The index file becomes `report-references-v3.json`.
- **Import publication.** Publication re-checks for a pending catalog journal after taking its lock, and if one exists retries with a fresh read, which recovers it.
- **Tracker cycles.** Paths and roll-ups stop at a node already visited, so a hand-edited `tree.json` with a cycle cannot hang the tracker. `impl place` lists each record once even when it is named twice.
- **Dead code.** A sort of the move rewrites after they were consumed is removed.

## Tests

- Batch: a same-name rename, a current `parent`, a current `context_only` and an identical reorder leave a Complete record Complete and commit nothing. A real rename in the same batch still applies.
- Seal: `0X` and space-separated spellings parse alike. A prerequisite on a quarantined function needs no direction. Changing a quarantined function's own prerequisites does. `node delete` of a quarantined function is refused without `--directed-by` and accepted with it, recorded in history. Prune excludes a quarantined chunk with the reason `quarantined`.
- Names: names with a newline or `<!-- research-` are refused by `node create` and batch update.
- Session: `job run` is answered as one line and the session goes on. `serve` is refused.
- Library: `project list` lists readable projects and warns about an unreadable folder. A read of a missing project creates no lock file.
- Commit: a retry after a failed receipt write succeeds.
- Coordinator: two first requests for a project whose lock is held do not block a request for another project.
- Drift: a repin-only receipt excuses its ledger change. A ledger change while a transaction is pending is judged after it lands.
- Units: claim lease overflow, the citation index with nine-digit VAs, and tracker cycle guards.
