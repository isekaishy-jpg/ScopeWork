# Proposal: fixes from the Fable review of 0.15.0a, and the pool reason

Status: implemented 2026-10-29. Source: a review of the whole of `src/` at `1978a60` by Claude Fable (six findings, the two most severe verified by reading both code paths), and the complaint `2026-10-29-arch1-dnp-pool-reason-cannot-be-amended.md` recorded in TODO.md. Obvious fixes only; no feature changes beyond the pool reason the complaint asks for.

## Findings and design

### 1. Store sync and a promoted commit take two locks in opposite orders (medium)

**Problem.** `store sync` takes the store's writer lock, then waits up to two minutes for the project's shared lock. A promoted commit holds the project's exclusive lock and then waits 30 s for the store's lock. When both run, the commit times out twice and answers `commit_pending` after about a minute, with its reports already rewritten; the sync then finds the pending journal and answers `recovery_required`. Nothing is lost, but a maintenance command stalls every writer and produces a misleading answer.

**Design.** One order everywhere: the project's lock, then the store's. `sync` takes the project's shared lock first and the store's lock (which never waits) while holding it. Every other store writer already follows this order or takes no project lock.

### 2. `annotate stale` reads any path a ledger row names (low)

**Problem.** It reads and hashes `dir.join(row.report)` for every row. The ledger is written by tools outside Scopework's lock, so a row naming a path outside the project prints that file's hash, and a row naming a device such as `CON` blocks the command while it holds the project's read lock.

**Design.** It hashes only paths that are a record's report in the catalog. Any other row is listed as stale with the note that its report is not a record's report.

### 3. Every store writer reads every text pack in full (low, performance)

**Problem.** Opening the store's writer reads all `texts-N.pack` files to learn which texts are already stored (about 9 MB for `3-3-5a`, growing with history), on every promoted commit and every ledger follow. The release notes say a promoted commit reads only the active segment; that holds for logs only.

**Design.** The writer reads only each pack frame's header and index, seeking past the compressed texts. Readers and `store verify`, which need the texts or their checksums, still read whole frames.

### 4. A store that does not match is reported as `commit_pending` (low)

**Problem.** When a promoted project's store head is neither the commit's before- nor after-state (a store restored from an older copy), the append fails with `store_mismatch` after the reports were already rewritten, and the retry loop then answers `commit_pending`: "applied before any other operation... do not repeat". It never can be, and the project stays blocked.

**Design.** A promoted commit takes the store's lock and checks its head at the start of applying, before any file changes, and keeps the lock until the catalog is written. A `store_mismatch` or `store_damaged` there is not retried: on a fresh commit the pending journal is removed (nothing was applied) and the error says nothing was committed; on recovery it says the project waits for the store's repair. A busy store is still retried.

### 5. Two statements in the docs are not what the code does (low)

**Problem.**
- Promotion is documented to refuse when the ledger changed since the store last recorded it; the code does not check.
- Scopework's own ledger changes on a promoted project (re-paths and re-pins in a commit) are documented as recorded at once with their author; they are recorded at the next sync, without one.

**Design.** Promotion refuses when the ledger file's hash differs from the store's last ledger entry (`ledger_changed`, sync and promote again). A promoted commit that changed the ledger records it in the store under the lock it already holds (finding 4), naming the caller.

### 6. `annotate` can lose its author to the coordinator (low)

**Problem.** `annotate set|remove` writes the ledger, then records it. The coordinator's ledger thread can record the same bytes in between, and the author is never written.

**Design.** `annotate` takes the store's lock before reading the ledger and keeps it until the change is recorded. The coordinator's follower only tries the lock, so it waits for the next idle moment.

### 7. A pool's reason cannot be amended (complaint)

**Problem.** `pool add` with an existing pool and a new reason updates each record's entry but never the pool's own reason, which `pool list` shows first. Nothing else changes it.

**Design.** When every record a `pool add` names is already in that pool, a different reason amends the pool's own reason too, logged as a `reason` event. This is a change to those pooled records, so the existing guard applies: only members, the dashboard, or `--directed-by`. Adding new records with a different reason leaves the pool's reason as it is. The answer shows the pool's reason.

## Acceptance

- A regression test for each finding where one can be written: the lock order (a sync waiting on the project lock holds no store lock), the contained `stale` paths, the mismatch refusal before any file changes with nothing left pending, the promotion's ledger check, the recorded author for commit-time and `annotate` ledger changes, and the pool reason.
- The full suite, Clippy and fmt are clean.
