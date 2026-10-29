# Proposal: app-wide bug review, round 3

Status: implemented 2026-10-29 (rounds 3 and 4). Rounds 1 and 2 (2026-10-27) covered the whole app. Round 3 reads what changed since, after quality rounds 4 and 5: the store and its ledger follower, compaction, the promoted commit path and its recovery, promotion, `research annotate`, and every place a record is found by its address now that addresses are unique per space. Only obvious fixes; no feature changes.

## Findings and fixes

### 1. A damaged store segment could be deleted with its entries (high)

**Problem.** When the store's last segment log had no readable start entry, or its checkpoint was missing, both the writer's repair and the active-segment lookup treated it as a segment whose creation was interrupted and deleted the log. An interrupted creation leaves a log with no complete frame at all; a log whose first frame is damaged, or whose checkpoint was removed, still holds entries. Deleting it loses every commit since the last checkpoint, and for a promoted project the store is the only history.

**Design.** A log is removed only when it holds no complete frame (`unfinished`). Anything else is refused as `store_damaged` and left for `store verify`, as damage in a closed segment already was. A regression test removes an active segment's checkpoint and checks that the sync refuses and the log stays, and that an empty log left by an interrupted creation is still removed.

### 2. Prerequisites in another address space could not be given back (medium)

**Problem.** `node show` lists a record's prerequisites as bare RVAs, and the dashboard fills its prerequisites box with that list and saves it back. A prerequisite in another space was listed as `0x00001000`, which `deps set` reads as the default space: saving the record's prerequisites unchanged would replace it with the default-space record at that address, or fail.

**Design.** One `address_text` writes a spaced record's address as `S<n>:0x…`, the form `deps set` reads; `node show`, the dashboard's prerequisite list and its "used by" line use it. `node list` and implementation links also show each record's `space`. A test checks that the list reads back.

## Checked, no defect

- An interrupted promoted commit (before the store append, after it, after the catalog) finishes on the next open without appending twice (covered by `tests/promotion.rs`).
- Promotion between its checks and its lock: a commit in that window is caught by the head check made under the project's exclusive lock.
- Import publication on a promoted project takes its before-catalog from `project.json`, not from the in-memory catalog the publish path hands over.
- Every remaining lookup by RVA is either space-aware or deliberately default-space (the annotation ledger, history's `--rva`, rename citations).

## Round 4 (same day)

Round 4 read what round 3 did not reach: compaction, the ledger follower's edge cases, artifact and space editing, the body-hash cache, the coordinator's ledger thread, and how history reports the new kinds of commit.

### 3. A store reader could fail while a compaction replaced the packs (low)

**Problem.** Store readers take no lock. A reader that indexed the text packs just before a compaction rewrote and removed them failed to open the old pack when it read a text (`store show`, the ledger history).

**Design.** A pack that is gone makes the reader index the store again and read once more. A unit test reads a text through an index taken before a compaction.

### 4. History showed artifact, space and format changes as empty commits (low)

**Problem.** History reports changes to the catalog's own fields from a fixed list that predates artifacts and spaces, so adding an artifact or space, raising the catalog format or promoting a project appeared as a commit that changed nothing.

**Design.** `artifacts`, `spaces` and `layout_version` join the list. A test checks the entry for an added artifact.

## Acceptance

- The full suite, Clippy and fmt are clean.
