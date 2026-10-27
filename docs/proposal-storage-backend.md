# Proposal: data records and the storage backend's first stage

Status: proposed 2026-10-27. Design: [design-storage-backend.md](design-storage-backend.md). Research: [research-storage-backend.md](research-storage-backend.md) §10–12, [research-data-records.md](research-data-records.md).

## Approach

Three increments, each committed on its own with tests, zero warnings, and its documentation (docs/cli.md, CHANGELOG.md):

1. **Data records** (`feat(catalog)`): `Kind::Data` with `length`, catalog format 6 raised by the first data item, `D<RVA>` folders, address identity across kinds, prerequisites across kinds, data counts, CLI, imports and batches, GUI record form and tree.
2. **The store and its follower** (`feat(store)`): a new `store` module (frames, packs, segments, the receipt walk), `research store init|sync|status|verify`.
3. **History from the store** (`feat(store)`): `research store history|show|catalog`, and the coordinator keeping stores current when a project's writer is idle.

## Tradeoffs

- **Follower, not dual write.**
  - Following receipts covers every writer, older builds included, and keeps the commit path untouched, so a store fault cannot fail a commit.
  - The cost is lag (until the next sync or idle moment), and gaps where a writer leaves no receipt (import publication, layout migration). A gap is recorded and the follower re-anchors on the current catalog.
- **JSON inside checksummed frames** keeps the log readable with a text viewer, at about 4× the size of a binary encoding before compression. Checkpoints and texts are compressed, so the uncompressed part is the log of record changes (about 40 MB for `3-3-5a`).
- **zlib, not zstd:** zlib is already in the lock file (no new download); the store names its codec, so zstd can come later.
- **Data in the current catalog** makes data usable now, with no wait for promotion. The price is a catalog format change (6) that older builds refuse, limited to projects that create data.
- **Flat data containment.** Nested items are not children, so a 104-byte table and a field inside it are siblings in their chunk. Child records would add a second hierarchy rule for one kind. Type fields arrive with annotations at promotion.

## Acceptance

As in the design's acceptance criteria. In particular:
- `store init` and `store verify` run on a copy of `3-3-5a` taken for the check (never on the live home).
- `store catalog --revision N` reproduces recorded fingerprints.
- Projects without data keep format 5 and their exact catalog bytes.

## Continuation path

- **After this stage**, the agents use the store's history reads side by side and file complaints.
- **Next stages** (each needs its own proposal):
  - repacking texts and absorbing receipts;
  - ledger annotations ingested at promotion;
  - artifacts and address spaces (data in files);
  - the commit-path interface and promotion;
  - exporting `project.json` from the store.
- **If increment 2 or 3 stalls**, increment 1 stands on its own.
