# Proposal: the storage backend's remaining stages

Status: proposed 2026-10-28. Design: [design-storage-promotion.md](design-storage-promotion.md). Research: [research-storage-stage2.md](research-storage-stage2.md).

## Approach

Five increments, each committed on its own with tests, zero warnings, and its documentation (docs/cli.md, CHANGELOG.md):

1. **Ledger follower** (`feat(store)`): ledger base and ledger entries in sync, the ledger chain in `verify`, `store annotations`, `store conflicts`, ledger facts in `store status`.
2. **Artifacts and address spaces** (`feat(catalog)`): catalog fields and format 7, per-space identity and folders, `project artifact|space add|update|remove`, `--space` on creation, the space in report headers and the record panel.
3. **Store maintenance** (`feat(store)`): `store compact` (packs merged, closed logs compressed), readers of compressed logs, automatic compaction past 64 packs.
4. **Promoted commit path** (`feat(store)`): the backend dispatch in `catalog_batch::commit`, the Store backend's commit and recovery, history from the store, import publication through the backend, refusals for layout migration, tidy and journal maintenance.
5. **Promotion and annotations** (`feat(store)`): `store promote` with its refusals, acceptance conversion and annotation take-over in one entry, body hashes in the report hash cache, `annotate set|remove|stale`, import of outside ledger writes.

## Tradeoffs

- **Following the ledger before promotion** gives conflicts and history now without touching the tools' authority. The price is sync-granular ledger history (several tool writes between syncs become one entry) and one stored copy of the whole ledger as the base.
- **Annotations keyed by address, not by item.** 20,365 data addresses have annotations and no data record. Keying by `(rva, kind)` as the ledger does keeps every row through promotion without forcing records to be created first.
- **Artifacts and spaces in the catalog** make them usable now (the Populous data disks can hold research), at the cost of a format change (7) for projects that use them, exactly as data did with 6.
- **Receipts stop at promotion** rather than being kept beside the store: the store's entries carry their facts with texts by hash, so keeping both would double the history for no reader. Receipts already on disk stay.
- **`project.json` stays as an export** instead of readers loading the store directly: every reader (the coordinator, the GUI, 14 project tools) keeps working unchanged. The export costs the whole-file write per commit that the store would otherwise save; moving readers off it is a later stage.
- **Acceptance conversion inside promotion,** not on its own: the user's decision requires the ledger pins to convert in the same transaction, and Scopework owns those pins only from promotion. A project that is never promoted keeps whole-file acceptance.
- **Outside ledger writes imported, not refused,** after promotion: the tools keep working with no change except their staleness check; refusing them would stop the agents' publishing until their tools change.
- **No demote command.** Promotion keeps copies of the pre-promotion catalog and ledger and every receipt, so a return is possible by hand; a command for it would need its own reverse conversion of acceptance hashes.

## Acceptance

As in the design's acceptance criteria. In particular:
- Projects that are not promoted behave exactly as before, and the existing suite passes unchanged.
- Increments 1 and 3 are validated on a copy of `3-3-5a` (never on the live home); increments 4 and 5 on fixtures and on a copy of `3-3-5a` promoted for the check.

## Continuation path

- **Rollout:** these increments ship in an unreleased build until the user decides to release one; agents use the store side by side once it is installed.
- **Before promoting `3-3-5a`:** the agents' tools move their staleness check to `research annotate stale` or body hashes (user-directed), and `store conflicts` reports none.
- **Later stages, each with its own proposal:** readers loading the store instead of `project.json`, deleting receipts the store has absorbed after promotion, cross-project references, and the GUI entering artifacts and spaces.
- **If an increment stalls,** the earlier ones stand on their own: 1–3 change nothing on the commit path, and 4 is inert until a project is promoted by 5.
