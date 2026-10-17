# 0.8.0 qualification

Scopework 0.8.0 is a maintenance release for several agents working on one large project. It answers three research-home complaints from 2026-10-17: CLI throughput, work and operation queueing, and a unified history log.

## Measurements

Measured on a real project catalog with 30,682 functions (9.8 MB `project.json` before compaction), on the development machine, comparing 0.7.0 behaviour with 0.8.0 on identical data. Results were identical before and after.

| Operation | 0.7.0 | 0.8.0 |
| --- | --- | --- |
| Warm `status` (every accepted report checked) | 1.94 s | about 0.43 s |
| Delta computation per catalog write | 263 ms | 86 ms |
| Identity and revision for a batch request | whole catalog (9.8 MB) via `project show` | about 300 bytes via `project revision` |
| Read during another process's write | fails `busy`; caller retries | waits (default 120 s) and answers |

The catalog write itself stays at about 50 ms per mutation. Catalogs are still rewritten whole (now as compact JSON); append-only catalog storage is not part of this release.

## Regression coverage

- `tests/lock_wait_and_catalog.rs`: readers wait for a writer, `--wait 0` answers `busy`, holder and waiter files appear and are removed, catalogs are compact, orphaned pending temporaries are removed.
- `report_hashes` unit tests: parallel bulk hashes match file contents and persist; a change inside the racy-timestamp window is always rehashed; an unchanged old file reuses its stamp; missing and non-UTF-8 reports never hash.
- `tests/cli_small_answers.rs`: `project revision` omits the catalog; `status`/`node list` filters and `--summary`/`--ready`.
- `tests/claims_and_review_queue.rs`: claim conflicts, coverage of chunk members, release; a move queues exactly the reopened completed records (moved function, both chunks, ancestors), reports the claimed destination, and entries clear when records are completed again.
- `tests/project_history.rs`: catalog receipts summarised by node and revision, tooling entries appended and filtered, reserved and malformed kinds refused.
- Record browser unit test for the review-queue filter.

All tests, `cargo fmt --check` and warning-denying Clippy pass with Rust 1.98.0.

## Not covered

- No operation queue with priorities or automatic re-basing of conflicting catalog edits; writers still take turns on the project lock, now by waiting instead of failing. Conditional jobs are not implemented.
- Effective status is still evaluated for the whole catalog; only report hashing is cached.
- Completion tokens and `save-batch` hash serially.
- No GUI history view; history is CLI only and cannot filter by RVA or date.
- The MSI was built locally but not installed or upgrade-tested for this note, because research sessions were using the installed 0.7.0 at the time.

These items remain in `TODO.md`.
