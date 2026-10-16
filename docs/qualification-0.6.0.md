# Scopework 0.6.0 qualification

Date: 2026-10-16. Platform: Windows x64, Rust 1.98.0.

## Changes checked

- Pending delta reconstruction from either pre-publication or post-publication catalog state; whole-catalog fingerprint conflict rejection; older full-journal recovery.
- Single-record metadata change and deletion against 1,005 in-memory nodes produce journals smaller than 4 KB. This test creates only a five-node disk fixture, not 1,005 report files.
- Completed-history compaction preserves pending files and retained change history. Standalone history commands neither migrate nor recover a project with an intentionally conflicted pending journal.
- Branch removal preserves evidence bytes, updates recognized report links and live dependencies, and recovers from process exits after journal creation, directory moves, report writes and catalog publication.
- Pruning preserves functions, authored source scopes and explicit keep selections.
- Format-5 migration preserves reports and stores the original catalog once.
- Existing import concurrency, token conflicts, cancellation, corruption detection, provider failure, locking and GUI state unit tests remain covered using small fixtures.

## Results

- 13 library tests and 15 CLI integration tests passed. One newly added assertion initially had an incorrect literal byte count; it was corrected to use the fixture length and the affected test passed on rerun.
- `cargo clippy --locked --all-targets -- -D warnings` passed.
- The 16-function smoke harness passed and removed its temporary project. A 150,000-count request was rejected before creating a fixture.
- The package build succeeded and removed its owned scratch directory. Injected Rust-build failure also removed scratch and restored TEMP/TMP.

No large on-disk inventory was generated. No live research catalog was migrated, pruned, deleted or compacted for qualification. Existing legacy history has not yet been reclaimed. Live GUI interaction and MSI install/repair/upgrade/uninstall were not exercised in this pass; successful packaging is not a claim of those runtime checks.

## Storage and compatibility limits

Catalog transactions retain changed records, report contents and sidecars; they no longer retain two unchanged full inventories per edit. Compact history and deleted-node evidence archives remain retained indefinitely, so actual changed content still consumes space. Bulk reorders may have large deltas. Import publication records and tool databases are separate storage categories.

The compatibility marker is now format 5. Finish or cancel active imports using the previous release before upgrading a project. Older executables reject the new format. History inspection/compaction works separately from project migration and pending recovery.
