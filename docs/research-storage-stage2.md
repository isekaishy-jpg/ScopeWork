# Research: the storage backend's remaining stages

Status: research, 2026-10-28. TODO item "Storage backend". The first stage (data records, the side-by-side store, history reads) is done: [design-storage-backend.md](design-storage-backend.md), [proposal-storage-backend.md](proposal-storage-backend.md). This document covers what is left before the user can promote the store: the annotation ledger, artifacts and address spaces, the acceptance scope, the commit-path interface with promotion and export, and store maintenance. It decides nothing; the design step does.

## 1. Decisions that bind these stages (user)

From [research-storage-backend.md §1 and §10](research-storage-backend.md#answers-and-triage-2026-10-25):

- A backend behind an interface the existing frontends route onto. Reports stay Markdown files; record `data/` folders stay untracked; Ghidra and the disassembly are derived and not stored.
- Side by side: today's files stay authoritative until **the user promotes** the new backend. Agents use both; disagreements are filed as complaints.
- **Annotations:** the backend takes over the annotations **at promotion**, as part of the manual migration. Until then the tools' ledger stays authoritative.
- **Conflicts:** name and other data conflicts must be resolved before the backend is adopted. The backend reports them; it needs no rule of its own for settling them.
- **Migration:** carrying a project over is a manual step the user runs. Which history comes along is decided at that step.
- **Projects:** one project per implementation; cross-project references wait.
- **Acceptance** covers the research body and evidence, not the generated header. The one-time conversion converts only acceptances whose current full-report hash still verifies, records every old and new hash, and converts the ledger's report pins with the same mapping, **in one journaled transaction**. Renames, moves and amendments stay in the history so the name, place and identity a report was accepted under can be reconstructed.

Not decided by the user, and so open for design: the shape of artifacts and address spaces, whether the store follows the ledger before promotion, what promotion writes, and how the tools reach the annotations afterwards.

## 2. The annotation ledger today (`3-3-5a`, measured read only)

| Measure | 2026-10-22 | 2026-10-28 |
|---|---|---|
| File | 49.9 MB | 53.8 MB, zlib 3.6 MB (6.6%) in 0.6 s |
| Rows | 71,252 | 78,513, average 578 bytes, unique by `(rva, kind)` |
| Kinds | | function-name 19,341; function-signature 19,310; data-definition 16,391; plate-comment 15,415; label 7,093; data-type 932; function-noreturn 29; data-retire 2 |
| Types | 992 | 1,038 (fields, size, description, report pin) |
| Data addresses | | 20,365 distinct (definitions, labels, data types) |
| Function-name conflicts with the catalog | 3,342 | **10**; no ledger name lacks a record |

- **Shape:** `{schema: 1, image_base, source_sha256, annotations: [...], types: [...]}`. Each row: `kind`, `rva` (hex text), `value`, `report`, `report_sha256`, `reason`, `classification`, `reviewed`, plus migration notes (`catalog_move`, `organization_migration`).
- **Writers:** the project tools rewrite the whole file under a coordinator lease (`annotations.py`, `function_research.py`, `typed_signature.py`, `move_functions.py`, `revert_authored.py`). Scopework rewrites it inside `finish` to re-path and re-pin rows (`ledger.rs`), as a recorded sidecar. Drift detection (`coordinator/drift.rs`) records outside writes; it never blocks them.
- **History:** none outside Scopework's own sidecar records. A tool write leaves no record of what changed.
- **Pins:** the tools compute `report_sha256` as the SHA-256 of the **whole report file** and treat a mismatch as stale (`annotations.py` lines 71, 194, 306; `function_research.py` 220, 263; `stale_queue.py` 205). They re-pin rows themselves after their own saves.

Findings:
- The key `(rva, kind)` is unique, so a row-level change record per ledger write is exact: rows added, removed and changed, each a small entry, with texts stored once.
- The agents have resolved almost all name conflicts. Conflicts are cheap to compute on every sync (one pass over 19,341 names).
- **The acceptance conversion reaches the tools.** Converting ledger pins to body hashes while the tools still hash whole files would mark every row stale in their checks. The conversion therefore belongs where Scopework owns the pins (promotion), and the tools must take pins from Scopework from then on (`research` output) rather than hashing files. That is a change to the agents' tools, which the user directs.

## 3. Artifacts and address spaces

Today a project has one free-text `binary` identity, and every leaf has one `rva`. The identities in the research home:

| Project | Inputs | Addresses |
|---|---|---|
| `3-3-5a` | One PE32 image | RVA from image base `0x00400000` |
| `populous-amiga` | An ADF disk; the main code file (`DAD`) inside it; a loader | RVA = offset in the absolute image loaded at `$0005E7C4` |
| `populous-editor-amiga` | An ADF; a hunk executable | RVA = offset in hunk 0 CODE |
| `populous-final-frontier-amiga`, `populous-promised-lands-amiga` | An ADF data disk; **no program of their own**: land, sprite and level files loaded by the base game | None yet: their research is data inside files |
| `populous-snes` | A LoROM ROM | RVA = ROM file offset; VA by a bank formula; two CPUs (65C816 and an SPC700 sound processor) |

Findings:
- **Every input is already named with its SHA-256 in the identity text.** Artifacts can be entered once per project (a manual step, research-storage-backend §8) with role, path inside its container, SHA-256, size and format, while the text stays as the description.
- **An address needs a space.** Two data disks need data at an offset inside a named file; the SNES project needs a second CPU's address space. Today's `rva` remains the address in the project's default space, so existing records, report paths and the ledger keep their meaning.
- **Identity per space:** the current rule (an address names at most one leaf) holds within a space; the same number in two spaces is two places.
- **Folders:** leaf folders are `<RVA:08X>` (functions) and `D<RVA:08X>` (data). A record in another space needs the space in its folder name, or two records with the same offset in one chunk would collide.
- **Mapping rules** (image base, bank formula, load address) are descriptive today: nothing in Scopework computes VAs except the rename citation form (VA = RVA + image base, parsed from the identity text). Storing the image base as a field would replace that parse.
- **Format:** as with data, a catalog that uses spaces or artifacts needs a format that older builds refuse; catalogs without them keep their bytes.

## 4. The acceptance scope

- `accepted_hash` is the SHA-256 of the whole rendered report (generated header plus body and evidence), read in 32 places across 11 modules (`catalog_index`, `catalog_batch`, `research`, `reopen`, `edit_tokens`, `imports`, `layout`, `ledger`, `history`, `pools`, `catalog_browser`). Prerequisite stamps (`revision:sha256`) use the same hash.
- The header is rendered by `catalog_index::Index::render`; the body follows `<!-- research-content -->` and the evidence `<!-- research-evidence -->` (`CONTENT_MARKER`, `EVIDENCE_MARKER`). Draft tokens (`d4`) already hash only the authored sections, so the split exists.
- What a body hash ends: re-pinning after renames, moves and binary amendments (`repin_ledger`, the mechanical-edit handling in `catalog_batch.rs` around the acceptance carry-over, and `set-binary`'s whole-project re-render with every report's text in its journal).
- The report hash cache (`report_hashes.rs`) hashes whole files by stamp. A body hash needs the file read and split, or a second cached hash per file.
- A conversion is a whole-project transaction (31,605 reports in `3-3-5a`). Today's journal format would hold every report's text twice (as `set-binary` does); converting touches no report text, only the catalog and the ledger, so the journal holds only the catalog delta and the ledger sidecar.

## 5. Promotion: what has to change on the commit path

Today (research-storage-backend §2): the commit point is the pending journal; `finish` writes reports, sidecars, the ledger, views and `project.json`; the receipt is the renamed journal. The store follows receipts.

After promotion the store is authoritative. What that requires:
- **Commit point:** the store's log. A commit appends its texts (packs) and then its entry; the entry being complete is the commit. The files (reports, `project.json`, the ledger export) are applied afterwards, as `finish` does now.
- **Recovery:** on open, if the store's head is ahead of `project.json` (by revision and fingerprint), the files are rolled forward from the store's last entry, which holds every changed report and sidecar as text hashes resolvable from the packs. This is today's roll-forward with the store in place of the pending journal.
- **Every writer must go through it:** the coordinator, direct mode and the GUI share `catalog_batch::commit` (every production catalog transaction ends there); import publication and layout migration write on their own (research-storage-architecture §3.5). After promotion they must commit through the store too, or be refused.
- **Older builds:** they would keep writing files only, so the store would fall behind with no receipt to follow if receipts stop. A version gate is needed: after promotion the catalog carries a format older builds refuse.
- **Readers:** the coordinator's snapshot reads and direct reads load `project.json`. After promotion `project.json` is an export the store keeps current; it is exactly the bytes whose hash the store records as each entry's after-fingerprint, so a reader can keep loading it and check its hash against the store's head when it needs certainty.
- **Receipts:** the store's entries carry everything a receipt carries (with texts by hash), so receipts can stop at promotion; `history show` must then read the store.
- **Annotations:** at promotion the store takes the ledger's rows as its own state. The tools still read `annotations.json` (14 tools load files directly), so the store exports it after each change that touches annotations. Tool writes to the file after promotion would bypass the store; they need a `research` command to change annotations, or drift detection to refuse them.
- **The interface:** a trait under `Library` with two implementations (files, store), chosen per project from the catalog's format, keeps every domain method unchanged: the domain already hands storage one `Journal` per change (research-storage-architecture §8).

## 6. Store maintenance (measured in the first stage)

- `3-3-5a`'s store was 97 MB: log 78 MB of uncompressed JSON frames, checkpoints 17 MB, texts 5.6 MB.
- Closed segments never change. Compressing a closed log as one stream would cost one inflate per read of that segment; JSON receipts compressed together measured 3% (research-storage-backend §4), so a closed log of 4 MB would be about 0.1–0.3 MB.
- Frequent syncs write small text packs (one frame per sync); `TextIndex` reads every pack's index at open, so their number matters more than their size.
- Receipts can be deleted only after promotion; before it they are the authoritative history the store follows.

## 7. Questions for the design

1. Whether the store follows the ledger before promotion (history and conflicts only, the ledger staying authoritative), or meets it only at promotion.
2. The shape of artifacts and spaces, and whether they live in the catalog (usable now) or only in the store (usable after promotion).
3. Where the acceptance conversion runs: alone now, or inside promotion (where Scopework owns the ledger pins).
4. How the tools change annotations after promotion: a `research annotate` command, an import of their rewritten file, or both.
5. What promotion refuses: open conflicts, an unverified store, a store behind the catalog, a pending journal.
6. Whether a promoted project can be returned to files (an export of the current state with receipts regenerated), and who runs it.
