# Research: Scopework storage backend

Status: research, 2026-10-22. TODO item "Storage backend". Builds on [research-storage-scoping.md](research-storage-scoping.md) (prior art) and [research-storage-inventory.md](research-storage-inventory.md) (inventory, interface, gaps, model check, rollout). This document adds how Scopework commits and who touches the files today, measurements at scale, the data model in detail, the mechanics to choose between, and the side-by-side, migration and coexistence requirements. It decides nothing; the design step does.

## 1. Decisions already made (user)

- A **backend behind an interface** that the existing frontends route onto; not an off-the-shelf engine.
- **Reports stay Markdown files.** The backend records their hash; history keeps their texts.
- **Record `data/` folders stay untracked working space:** not hashed, logged or stored.
- **Ghidra and the disassembly are derived** from stored inputs and are not stored.
- **Side by side:** agents use both backends in real work; today's files stay authoritative until the user promotes the new backend; disagreements are filed as complaints.

## 2. How Scopework commits today (the guarantees to keep)

From `catalog_batch.rs` (`commit`, `finish`, `resume`) and `research.rs` (`atomic`):

1. **Commit point:** a pending journal (`catalog-transactions-v4/pending/journal.json`) is written with `atomic` (temp file, `sync_all`, rename). It holds:
   - the catalog delta;
   - every edited report's full text before and after;
   - moves;
   - sidecar texts before and after;
   - the records whose prerequisite views change;
   - ledger instructions (re-path, re-pin).
2. **Apply** (`finish`):
   - Checks that every report and sidecar on disk equals its recorded before or after text, so replay is idempotent.
   - Moves folders in two phases through a staging area.
   - Writes each report and sidecar with `atomic`.
   - Re-pins and re-paths the project tools' ledger, and rewrites the prerequisite views.
   - Writes the whole `project.json` last.
3. **Receipt:** the pending folder is renamed `revision-N`, and its journal is rewritten as a compact change record with `at_unix`.
4. **Recovery:** on open, a pending journal is rolled forward through the same apply step if the catalog equals its recorded before or after state; otherwise the project is blocked with a message.
5. **Retries:** renames retry Windows sharing violations; an apply that keeps failing returns `commit_pending` after 20 s.

Guarantees to preserve: atomic commits, crash recovery by roll-forward, idempotent replay, a definite outcome per commit, one receipt per revision, and snapshot reads at a revision.

## 3. Who reads and writes, and how

| Process | Reads | Writes | Path |
|---|---|---|---|
| Coordinator (one per research home) | Catalog held in memory; answers CLI reads at the last committed revision | Merges record operations into batches; commits | `coordinator/` |
| CLI in direct mode (`SCOPEWORK_COORDINATOR=off`, or none running) | Opens project files | Commits under the project lock | `Library::open_project` |
| GUI | Opens project files directly (`Library::open_project` / `read_project`), in its own process | Commits directly under the project lock (`ResearchUi::action`) | `research.rs`, `records_ui.rs` |
| Project tools (14 of the WoW tools) | **Read `project.json` directly** (`load_project`), and report files | Through the CLI; the ledger directly under a lease | `wow-335a-rust/tools` |
| Older Scopework builds | Project files | Commits under the legacy exclusive gate | lock compatibility in `research.rs` |

So a backend must serve **several processes at once**: one writer at a time, chosen by the project lock (coordinator, direct CLI or GUI), and readers in other processes reading consistent snapshots while a write is in progress. While the files are authoritative, the tools' direct reads of `project.json` keep working. Promotion requires either moving those reads to the CLI or keeping `project.json` exported.

## 4. Measurements

| Measure | Value | Source |
|---|---|---|
| Catalog size | `forever` 40.0 MB for 150,818 records (278 bytes each); `3-3-5a` 6.1 MB for 31,606 (203 bytes); Populous 80–116 KB | `project.json` |
| Catalog parse and serialise (Python, as an upper bound) | `forever` 432 ms and 419 ms; `3-3-5a` 165 ms and 84 ms | timed |
| Catalog content | `forever`: names 1.9 MB; fixed fields (id, parent, kind, address, status, revision, 32-byte accepted hash) about 7.2 MB as binary | computed |
| Catalog compression | zlib to 4% (`forever`) and 10% (`3-3-5a`); lzma 3% and 7% | measured |
| Commit write cost on this disk | Rewrite 6 MB, flush and rename: **25.7 ms**; append 600 bytes and flush: **2.8 ms** | timed, 20 and 200 runs |
| Live write mix (`3-3-5a`) | 1,519 commits, each one operation; 4,131 reads, 89% single-function | coordinator statistics |
| History (`3-3-5a`) | 3,319 receipts, 30.1 MB after about a day and a half; 70% report texts, half duplicates | receipts |
| History compression | 21% one receipt at a time; **3%** compressed together | measured |
| Report texts (old project, full research) | Median 3.1 KB; one file at a time 44%; with a 32 KB preset dictionary 30%; **all together 9%**; as separate files they occupy 149% of their size in 4 KB clusters | 2,000 sampled |

Conclusions the numbers support:
- Appending is about **9× cheaper per commit** than today's whole-catalog rewrite at `3-3-5a` size. At `forever` size the rewrite costs roughly 170 ms per commit.
- The data is highly repetitive: catalogs and history compress to 3–10% as a whole, but only to 20–45% item by item. **Compressing in groups (segments) matters more than the choice of codec.**
- A dictionary helps small items (44% to 30% for reports), but grouping helps far more (9%).

## 5. Data model (entities, as the research found them)

| Entity | Fields (found so far) | Cardinality (`3-3-5a`, `forever`) | Notes |
|---|---|---|---|
| Project | id, name, research scope, revision | 1 | Binary identity text is replaced by artifacts |
| Artifact (file) | path or role, SHA-256, format, origin (disk image, dump variant), size | 1 (WoW, Forever) to several (Populous) | Gap 1 |
| Address space | name, processor, mapping rule (image base, bank formula, load address), artifact | 1 (WoW) to several (SNES) | Gap 2 |
| Item | id, kind (App, Architecture, Chunk, Function, Data), parent, order, name, address (space plus offset) or file offset, extent (data), status, revision, accepted report hash, context-only, report path | 31,606 to 150,818, plus data (about 20,000 in WoW) | Data nests (87 cases) |
| Item annotations | function: signature, no-return, plate comment; data: definition (type, count, length), data type, label, plate comment | about 70,000 rows in WoW today | Today in the tools' ledger |
| Type | name, kind (struct, enum), size or width, fields or values, description | 992 | Shared across items |
| Prerequisite edge | dependent item, prerequisite item, accepted stamp | 1,297 | Extend to data |
| Record state | review flag, claim, review-queue entry, quarantine | small | Today's sidecars |
| Implementation | tree nodes, placements, implementation-only items, completion marks, repository | up to all items | Owned by one project or several (open) |
| Group (home) | name, members, order | few | Home-level |
| Cross-project reference | from item or project to another project's item | few | Open (gap 4) |
| Analysis inputs | foundation profile files (analyzer policy, decoding repairs, no-op spans, boundary closures, flow and structure stages), Ghidra version | about 70 files | Needed to rebuild Ghidra |
| Log entry | revision, time, who, directed-by, changes (field diffs, edges, state), report hash changes | one per commit | Replaces receipts |
| Text store | report texts (and other texts) by hash | as history needs | Content-addressed |
| Tooling entry | kind, summary, nodes, refs, diff | small | Today's `tooling.jsonl` |

## 6. Mechanics to choose between

1. **Catalog and state as a log with checkpoints (Bitcask, SQLite WAL):**
   - A commit appends framed records (field changes) and a commit record, then flushes (about 3 ms measured).
   - A checkpoint writes the full state as one compressed binary snapshot from time to time.
   - Start-up loads the checkpoint and replays the log after it.
   - The coordinator's in-memory catalog is the index.
   - Readers in other processes read the checkpoint plus the log up to the last complete commit record (their end mark), so a write in progress never disturbs a reader. Appending also avoids depending on whether a file replace is atomic on Windows.
2. **Framing (LevelDB log):** fixed blocks, records with CRC32C, length and type. Readers skip unknown record types (older builds stay compatible) and resynchronise after damage.
3. **Texts by content hash (Git, Fossil):** report texts in history are stored once under the hash of their uncompressed bytes, which is already the identity Scopework uses for acceptance and the ledger uses for pins. They are packed into segments compressed together (9% measured for reports, 3% for receipts).
4. **Encoding:** JSON lines inside the framed records keep the store readable by hand; a binary encoding is about 4× smaller before compression (7–9 MB against 40 MB for `forever`). Compression in groups narrows that difference; the design weighs readability against size.
5. **Compression:** zlib is available everywhere and measured above; zstd is the stronger general choice (not measured here: no library installed). Either way, per-segment compression with an optional dictionary for small items.
6. **Concurrency:** one writer at a time under the existing project lock (coordinator, direct CLI or GUI). Readers open files with sharing that allows appends. A checkpoint is written as a new file and published by an append to the log, not by replacing a file readers hold.

## 7. Side by side (user's rollout rule, mechanics)

- **Dual write at the commit point:** the same transaction that `finish` applies to the files is appended to the new backend. A failure in the new backend never fails the operation (the files are authoritative); it is reported, and filed as a complaint.
- **Reads from both:** agents use the new backend for reads, including features only it offers (data items, cross-references, dated history queries). Every answer names its backend.
- **Comparison:** a check compares the two stores at a revision and lists differences; agents file complaints from real use.
- **Promotion and exit:** the user promotes the new backend after a stretch of reliable use. `project.json` (and the views tools read) can be exported from it, which also keeps the tools working until they move to the CLI.

## 8. Migration and ingestion

- **Catalog, sidecars and history:** imported from today's files. History has 72,000 receipts in the old project and 3,319 in `3-3-5a`; old receipts may hold whole sidecar copies, already compacted in the old project.
- **Ledger rows** become item annotations. The name conflicts must be resolved, not copied: 3,342 in `3-3-5a`, growing as agents work. During side by side the tools still write the ledger directly under a lease; the coordinator already detects such outside writes as drift, which gives the point where the backend ingests ledger changes.
- **Artifacts and address spaces** cannot be parsed reliably from today's free-text binary identity (Populous). Each project needs them entered once, with the text kept as a description.
- **Analysis inputs** are registered by path and hash.

## 9. Coexistence

- Older Scopework builds must not corrupt a project that has a new backend beside its files. During side by side they keep writing the files (authoritative), and the new backend catches up from the receipts they leave (one per revision). After promotion, a version gate refuses older builds.
- Direct mode and the GUI write without the coordinator, so the dual write must happen in the shared commit path, not only in the coordinator.

## 10. Questions for the design step

1. Log framing and record encoding: JSON lines or binary inside the frames.
2. Checkpoint policy: by log size, by time, at coordinator shutdown.
3. Which history to import: all receipts, or full history only for `3-3-5a` and a summary of the old project.
4. How name conflicts are resolved, and by whom (a list for agents, an import rule, or user decisions).
5. When the backend owns the annotations: from the start of side by side (ingesting the tools' ledger writes), or only at promotion.
6. Data nesting: child items or type fields.
7. Whether an implementation belongs to one project or spans several, and how cross-project references are stored (the home level or the projects).
8. Verifying that Ghidra rebuilds identically from the stored inputs (a project-tools task the backend depends on).
9. Windows specifics to test: append with readers holding the file, flush cost on other disks, antivirus scanning of growing log files.
