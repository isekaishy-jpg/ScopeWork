# Design: the storage backend's remaining stages

Status: design, 2026-10-28. Research: [research-storage-stage2.md](research-storage-stage2.md). Builds on the first stage ([design-storage-backend.md](design-storage-backend.md)). Everything here keeps the user's rule that today's files stay authoritative until the user promotes a project, and that promotion is a manual step the user runs.

## Scope

1. **The store follows the ledger** (history and conflicts; the ledger stays authoritative).
2. **Artifacts and address spaces** in the catalog.
3. **Store maintenance:** fewer packs, compressed closed logs.
4. **The backend interface and the promoted commit path:** the store as commit point, files as exports, recovery from the store, history from the store.
5. **Promotion:** `research store promote`, which converts acceptance to body hashes and takes over the annotations in one transaction, and `research annotate` for annotation changes afterwards.

Not here: cross-project references (user: they wait), deleting receipts before promotion, changing the agents' tools (the user directs that), and running promotion on any live project (the user's decision).

## 1. The ledger follower

- **When:** every `store sync` (and so every idle sync by the coordinator) reads `tools/profiles/annotations.json` after the catalog walk. If its SHA-256 differs from the store's last ledger fingerprint, the sync records a ledger entry.
- **Base:** the first time a store meets the ledger, the whole file is stored once as a text (by hash, in a pack; 3.6 MB compressed for `3-3-5a`) and a `ledger-base` entry names it.
- **Entries:** `{"type":"ledger","revision":<catalog revision at the sync>,"at_unix","before_sha256","after_sha256","header":{before,after}?,"rows":{"removed":[keys],"added":[rows],"changed":[{"before","after"}]},"types":{same, keyed by name}}`. A row's key is `(rva, kind)`, a type's is its name (both unique, measured). Header fields (`schema`, `image_base`, `source_sha256`) are recorded only when they change.
- **Granularity:** one entry per sync that sees a change, so several tool writes between syncs collapse into one entry. Ledger history is as fine as the syncs (the coordinator syncs whenever the project is idle).
- **Torn reads:** a ledger that does not parse (a write in progress outside the lease) is skipped for this sync, with a note in the result; the next sync retries. A parsed ledger is recorded only if its bytes hash to what was parsed.
- **Replay:** the ledger at an entry is the base plus every later ledger entry; each entry's `before_sha256` must equal the replayed state's hash, as commit entries chain catalog fingerprints. `verify` checks this chain.
- **Reads:**
  - `research store annotations <project> <rva> [--kind K]`: the rows at an address, newest first through their history (value, reason, report pin, which sync changed them).
  - `research store conflicts <project>`: disagreements between the catalog and the ledger, computed now:
    - `name`: a `function-name` row whose value differs from the function's name at that address, or a `label` row unlike the data item's name at its address;
    - `kind`: a function row (`function-*`) at a data item's address, or a data row (`data-definition`, `data-type`, `label`) at a function's address that is not the function's entry label;
    - `report`: a row whose `report` is not the current report path of the record at its address;
    - `unknown`: a row kind this build does not know.
    Rows at addresses with no record are counted (`unplaced`), not conflicts: they stay annotations on an address.
- **Status:** `store status` shows the ledger fingerprint, rows, types and when the last ledger entry was recorded.
- **Files:** ledger entries go to their own `ledger.log` (so catalog readers are unchanged), and `ledger-head.zlib` keeps the ledger as of the last entry (fast compression; rewritten per change) so a sync diffs without replaying. Fingerprints are of the canonical form (keys sorted, rows ordered by key), which a replay reproduces exactly.
- **The coordinator** follows the ledger when its stamp (size, modification time) changes, on a thread of its own: comparing the `3-3-5a` ledger takes about 2.7 s (hash 0.4 s, parse 0.7 s, previous state 0.6 s, diff 0.7 s), and no commit may wait behind it. The catalog sync stays on the idle writer thread. A sync with an unchanged ledger only hashes its bytes.

## 2. Artifacts and address spaces

Catalog fields, both written only when not empty, so catalogs without them keep their bytes:

```text
Project.artifacts: [{ id, name, sha256, size?, format?, inside?, path?, description? }]
Project.spaces:    [{ id, name, kind: "image"|"file"|"processor", artifact?, processor?, base?, description? }]
Node.space?        (absent: the project's default space, as today)
```

- **Artifact:** an input file. `inside` names the artifact it is extracted from (a file on a disk image) and `path` its path there. `sha256` is required (64 lowercase hex digits); names and SHA-256 are unique in the project.
- **Space:** where addresses are counted. `image` (the loaded image; `base` is its load address), `file` (offsets in one artifact, for data disks), `processor` (a second CPU's address space, such as the SNES sound processor). An `artifact` must exist.
- **Default space:** records without `space` are in the project's default space, which is today's: `rva` from the binary identity's image base. Nothing existing moves.
- **Identity:** an address names at most one leaf **per space**. Functions and data may be placed in any space.
- **Folders:** a leaf in space `n` uses `S<n>-<RVA:08X>` (function) or `S<n>-D<RVA:08X>` (data). Default-space folders are unchanged.
- **Format:** a catalog with an artifact, a space or a node with `space` has `layout_version` 7, raised lazily as data raises 6. Validation requires 7 when any is present. This build reads 5, 6 and 7.
- **CLI:**
  - `research project artifact add <project> --name N --sha256 H [--size B] [--format F] [--inside ID --path P] [--description D] --expect-revision R`;
  - `research project artifact update <project> <id> [fields] --expect-revision R`, `remove` (only when no space names it and no artifact is inside it);
  - `research project space add|update|remove` likewise (`remove` only when no record is in it);
  - `node create --space ID`, and `space` in batch and import `create`; a batch may move a leaf to another space only by deleting and recreating it (identity is per space);
  - `deps set` names a prerequisite in another space as `S<n>:<RVA>`; rename citations (which compute a VA from the image base) apply to the default space only;
  - `project show` lists artifacts and spaces.
- **Commits:** artifact and space changes are header changes committed through `commit_catalog`, so they leave receipts and the store follows them.
- **Reports:** a leaf's generated header names its space when not the default.
- **GUI:** the record panel shows a leaf's space; adding artifacts and spaces is a CLI task in this stage (the dashboard layout is unchanged).

## 3. Store maintenance

- `research store compact <project>` (writer lock):
  - **Packs:** all packs are rewritten into as few as possible (texts grouped in 4 MB batches, packs capped at 64 MB), written as new files and published before the old ones are deleted.
  - **Closed logs:** a segment's log is closed once a later segment exists. Compaction writes it as `segment-N.logz` (one zlib stream of the same frames) and deletes the `.log`. Readers accept either; the active segment is never compressed.
- Sync runs compaction by itself when the store holds more than 64 packs.
- `verify` checks compressed logs exactly as plain ones.

## 4. The backend interface and the promoted commit path

**Interface.** A `Backend` chosen per project from the catalog's format:

```text
trait Backend {
    commit(journal) -> outcome          // committed | commit_pending | failed
    recover(project) -> state           // roll forward an interrupted commit
    history(filter) -> rows             // the history view and `history show`
}
```

`Files` is today's code (pending journal, `finish`, receipts). `Store` is used by promoted projects. Every catalog transaction already ends in `catalog_batch::commit`, which dispatches to the project's backend; the domain is unchanged.

**Promoted format.** A promoted catalog has `layout_version` 8 and `"backend":"store"`. Older builds refuse it (they accept 6 at most), which is the gate keeping them from writing files the store would never see.

**Commit (Store backend):**
1. Build the delta as today (catalog before and after, edits, moves, sidecars, annotation changes).
2. Write the texts it references to a pack (flushed).
3. Append the commit entry to the active log and flush. **This is the commit point.**
4. Apply the files as `finish` does: reports and sidecars (each checked against its before or after text), folder moves, views, the ledger export, and `project.json` last. No pending journal and no receipt are written.
5. A new segment starts at 4 MB of log, as now.

**Recovery.** On open, if the store's head is ahead of `project.json` (head revision greater, or the catalog's hash is the head's before-fingerprint), the head entry is applied to the files again (every text resolves from the packs). If `project.json` matches neither the head's before nor its after state, the project is blocked with a message, as a mismatched pending journal is today.

**Readers** keep loading `project.json`: it is the export whose hash the head records, so the coordinator and the GUI read it as now. `store status` reports any difference.

**History.** For promoted projects `history show`, the GUI history view and `store history` read the store's entries. The rows keep today's shape (`revision`, time, operation, records, reports).

**Other writers.** Import publication commits its result through the backend (its own publication record stays in the job folder). Layout migration refuses promoted projects (they are current by construction). `project tidy` and journal maintenance refuse them (nothing to tidy: no receipts are written).

## 5. Promotion

`research store promote <project> --expect-revision R` runs under the project's exclusive lock and refuses unless:
- the store exists, `verify` passes, and its head equals the catalog's revision and fingerprint (a sync runs first);
- no pending journal, active import or import publication is in flight;
- `store conflicts` reports none (the user decision: conflicts are resolved before adoption);
- the ledger parses and its fingerprint equals the store's last ledger entry.

**One transaction** (one commit entry, the first through the Store backend):
1. **Acceptance conversion.** For every record whose `accepted_hash` equals its report's current whole-file hash, the accepted hash becomes the SHA-256 of the research body and evidence; prerequisite stamps whose hash verifies convert with the same mapping. Anything that does not verify stays as it is (and so stays stale). The entry records every old and new hash.
2. **Annotations.** The ledger's rows and types become the store's annotation state. Report pins convert with the same old-to-new mapping; pins that do not match a converted hash stay as they are (stale).
3. **Format.** `layout_version` 8, `backend: "store"`.
4. **Exports.** `project.json` and `annotations.json` are written from the result.

Before the transaction, promotion copies `project.json` and `annotations.json` to `store/promotion/` (with their hashes), and receipts stay on disk; returning a project to files is a manual recovery from those copies, not a command, in this stage.

**After promotion**
- **Acceptance** is the body-and-evidence hash: renames, moves and binary amendments no longer change it, so re-pinning becomes a no-op. The report hash cache keeps a body hash per file beside the whole-file hash, by the same stamp rule.
- **Annotations** are store state, exported to `annotations.json` after each change:
  - `research annotate set <project> --kind K --rva A --value V --report PATH --reason R [--classification C]`, which pins the report's current body hash; `research annotate remove <project> --kind K --rva A`; `research annotate stale <project>` lists rows whose pin no longer matches.
  - **Outside writes** (a tool rewriting `annotations.json`): the next commit or sync finds the file's hash differing from the last export and imports the difference as an annotation change (`directed_by: "outside write"`) before anything else. Tool writes keep working and are recorded; pins they write are taken as given.
  - The tools' own staleness checks hash whole files, so after promotion they disagree with the converted pins until they use `annotate stale` or body hashes. The user directs that tool change before promoting `3-3-5a`.

## Acceptance criteria

**Ledger follower**
- After a ledger change, `store sync` records one ledger entry with exactly the rows added, removed and changed; `store annotations` shows a row's history; `verify` replays the ledger chain.
- A ledger that does not parse is skipped without an error and recorded at the next sync.
- `store conflicts` reports name, kind, report and unknown-kind conflicts on a fixture, and none when the catalog and ledger agree.

**Artifacts and spaces**
- A catalog without them keeps its bytes and format; the first artifact, space or spaced node raises it to 7.
- The same address is accepted in two spaces and refused twice in one; spaced leaves get `S<n>-` folders.
- Removing an artifact or space in use is refused.

**Maintenance**
- After `store compact`, the store holds one pack per 64 MB of texts and compressed closed logs, and `verify`, `history`, `show` and `catalog` answer exactly as before.

**Promoted commit path**
- On a promoted fixture, saves, completions, batches, deletes, `deps set`, imports and GUI saves commit through the store: no receipts or pending journals appear, `project.json` hashes to the head's after-fingerprint, and `store verify` passes.
- A commit interrupted after its entry is appended is finished by the next open; one interrupted before it leaves the project at the previous revision.
- `history show` answers from the store for promoted projects.
- A build reading only formats up to 6 refuses the promoted catalog (tested by the format check).

**Promotion**
- Promotion refuses each condition listed above, with a message naming it.
- After promotion: completed records whose whole-file hash verified are still complete; a stale one stays stale; ledger pins follow the same mapping; a rename or move no longer re-pins anything; `annotate set|remove|stale` work; an outside rewrite of `annotations.json` is imported and recorded.
- Projects that are not promoted behave exactly as before (the existing suite passes unchanged).
