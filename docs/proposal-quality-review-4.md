# Proposal: app-wide code-quality review, round 4

Status: implemented 2026-10-29. Rounds 1–3 (2026-10-27) covered the whole app. Since then about 4,500 lines of `src` changed: data records, the store and its ledger follower, artifacts and spaces, compaction, promotion and `research annotate`. Round 4 reads those modules in full (`store.rs`, `store_ledger.rs`, `store_commit.rs`, `inputs.rs`, `annotate.rs`, `promotion.rs`), reads the changes to the others, and runs the duplicate-block search over all of `src` (six or more matching lines, test modules excluded). Fixes are the obvious ones only: no feature or behaviour changes.

## Findings and fixes

1. **Catalog transactions are spelled out field by field.** Eight `Journal` literals list every field, most at their empty defaults; adding `repin_pairs` meant editing eighteen. `Journal` and `Project` derive `Default`, and the literals name only the fields they set (`..Default::default()`).
2. **A creation's record is built twice.** Batch creation and import creation build the same `Node` from a `Create`. `Create::node` builds it.
3. **A segment is parsed from its log in three places.** `segments`, the writer's active-segment lookup and the closed-log check each read a log, find its start entry and check its checkpoint. `load_segment` does it once.
4. **The debug-only size overrides are written twice.** The segment size and the small-frame limit each read their environment variable the same way. `tunable` reads it.
5. **The ledger follower re-implements three helpers:**
   - its own hexadecimal address parser beside `parse_rva`;
   - its own copy of the ledger path beside `ledger::LEDGER`;
   - its own compressor beside the store's.

   It uses the shared ones; the store's compressor takes a level (`deflate_at`).
6. **Stale or wrong comments.** `store.rs`'s module comment still says nothing in it takes part in a commit, which is no longer true for promoted projects. `space_label`'s comment describes a lookup by name it does not do.

Not changed: two match-arm lists in `cli.rs` repeat the same command names (routing and project lookup); they are how each match lists its cases.

## Acceptance

- No behaviour change; the full suite, Clippy and fmt are clean.
- The duplicate-block search finds only the `cli.rs` arm lists.
