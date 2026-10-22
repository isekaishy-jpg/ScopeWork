# Research: what the storage backend holds

Status: research, 2026-10-22. Companion to [research-storage-scoping.md](research-storage-scoping.md). This defines what goes into the backend before any format is chosen.

## Framing

The work is a **storage backend behind an interface**. The frontends keep their roles and route reads and writes onto the interface instead of onto files: the CLI, the GUI, the coordinator, and the project tools through the CLI. Today's files become the first implementation of the interface; a new backend becomes the second, so both can run side by side during migration.

## Projects surveyed

| Project | What it is | Catalog |
|---|---|---|
| `3-3-5a`, `wow-335a-12340` | One PE32 x86 executable, image base `0x00400000` | 18 architectures, 905 chunks, 30,682 functions; 1,150 prerequisite edges |
| `forever` | One PE x86-64 executable, image base `0x140000000` | 25 architectures, 140 chunks, **150,652 functions** |
| `populous-amiga` | Amiga floppy image (ADF) holding a loader (`populous`), a main code image (`DAD`) loaded to `$0005E7C4`, and data files; MC68000, Aztec C | 11 architectures, 28 chunks, 445 functions; RVAs are offsets in the loaded image |
| `populous-snes` | SNES LoROM cartridge (512 KB); 65C816 main CPU plus the SPC700 sound CPU | 12 architectures, 45 chunks, 390 functions; RVA = ROM file offset, bus address derived by a LoROM bank formula |
| `populous-editor-amiga` | Amiga data disk with a hunk executable (`PopEd/editor`, likely packed) | App record only |
| `populous-final-frontier-amiga`, `populous-promised-lands-amiga` | Amiga data disks with **no program**: landscapes, sprites, levels and words loaded by the base game's `DAD` | App record only |

Record data folders hold real evidence: the SNES project stores emulator screenshots (PNG) and capture JSON beside records. The Amiga project keeps an emulator and decoded assets in its tools.

## What Scopework persists today

| Data | Stored as today | Source of truth or derived | Size (`3-3-5a`) | Written | Read |
|---|---|---|---|---|---|
| Project identity: id, name, binary identity (free text), research scope, revision, layout version, structural epoch, next id, import state | `project.json` | Source | (in catalog) | Rarely | Every operation |
| Records: id, kind, parent and order, name, RVA, status, revision, accepted report hash, accepted prerequisite stamps, context-only | `project.json` | Source | 6.1 MB with the edges | Every commit, whole file rewritten | Every operation (held in memory by the coordinator) |
| Prerequisite edges | `project.json` | Source | 1,150 edges | `deps set`, moves | Status, completion |
| Dependency views (`dep.toml`, `prereq.toml` per function) | Two files per function | Derived from the edges | 61,364 files | With edges | Rarely (people and tools) |
| Report texts: header, understanding, evidence | `research.md` per record, `app.md` | Source (header rendered) | 31,605 files, median 347 bytes, 15.5 MB | Saves, renames and moves re-render | `read-batch`, GUI, people; the project tools hash the files |
| Record data files | `data/` folder per record | Source (arbitrary files, binary included) | Empty folders here; PNGs and captures in the SNES project | Tools and people | Tools and people |
| Review flags, claims, review queue, quarantine | `review-flags.json`, `claims.json`, `review-queue.json`, `quarantine.json` | Source | Small | Per change, whole file rewritten | Status, GUI |
| History: commit receipts (changes, report edits with full texts before and after, moves, sidecars, who directed it, time) | `revision-N/journal.json` per commit | Source (audit) | 2,623 receipts, 20.6 MB; 70% report texts, half of them duplicates | Every commit | `history show`, recovery tools |
| Tooling log | `tools/workspaces/history/tooling.jsonl` | Source | Small | Tool appends, daily coordinator figures | `history show` |
| In-flight transaction | `catalog-transactions-v4/pending/journal.json` | Source while pending | Transient | Before applying a commit | Recovery |
| Import jobs | Reservation, manifest, publication and receipt files per job | Source while running | Transient | Imports | Imports |
| Report-hash cache, report-reference index | `scopework-cache/*.json` | Derived | Small | Rebuilt as needed | Completion tokens, moves |
| Locks | `.library.lock`, runner and lease lock files | Runtime | — | — | — |
| Annotation ledger (owned by the project tools) | `tools/profiles/annotations.json` | Source, **but Scopework re-paths and re-pins its rows** during moves, renames and identity amendments | 47 MB | Tools; Scopework on structural changes | Tools |
| Home settings | Registry (`HKCU\Software\ResearchDashboard`: install location, home), GUI storage (dark mode), `logs/gui.log` | Source | Tiny | Rarely | Start-up |

Coming with planned features: **Data records**, the **implementation tracker** (a free-form tree, item placements, implementation-only items, completion marks, repository link), and **project groups** (across projects).

## Interface: what the frontends call

- **Catalog reads:** `project list/show/revision`, `node show/list/read-batch/resolve/flagged/quarantined`, `deps get`, `claim list`, `status` (with filters and summary), `review-queue`, `history show/kinds`, `queue`, `locks`.
- **Catalog writes:** `project create/set-binary/set-scope`; `node create/save/save-batch/batch` (create, move, rename, reorder, context membership); `complete`, `reopen`, `reset-status`, `delete`, `prune`, `flag`, `unflag`, `quarantine`, `unquarantine`, `repair-links`; `deps set`; `claim set/release`; `history append`; `import`.
- **Maintenance and coordination:** `journal` (inspect, compact), `wait`, `session` (many commands in one process), `coordinator status/stop`, `job run/status` (tool leases), `tools audit`.
- **Measured mix** (`3-3-5a` since the coordinator last started): 1,519 commits, every one a single operation, and 4,131 reads, 89% of them single-function reads. Writes are small and frequent; whole-file rewrites make each one cost the whole catalog.

## Gaps the other projects expose

1. **One binary per project.** Populous projects are disk images holding several files: a loader, a code image, hunk executables, data files, and dump variants (cracked, damaged, a second TOSEC dump). The model has one free-text "binary identity" string. The hashes, load addresses and address formulas are written into that prose, where nothing can check them. The backend needs **artifacts**: each file with its hash, format and origin (the disk it came from, the dump variant).
2. **One address space keyed by a 32-bit RVA.** The SNES has a ROM offset with a bank mapping and two CPUs, the SPC700 code running in its own address space. The Amiga has an absolute load address and hunk offsets; the editor's code is likely packed (the code that runs is not the bytes on disk). Records need an address in a named **space**, each with its mapping rule, not one RVA per project.
3. **Projects without code.** The two data disks are entirely data files (landscapes, sprites, levels). Data records must attach to **artifacts and file offsets**, not only to addresses in a loaded image.
4. **Cross-project relations.** The data disks are loaded by `populous-amiga`'s code, and the Sol client draws on both WoW and Forever. Groups are UI only; references between projects are not modelled.
5. **Scale.** `forever` already has 150,652 functions, five times WoW, before any data records.
6. **Binary evidence.** Record data holds screenshots and emulator captures. The backend must store, hash and serve binary blobs, not only text.
7. **Structure hidden in prose.** Binary identity and scope are free text that tools parse by eye. Anything that drives behaviour (hashes, mappings, load addresses) should be structured fields.
8. **Ownership of the ledger.** Scopework already writes the project tools' ledger. The backend should either own it properly or stop touching it.

## Model checked against the data (2026-10-22)

Proposed model: one row per item (architecture, chunk, function, data) carrying its name, address, status, accepted report hash and its annotations; prerequisite edges between items; types as their own entities; reports kept as Markdown files with their hash in the row; Ghidra and the disassembly derived, not stored; record `data/` folders as untracked working space; everything else an append-only log.

| Claim | Checked | Result |
|---|---|---|
| Annotations belong to items | Every ledger row in `3-3-5a` (70,551) and `wow-335a-12340` (65,631) located against the function inventory | **Holds.** Function annotations (signature, name, no-return, 12,567 of 12,801 plate comments) sit exactly at function entries. Data annotations (16,201 data definitions, 6,894 labels, 920 data types, 234 plate comments) sit outside code and belong to Data items. No annotation sits inside a function body. |
| Types are shared entities | Ledger types | **Holds.** 956 structs and 36 enums with fields, referenced from many items. |
| Annotation provenance fits a row | Ledger row fields | Each row carries kind, address, value, reason, classification, reviewed flag and a report pin; old rows also carry migration notes. With annotations stored on the item, the provenance is the item's own report, and the separate pin goes away. |
| Ghidra is reproducible from stored inputs | `fresh_re.py analyze`, the foundation profile | **Holds by design, not verified.** The project is built fresh from the binary (hash-checked), the pinned Ghidra version 12.1.2 with fixed processor and compiler settings, the foundation profile (analyzer policy, decoding repairs, no-op spans, boundary closures, flow-repair and structure stages with hash-checked inputs), the reviewed ledger and the versioned Ghidra scripts. No record shows a rebuild compared against the existing project, and the analysis timeout (1,800 s) could make a rebuild differ. The profile files are inputs the store must keep. |
| One name per function | Catalog names against ledger `function-name` rows | **Broken today.** 3,342 of 16,310 functions in `3-3-5a` (3,595 in the old project) have different names in the catalog and the ledger, sometimes entirely different functions (`0x1B71E0`: catalog `Script_GetNumGlyphSockets`, ledger and Ghidra `Script_GetMultiCastBarOffset`). One store means one name; migration must resolve these conflicts, not copy them. |
| Prerequisites are item edges | Catalog dependencies | **Holds.** 1,297 edges in `3-3-5a`, all function to function. Data prerequisites would be new edges of the same shape. |
| Acceptance hashes live on the item | Catalog fields | **Holds.** 1,879 records carry an accepted report hash, plus 153 prerequisite stamps. |
| History is a log of diffs | Commit receipts | **Holds, but it stores whole report texts** before and after each edit (70% of history bytes, half duplicates). In the log, a report change is a hash change, and texts are stored once. |
| `data/` folders are working space | Folder contents and report links | **Holds.** 75 of 30,682 folders in `3-3-5a` hold anything (110 files); 2 of 390 in `populous-snes`; none in `populous-amiga`. Reports mention `data/` 42 times, so deleting a folder can leave a dead link in a report, which is acceptable for working material. |

## Rollout requirement (user, 2026-10-22)

The new backend runs side by side with today's files until it is proven reliable, and agents use **both at the same time in real work**, not through a background checker only:

- Every write goes to both. Agents also read from the new backend, including features only it offers, so it is exercised by real use.
- Today's files stay authoritative until the user promotes the new backend. A disagreement between the two is filed as a complaint against the new backend.
- Every answer says which backend produced it.
- Promotion is the user's decision after a stretch of reliable real use; an export back to files remains the escape hatch.

## Next

The design step defines the data model first: artifacts and address spaces, records (functions and data) addressed within them, report texts and blobs, record state, history, the implementation tracker, groups and cross-project references. Formats and the on-disk layout follow from that model, using the prior art in the scoping document.
