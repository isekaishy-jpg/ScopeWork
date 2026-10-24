# Proposal: fixes found by the storage architecture research

Status: proposed 2026-10-24. Research: [research-storage-architecture.md](research-storage-architecture.md), section 11. These are standalone fixes. They do not depend on the storage backend, and they make today's files a cleaner input for it.

## 1. Every catalog change leaves a receipt

**Problem.** Three writers change `project.json` directly (`Library::persist`), bumping the revision with no receipt in `catalog-transactions-v4`:
- `set_scope`;
- import reservation (`next_id`, `active_import`);
- the release of a cancelled import's reservation.

They break the receipt chain, which is otherwise verifiable by `before_hash`/`after_hash` across every project. Scope changes appear nowhere in history: `forever` revision 4,557 changed the scope from "Rendering, GPU, Asset Platform" to "Whole executable" without a trace.

**Design.**
- All three commit through the ordinary journal (`commit_metadata_with`, with no report edits), so each gets a receipt, a pending-journal commit point and crash recovery like any other change. `persist` is removed.
- History summaries list changes to the catalog's own fields (`project`: research scope, name, binary identity, active and last import), and `history show` prints them ("scope -> Whole executable").

Import publication keeps its own journal and receipt in the job folder, which links into the chain by hash.

Layout migration also changes the catalog without a receipt (the old project's one hash break, revisions 1,491–1,492). It is left as it is: it runs only on projects older than layout 5, every project in the research home is at layout 5, and none is pending.

## 2. Date filters use the commit time recorded in the receipt

**Problem.** `history show --from/--to` filters receipts by their journal file's modification time, not by the `at_unix` they record. A copy, restore or rewrite changes that time; receipt 1,491 of the old project is newer on disk than 1,492.

**Design.**
- A receipt whose modification time is at least two seconds before `--from` is skipped without parsing: `at_unix` is never later than the file's last write.
- Every other receipt is parsed and filtered by `at_unix`, falling back to the modification time only when the receipt has none (receipts older than 0.13).

## 3. One rule for report paths

**Problem.** The rule that maps a record to its report path (`app.md`, `architectures/<id>/research.md`, chunk `<id>`, function `<RVA>`) is written twice: `research::relative` and `catalog_index::Index::new`. A change to one would silently break the other.

**Design.** One function computes a record's report path from its parent's path. `relative` and `Index::new` both call it.

## 4. The GUI reads sidecars at the same revision as the catalog

**Problem.** The GUI's refresh reads each project's catalog under the project's shared lock, then releases the lock and reads the flags, review queue and quarantine sidecars. A commit in between pairs a catalog at one revision with sidecars from a later one.

**Design.** The refresh reads the three sidecars while it still holds the lock, together with the catalog. It hands them to the browser data builder, which today reads them itself. The meters are still computed after the lock is released, as now.

## Tests

- `set-scope`, an import's reservation and a cancelled import's release each leave a receipt, and the revisions are unbroken from creation.
- `history show` reports the scope change.
- A date filter selects a receipt by its recorded time even when its file's modification time is moved outside the range.
- Report paths for every kind, unchanged against a fixture.
- The GUI snapshot's sidecar reads come from the locked library (exercised through `background::snapshot` on a fixture home).
