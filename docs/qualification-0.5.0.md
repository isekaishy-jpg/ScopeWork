# Scopework 0.5.0 qualification

Date: 2026-10-14. This release implements concurrent additive imports and background GUI snapshots. It is distributed as a prerelease because native visual GUI inspection was unavailable. Functional and headless checks are not a substitute for that inspection.

## Software checks

- 24 focused regression tests passed: nine library/UI tests, three meter-layout tests, and twelve multi-process integration tests. Warning-denying Clippy and formatting checks passed.
- Publication fault cases cover termination after planned/prepared segments, readiness, publication journaling, parent replacement and catalog replacement. Resume is idempotent.
- Independent drafts survive unrelated changes; stale same-record edits and changed completion inputs are rejected. Schema-2 draft batches retain all-or-nothing validation.
- Cancellation preserves changed/unowned reports and added evidence, releases reservations without reusing IDs, and recovers after catalog release before receipt completion.
- Injected file-provider failure leaves a resumable job. This exercises the I/O-error path; it is not a physical disk-full or power-loss test.
- Shared reads coexist across processes and exclude writers. Mixed structural transactions remain exclusive. OS-level writer fairness is not guaranteed; a busy response remains possible and should be retried with bounded backoff.
- GUI freshness tests reject stale loads/snapshots, retain unsaved drafts while meters update, bound queued snapshot publication and reject delivery after shutdown.
- The app-wide sweep also fixed non-journaled single-node creation, binary/dependency edits, interrupted empty journal admission, foreign-folder adoption, reserved Windows project names, dangling-link containment and repeated record indexing.

## Representative import

Reference machine: Windows 10 Pro 10.0.19045, Intel Core i5-9600K (6 cores), approximately 32 GiB RAM. Both runs used the same local workspace volume and generated the same 150,000 functions plus 155 hierarchy records. One baseline/candidate run pair was used; normal development activity was present, so this is not a controlled throughput comparison.

| Measurement | Result |
| --- | --- |
| 0.4.5 baseline import | 548.120 seconds |
| 0.5.0 candidate import | 596.967 seconds |
| Candidate file preparation | 467.361 seconds |
| Candidate prepared-output verification | 113.004 seconds |
| Candidate CPU rendering | 0.719 seconds |
| Reservation lock | 118 ms |
| Publication candidate | 430 ms |
| Publication lock | 227 ms |
| Publication lock wait | 0 ms |
| Existing-record reads during preparation, 20 samples, p95 | 12.3 ms |
| Existing-record saves during preparation, 20 samples, p95 | 34.4 ms |

The concurrent probes addressed an existing report while the 150,000 new functions were still unpublished. All probes succeeded, and the final report retained the edits. The importer is approximately 9% slower end-to-end in this pair; the improvement is availability and recoverability, not a demonstrated throughput gain. Verification now has its own visible state/count and checks pause/cancel requests between segments.

The final full-catalog fixture has 150,159 committed records. With another import paused, ten ordinary reads took 333–530 ms and ten saves took 1,676–1,917 ms. The proposed 250 ms interactive CLI target is **not qualified for an already populated 150,000-function catalog**. This scale is supported for correctness and background GUI use, with approximately two-second single-report saves on this machine. Full-catalog serialization/journals remain the limiting storage cost; a segmented metadata design is a separate follow-up. No globally fair or constant-time commit claim is made.

A headless egui run on the full fixture measured 1,784 frames over 15 seconds: p95 update/layout time 0.136 ms, maximum 1.606 ms, first snapshot approximately 900 ms. It exercised the report editor and background polling with a 1280×900 layout. It does not measure native window rendering, display latency or user interaction. A writer attempted during initial snapshot capture received the documented busy response; subsequent large-catalog measurements used bounded retry.

Run artifacts remain under the ignored `target/qualification` directory. The manual harnesses are `scripts/qualify_import.py` and `examples/qualify_gui.rs`. The latter requires an explicit isolated `RESEARCH_DASHBOARD_HOME`.

## Compatibility and packaging

- Actual 0.4.3, 0.4.4 and 0.4.5 CLI binaries extracted from their MSI packages refused a migrated format-4 fixture with a paused job. Catalog, canonical reports and job files remained byte-for-byte unchanged.
- Installed an isolated 0.4.5 package, created synthetic research, upgraded to 0.5.0, read/migrated the project with the installed CLI, and uninstalled. Canonical research and a sentinel file survived. Live application/research settings were unchanged.
- The isolated packages used alternate product/component/upgrade identities, registry keys and shortcut folder names. Production installer authoring was otherwise retained. This checks installer behavior without upgrading the user's active installation; it is not a live-installation upgrade claim.
- Production UpgradeCode, executable names and research-home rules remain unchanged. Both GUI and CLI ship together. Upgrade all workers before adopting format 4; manual catalog-version downgrade is unsupported.
- The Windows computer-use runtime failed to initialize (`failed to write kernel assets`), so no native visual GUI or installer-dialog pass is claimed.

The durability guarantee is recovery from process termination with available, uncorrupted stored files. Flushed checkpoints and catalog-last publication do not establish power-loss/storage-controller ordering for every prepared file. Keep ordinary research backups.
