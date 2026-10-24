# Proposal: bounded report-hash memory

Status: implemented 2026-10-24.

## Problem

The dashboard's memory grows with every agent commit and is never returned. It reached 50 GB on 2026-10-21, when it aborted on a failed allocation. The running GUI was at 3.9 GB after 22 hours on 2026-10-23. Measured: about 20–30 MB per commit, flat between commits. Complaint: `2026-10-23-agent-gui-memory-leak-per-refresh-thread.md`; the corrected `resolved/2026-10-21-agent-gui-abort-0xc0000409.md`.

## Research

- `report_hashes.rs` keeps a thread-local in-memory table of report stamps (Solcache). Its cleanup context is created with `Box::leak` once per thread, because the table borrows it for `'static`.
- In Solcache, dropping a table pushes every entry onto its cleanup context's queue (`impl Drop for Backing`). Only `drain()` or dropping the context destroys them. The context is leaked, so a thread that ends leaves every entry it cached. Draining before the thread ends would free the entries, but the leaked context and its queue's allocated capacity would remain per thread.
- `SCCleanupContext` holds a `PhantomData<Rc<()>>`: it is bound to its thread and cannot be shared or handed back.
- The GUI starts a new `std::thread` every 2 s for its refresh (`ResearchUi::poll`). When anything changed, the refresh computes every project's meters, which stamps the reports of every complete record in every project: 29,836 today. Record loads, actions and the history window also run on fresh threads. In the coordinator, each client connection is served on a new thread.
- So the table's purpose, reusing hashes across GUI refreshes, never applied: every refresh started with an empty table. Its only effect on short-lived threads was the leak.
- **A second leak, on any thread that keeps its table.** `hashes` re-installs every reusable stamp, including one it has just read from the table. Each install replaces the entry, and the replaced one waits on the cleanup queue. The queue is drained only when the table exceeds its 64 MiB budget, and the budget does not count queued entries. With about 30,000 entries (about 7 MB) the budget is never reached, so the queue grows by every evaluated report on every evaluation. Measured by running `background::snapshot` repeatedly under a counting allocator: live memory grew 4.4 MB per refresh with the first part of this design alone, and stayed at 129.6 MB on every refresh with both parts.

## Design

1. **Retention is opt-in per thread.** A thread keeps report stamps in memory only after calling `report_hashes::retain_on_this_thread()`. On other threads, lookups and inserts skip the in-memory table. Bulk evaluations still use the persisted copy (`scopework-cache/report-hashes.json`), and small ones hash directly, which is what those threads effectively did before. No thread that ends leaves anything behind.
2. **One long-lived refresh thread in the GUI.** A `RefreshWorker` owns one named thread that runs refresh jobs in order and retains stamps. `poll` hands it the snapshot job instead of spawning a thread. A job that panics is caught, so the thread survives. The refresh receiver disconnects as before, and the GUI shows "Snapshot worker failed". If the thread has gone, the next refresh starts a new worker.
3. **The coordinator's writer threads retain.** Each project's `write_loop` thread lives as long as the coordinator and evaluates completions on every commit, so it opts in. Connection threads do not.
4. **Bounded.** Retaining threads are few and long-lived. Each table stays within the existing 64 MiB budget, enforced by the existing maintenance.
5. **Drain on every insert, and no re-insert on a memory hit.** A stamp read from the table is used as it is. A stamp from the persisted copy, or a new hash, is installed, and the cleanup queue is drained right after, so replaced and evicted entries are freed at once.
6. **Dropping a retained table drains its context,** so a retaining thread that does end (for example in tests) frees its entries.

## Acceptance

- A thread that has not opted in creates no in-memory table, whatever it hashes.
- A retaining thread reuses an unchanged file's stamp from memory, as before.
- Refresh jobs run on one thread.
- The GUI's memory stays flat across commits once its table is warm.

## Tests

Unit tests:
- hashing on a fresh thread leaves no table;
- a retaining thread reuses stamps (the existing reuse test opts in);
- repeated evaluations on a retaining thread, including one that replaces a changed report's entry, leave nothing queued;
- dropping a retained table leaves its cleanup queue empty;
- two `RefreshWorker` jobs run on the same thread, and a panicking job does not stop the next.

## Validation

- A release build of this change ran beside the installed 0.13.1a dashboard on the live research home, and 20 refreshes were forced by touching one report's modification time (its content unchanged, restored afterwards).
- The new dashboard rose to about 305 MB as its table warmed, then varied between 305 and 334 MB, falling back to 310 MB twice. The installed dashboard grew from 4,090 to 4,235 MB over the same refreshes.
- Each regression test was confirmed to fail without its fix (8 entries left after drop; 40 queued after repeated evaluations).
