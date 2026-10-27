# Design: data records and the storage backend's first stage

Status: design, 2026-10-27. TODO items "Storage backend" and "Data as a record kind". Research: [research-storage-backend.md](research-storage-backend.md) (decisions in §10, measurements in §12), [research-storage-architecture.md](research-storage-architecture.md), [research-data-records.md](research-data-records.md).

## Scope

The user's rollout rule decides the shape: the files stay authoritative until the user promotes the backend, and migration is a manual step. This design covers what can be built and used before promotion:

1. **Data records** in today's catalog, so data is researched the way functions are.
2. **A store beside the files** that follows every commit, keeps the project's whole history in a compact, checksummed form, and answers questions the files cannot: what a record and its report were at any revision, and the whole catalog at any revision.

Promotion, the interface the commit path will use afterwards, annotations, artifacts and address spaces, and cross-project references are later stages (see "Later stages").

## Part A: data records

### Model

- A new leaf kind, `Data`, whose parent is a chunk. A chunk holds functions and data items in one ordered list of children.
- A data item has a start address (`rva`, required) and a byte length (`length`, required, at least 1). `length` is a new optional field of every record, written only when present, so catalogs without data serialise exactly as before and keep their hashes.
- **Identity:** an address names at most one record, function or data. Data items may contain one another (the extents decide); they stay flat in their chunk. Function extents are not in the catalog, so "no data inside a function body" is not checked.
- **Report location:** `<chunk folder>/D<8-digit hex RVA>/research.md`, beside the chunk's function folders (`0041A2B0/`). The `D` keeps the two kinds' folders apart.
- **Lifecycle:** as a function's: stored status, evidence required for completion, `data/` folder, review flags, claims, quarantine and pools. Proofs do not apply: a proof is a recreation of code.
- **Completion criteria:** "Explain this data item's layout, values, meaning and users, supported by evidence. Complete only the prerequisites necessary to establish its meaning."
- **Prerequisites** connect functions and data in any direction (function to data, data to function, data to data). The rule stays "leaf to leaf".
- **Format:** a catalog with at least one data item has `layout_version` 6. The first data item raises it; a catalog without data stays 5, so older builds keep working on projects that do not use data. Older builds cannot read a version 6 catalog (they refuse it), which is the intended gate.

### Where it shows

- **CLI:**
  - `node create --kind data --rva --length` under a chunk;
  - `length` in a batch `update` for data items;
  - `node list --kind data`;
  - batch creation and import creation accept `"kind":"data"` with `length`;
  - `status` counts data items beside functions (`data: {total, complete}`), never inside the function totals.
- **GUI:** the record panel shows a data item's RVA and length and offers **Add data item** on a chunk; the tree labels data rows with their address and length. The dashboard's meters and totals keep counting functions only, so the approved dashboard layout is unchanged; data counts are in `status`.
- **Coordinator:** the read class is `data`.

## Part B: the store (side by side)

### Placement and ownership

- One store per project, in `<project>/store/`. Untracked, like `data/` folders.
- **The store follows the files; it never takes part in a commit.** Every writer that matters (the coordinator, direct mode, the GUI, older builds) leaves a change receipt per revision. The store reads those receipts and catches up. Nothing in today's commit path changes, so a store failure can never fail or slow a commit.
- **Creating a store is the manual migration step:** `research store init <project>` imports the receipt history and starts following. Without that step nothing changes for a project.

### Files

| File | Contents |
|---|---|
| `store.json` | `{"schema":1,"project":…,"codec":"zlib"}`, written at init |
| `checkpoint-<revision>.zlib` | The catalog (`project.json` bytes) at that revision, zlib-compressed |
| `log-<revision>.log` | Framed entries following the checkpoint of the same revision: a start entry, then one commit entry per revision |
| `texts-<n>.pack` | Report and sidecar texts by SHA-256, compressed in groups |
| `.lock` | Taken by the one process that writes the store |

**Frames.** Every log and pack is a sequence of frames: a 4-byte length, a 4-byte CRC-32 of the payload, then the payload (little-endian). A reader stops at the first frame whose bytes are not all there yet (a write in progress); a complete frame with a wrong checksum is damage, which `verify` reports.

**Entries** are JSON:
- **Start:** `{"type":"start","revision","catalog_sha256","follows":"init"|"commit"|"gap","gap":{from,to,reason}?}`. `commit` means the checkpoint continues the previous segment. `gap` means revisions between the two could not be reconstructed (an import publication or layout migration, which leave no receipt).
- **Commit:** the receipt's facts with texts replaced by their hashes:
  - `revision`, `at_unix`, `directed_by`;
  - `before_sha256` and `after_sha256` (catalog fingerprints);
  - the header `before` and `after`;
  - the changed `nodes` and `dependencies` (positions and records, before and after, as receipts hold them);
  - `reports`: for each record whose report changed, its old and new path and text hash;
  - `moves`, `dependency_edits` (the receipt's `dependencies` field), `ledger` and `repin_ledger`;
  - `sidecars` (path and text hashes).

**Texts.** A pack frame holds an index (`[{sha256, offset, length}]`) and one zlib stream of the texts it lists, so similar reports compress together. A text is stored once whatever its number of revisions.

**Segments and checkpoints.** A new segment (checkpoint plus log) starts when the active log passes 4 MB, after a gap, and at init. Old segments stay: they are the history. A segment is published by writing its checkpoint (temporary name, flushed, renamed), then its log's start entry; a log without a complete start entry, or a checkpoint without a log, is not part of the store and is removed by the next writer.

### Following (sync)

`sync` brings the store to the catalog's current revision:

1. **Snapshot.** Under the project's shared lock it reads `project.json` and notes the current revision, then releases the lock. Completed receipts never change afterwards (journal compaction keeps their facts), so the rest runs without holding up writers.
2. **Walk back.** From the current catalog it applies receipts in reverse, newest first, checking:
   - each receipt is a completed delta for its revision;
   - its after-fingerprint is the one expected;
   - every record it removes is at the stated position with the stated content.

   It stops where the chain reaches the store's head (same revision and fingerprint), or at a revision it cannot cross (no receipt, another format, a check failing).
3. **Verify.** Before a reconstructed catalog becomes a checkpoint, its SHA-256 must equal the fingerprint the receipts recorded for it. A mismatch moves the start back to the last verified point.
4. **Write.**
   - Texts first (packs flushed), then checkpoints, then log entries (appended and flushed).
   - If the walk connected, entries extend the active segment; otherwise a new segment starts with a gap entry naming the revisions that could not be reconstructed.

`init` is the same walk with no head: it goes back as far as the receipts allow and writes a checkpoint about every 4 MB of entries. `3-3-5a` has an unbroken chain from revision 7 to the present.

**Who runs it.**
- `research store sync` runs it on demand.
- The coordinator runs it on its writer thread when the project's queue has been idle, for projects with a store, so the store stays current without anyone asking.
- Direct-mode and GUI commits are picked up by the next sync.

### Reading

- Readers take no lock: they read frames up to the last complete one.
- `research store status <project>`:
  - head revision and fingerprint, catalog revision, revisions behind;
  - segments, gaps, sizes and text count.
- `research store verify <project>`:
  - every frame's checksum;
  - every checkpoint's fingerprint;
  - each segment replayed from its checkpoint, with each commit's before-fingerprint chained to the previous after-fingerprint, and each segment's end matched to the next segment's checkpoint;
  - every referenced text present and matching its hash;
  - the head against the current catalog.
- `research store history <project> <id>`: the revisions that changed a record or its report. For each: time, directed-by, which fields changed, the report hash before and after, and moves.
- `research store show <project> <id> --revision N`: the record as it was at revision N, and its report text at N when the store holds it. If the report never changed in the store's history, the answer says the current file holds it.
- `research store catalog <project> --revision N [--out file]`: the whole catalog at revision N, rebuilt from the nearest checkpoint and checked against the recorded fingerprint.

Every answer names its source (`"backend":"store"`), as the side-by-side rule requires.

### Costs

| | `3-3-5a` |
|---|---|
| Receipts today | 277 MB on disk, about 226 MB of JSON, of which 143 MB are report texts repeated before and after |
| Store estimate | Log about 40 MB (record changes), checkpoints about 1.3 MB each every 4 MB of log, texts compressed in groups |
| Follow cost | Parsing the 10.6 MB catalog (about 100 ms) and the new receipts; nothing on the commit path |

### Not in this stage

- **Ledger annotations** stay in the tools' ledger until promotion (user decision).
- **Artifacts, address spaces and data in files** wait for the backend's model; data items use the image's RVA until then.
- **Promotion, the commit-path interface, and exporting `project.json` from the store** come at promotion.
- **Repacking** small text packs written by frequent syncs, and **deleting receipts** the store has absorbed, are later maintenance once the store has proved reliable.

## Acceptance criteria

**Data**
- Data items can be created, edited, moved, completed, flagged, claimed, quarantined and deleted like functions, in the CLI and the GUI, with evidence required to complete.
- An address used by any record is refused for a new function or data item.
- Prerequisites between functions and data are accepted in every direction.
- A project without data keeps `layout_version` 5 and an unchanged catalog hash. The first data item raises it to 6, and this build reads both.
- `status` shows data counts apart from function counts.

**Store**
- `store init` on a copy of `3-3-5a` imports the history back to revision 7. `store verify` passes, and `store catalog --revision N` reproduces the recorded fingerprint for sampled N.
- After commits through the coordinator, direct mode and an import, `store sync` brings the store to the current revision. An import publication (no receipt) appears as a gap, and the next commits connect again.
- A store damaged at its tail (a torn frame) is repaired by the next sync. Damage elsewhere is reported by `verify` and never changes the files.
- Commits take no longer with a store present (nothing on the commit path).
- Readers work while a sync appends.
