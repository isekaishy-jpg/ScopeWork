# Research: data as a record kind

Status: research, 2026-10-22. TODO item "Data as a record kind".

## Request

Data items (globals, tables, strings, vtables) become units like functions: an address, a name, a report, status and prerequisites, placed in the chunk they belong to. The prerequisite system links code to data.

## What exists today

Records have four kinds in a strict hierarchy: App, Architecture, Chunk, Function (`Kind::child` in `research.rs`). 48 code sites in 13 files test for `Kind::Function`. The ones that carry meaning:

| Behaviour | Where | Today |
|---|---|---|
| Hierarchy | `Kind::child` | A chunk's children are functions |
| Address required and unique | `research.rs` validation, `catalog_batch.rs` and `imports.rs` creation | Every function has an RVA, unique among functions; other kinds have none |
| Record folder name | `research.rs`, `layout.rs`, `catalog_index.rs` | A function's folder is its RVA (`0041A2B0/`) |
| Leaf status | `Library::effective` | A function's stored status is its status; parents derive theirs from children |
| Completion rules | `Library::save` and complete | A function needs evidence; a non-function needs every required child complete |
| Completion criteria text | `Kind::completion_criteria` | One text per kind |
| Prerequisites | `validate_dependencies`, `deps set` | Function to function only, by RVA |
| Counts | `cli.rs` status and summary, `imports.rs`, dashboard meters | Functions counted as the inventory and coverage |
| Data folder | `edit_tokens.rs`, `records_ui.rs`, `catalog_batch.rs` | Functions only |
| Binary identity lock | `research.rs`, `records_ui.rs` | Locked once any function exists |
| Quarantine | `quarantine.rs` | Chunks and functions |
| Coordinator read class | `coordinator/mod.rs` | `function`, `chunk`, `project` |
| CLI kind filter | `cli.rs` | `function`, `chunk`, `architecture`, `app` |
| Ledger re-pathing on moves | `ledger.rs` | Rows follow their report's record |

## What the data looks like (`3-3-5a` ledger)

| Measure | Value |
|---|---|
| Distinct addresses with data annotations, outside function code | 20,252 |
| With an explicit extent (data definition or data type) | 16,994; lengths from 1 to 163,840 bytes, median 8 |
| Definition shape | `{"type": "f32", "count": 5, "length": 20}`; data types `{"type": "BlizzardHashMap", "length": 12}` |
| Label only, no extent yet | 3,141 (1,808 in `.data`, 1,333 in `.rdata`) |
| By section | `.rdata` 9,103, `.data` 7,665, `.text` 225 (tables and constants outside function bodies), `.zdata` 1 |
| Nested items | 87 extents start inside a larger one (for example a 104-byte item at `0x5E8258` holding a separately defined item at `0x5E825C`) |
| Names | Labels, many generated from the chunk and address (`CRT_Process_initialization_and_termination_g_009E0ACC`) |

Other projects:
- The Populous data disks (`populous-final-frontier-amiga`, `populous-promised-lands-amiga`) are made only of data files (landscapes, sprites, levels) and currently hold nothing but an empty App report.
- The SNES ROM mixes code, graphics and tables in one image.

Data therefore also has to be addressable inside **files**, not only inside a loaded image. That ties this item to the storage research's files and address spaces.

## Requirements found

1. **A leaf kind beside functions:** chunks hold functions and data items; Data has its own completion criteria text.
2. **Address and extent:** a data item has a start address and a byte length. Functions take their extent from the inventory; data records carry it.
3. **Identity rule:** unique start address among data items and no data start inside a function body. Containment must be allowed (87 nested items), so "no overlap" cannot be the rule. A contained item is either a child of the containing item or a field of its type; the design must choose.
4. **Prerequisites across kinds:** function to data and data to data, where today only function to function is allowed.
5. **Same lifecycle as functions:** stored status, report, evidence required for completion, data folder, quarantine, flags, claims.
6. **Separate counts:** data counted beside functions (done of total), not merged into the function inventory and coverage totals.
7. **Address spaces and files:** data in a loaded image, data in files on a disk image, data in a second processor's space. This depends on the storage backend's model of files and address spaces.
8. **Creation in bulk:** the ledger's 20,252 addresses and the byte accounting's 122,035 spans are the candidate inventory. Which spans become items (a string table as one item or many) is a research decision per project, not automatic.
9. **Annotations become fields:** a data item's definition (type, count, length), its data type, label and plate comment belong to it, as function annotations belong to functions.

## Open questions for the design step

- Nested data: child records, type fields, or both?
- Folder naming: functions use the RVA; a data item at the same address scheme is safe only with the uniqueness rule above (no data start inside a function). Files and other address spaces need a prefix.
- Whether the 225 `.text` data items stay data or are treated as parts of the functions that own them.
- How the GUI shows a chunk's two kinds of children, and how the dashboard's meters count them.
