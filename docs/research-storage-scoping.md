# Research scoping: Scopework storage

Status: scoping, 2026-10-22. This decides what the storage research must study; it chooses nothing.

## Why

Projects take a lot of disk space and a lot of files: Explorer's folder properties crawl, and every small file costs a full disk cluster. JSON on disk was always going to be large. Scopework already has most of a database's machinery. The open question is the layer that puts data on disk.

## Measured (live project `3-3-5a`, about one day of work)

| What | Measured |
|---|---|
| Record folders (`architectures/`) | 125,934 files in 62,397 folders: **34 MB of data occupying about 494 MB** of 4 KB clusters |
| Per function | `research.md` (median 347 bytes), `dep.toml` and `prereq.toml` (derived dependency views, empty lists for nearly every function), a 36-byte `.scopework-import-*` marker left by the import, and an empty `data/` folder |
| Catalog | `project.json` 6.1 MB, rewritten whole on every commit |
| History | 2,623 receipts, 20.6 MB. 70% of it is whole report texts (before and after each edit); **49% of those texts are duplicates** of texts stored elsewhere in history |
| Annotation ledger (project tools) | `annotations.json` 47 MB, rewritten whole on every change. Each ledger apply snapshots the full ledger and input, about 100 MB per apply (tool side) |
| Old project `wow-335a-12340` | 83 GB logical but about 20 GB on disk, because its tools already use NTFS compression (10:1 on JSON outputs, 2.7:1 on history) |

Three of every four files per function, and half of all folders, carry no research.

## Database machinery Scopework already has

- Write-ahead journal with crash recovery and roll-forward.
- Single writer (the coordinator) with queued, merged record operations.
- Snapshot reads from the last committed revision.
- Change receipts (history) per commit.
- Indexes and caches: the in-memory catalog, report hashes, the report-reference index.

Missing: incremental writes, compact encoding, compression, and fewer files.

## Constraints found

- **Reports are an interface.** Agents and people read and edit `research.md` as Markdown. The project tools read report content through `node read-batch`, but the annotation ledger pins **report files** by hashing their bytes from disk (seven call sites in `wow-335a-rust/tools`). Packing reports away from files requires Scopework to provide report hashes (and paths) through the CLI first.
- **Windows file semantics.** Replacing files under readers produced sharing violations (fixed with retries). Fewer, larger files and in-place appends change this picture.
- **Coexistence.** Direct mode and older Scopework versions must stay correct beside the coordinator. A storage change needs a journaled one-time migration and a version gate.
- **NTFS compression** is available today, transparent and reversible. It reduces bytes but not file counts or cluster waste on tiny files.
- Scale ahead: data items as records could take a catalog from about 31,000 to 100,000+ records.

## Directions to research

1. **Stop writing what carries nothing** (no new engine): derive the dependency views on demand instead of writing two `.toml` files per record, remove import markers when the import commits, and create `data/` only when something is stored. Expected: about 75% fewer files and half the folders. Smallest step, independent of the rest.
2. **Content-addressed history** (as Git stores objects): receipts reference report texts by hash, stored once and compressed, optionally packed into pack files. Expected: history roughly halved by deduplication, then compressed.
3. **Catalog as an append-only log with checkpoints** (log-structured, as Bitcask does): commits append compact records; a periodic checkpoint bounds start-up; the coordinator's in-memory catalog is the index. Replaces whole-file rewrites.
4. **Packed reports** (segment files instead of one file per record), with Markdown exported on demand. Depends on the report-hash interface above, and on whether people need the files.
5. **Embedded engines as the alternative to building:** SQLite, redb (B-tree, pure Rust), fjall (LSM tree, pure Rust), LMDB. These need the same migration and interface work. They would replace the layer under Scopework's own journal, coordinator and history rather than add to them.

Encodings to compare: JSON (today), serde binary formats (postcard, bincode), CBOR or MessagePack, and zero-copy formats (FlatBuffers, Cap'n Proto). Compression: zstd, including trained dictionaries for many small similar texts; LZ4 for speed; NTFS transparent compression.

## Prior art for building our own

Proven designs that map onto parts of Scopework, without adopting an engine.

| Scopework part | Prior art | What carries over |
|---|---|---|
| Catalog: 31,000–100,000 small records, one writer, the whole catalog in the coordinator's memory | **Bitcask** (Riak) | An append-only data log plus an in-memory key directory mapping each key to its latest entry. *Hint files* (positions only) make start-up fast; a *merge* rewrites closed log files keeping only live entries. Its known limit, every key in RAM, is irrelevant here: the coordinator already keeps the catalog in memory, and that becomes the key directory. |
| Commit records on disk | **LevelDB log format** | Fixed 32 KB blocks of records, each with a CRC32C checksum, a length and a type (FULL, FIRST, MIDDLE, LAST) for records spanning blocks. Readers skip unknown types (forward compatibility) and, after corruption, resynchronise at the next block boundary. |
| Commit point and readers | **SQLite WAL** | A commit is the append of a commit record. The main file is untouched until a *checkpoint* folds the log back in, automatically past a size threshold or when idle. Each reader records an *end mark* (the last commit it sees), which is the snapshot read Scopework already does by revision. Fewer flushes than rewriting files. |
| One record's report revisions (history) | **Mercurial revlog** | One append-only log per item: an index (offset, compressed length, delta base, content hash) and data holding a compressed full text followed by deltas. Chain length is capped (about twice the text size) so reads stay bounded. Small revlogs store index and data in one file and split past a size threshold. |
| Many texts, deduplicated (history, report snapshots) | **Git packfiles** | Content-addressed objects (named by hash) stored once, packed with deltas against similar objects. A `.idx` gives sorted names, offsets and a CRC32 per object. Repacking is tuned by delta depth (default 50) and candidate window (default 10). |
| Identity versus storage | **Fossil** | All raw content lives in one blob store with delta and zlib compression, but **identity is the hash of the uncompressed content**: the delta format is an optimisation outside the repository's identity. For Scopework this keeps report hashes, completion tokens and the ledger's report pins valid whatever the storage format. |
| Alternative to log plus checkpoint | **LMDB** | A copy-on-write B+ tree in one memory-mapped file, with no write-ahead log and no recovery step. Freed pages are reused instead of compacted; one writer, lock-free readers. More complex, and memory-mapped file growth on Windows needs care. |
| Versions sharing structure | **Dolt and Noms (prolly trees)** | Content-addressed chunks with content-defined boundaries, so an edit changes only nearby chunks and identical subtrees are stored once across versions. Garbage collection is mark-and-sweep from the commit graph. Heavier than needed today; relevant if whole-catalog versions must be cheap to keep. |
| Compression of small items | **zstd with trained dictionaries** | Small inputs compress poorly alone. A dictionary trained on samples of one data type (reports, receipts) improves ratios dramatically, most in the first few KB. Use one dictionary per data type. Reports here have a median of 347 bytes. |
| Durable writes on Windows | Microsoft (Raymond Chen), PostgreSQL and Go discussions | `FlushFileBuffers` reliably commits to disk but is costly. `ReplaceFile`'s atomicity is undocumented and disputed; `MoveFileEx` with replace and write-through is the usual alternative. Appending and flushing avoids depending on rename atomicity at all, unlike the current whole-file replace. |

A combination these point to, for the design step to test: **a Bitcask-style catalog log** with LevelDB-style framing and SQLite-style checkpoints; **a content store for report texts and history** (revlog or pack style) addressed by the hash of the uncompressed text, as Fossil does; and **zstd with per-type dictionaries**. That keeps Scopework's journal semantics, coordinator and CLI, and replaces whole-file rewrites and per-record files. Reports can still be written out as Markdown where people need them.

Sources: [Mercurial revlog internals](https://repo.mercurial-scm.org/hg/help/internals.revlogs), [Gregory Szorc on the revlog](https://gregoryszorc.com/blog/2014/02/05/the-mercurial-revlog), [Bitcask overview (High Scalability)](https://highscalability.com/riaks-bitcask-a-log-structured-hash-table-for-fast-keyvalue/), [Bitcask explained (Arpit Bhayani)](https://arpitbhayani.me/blogs/bitcask), [Git pack format](https://code.googlesource.com/git/+/refs/tags/v2.33.2/Documentation/technical/pack-format.txt), [Fossil delta format](https://fossil-scm.org/home/doc/trunk/www/delta_format.wiki), [Fossil data model](https://www.fossil-scm.org/home/doc/tip/www/fossil-is-not-relational.md), [LevelDB log format](https://fuchsia.googlesource.com/third_party/leveldb/+/refs/tags/1.22/doc/log_format.md), [SQLite WAL](https://sqlite.org/wal.html), [LMDB](https://www.wikipedia.org/wiki/LMDB), [Dolt storage engine](https://docs.dolthub.com/architecture/storage-engine), [Dolt prolly chunker](https://dolthub.com/blog/2022-06-27-prolly-chunker), [zstd README](https://www.github.com/inikep/zstd), [FlushFileBuffers (The Old New Thing)](https://devblogs.microsoft.com/oldnewthing/20170510-00/?p=95505), [Go issue 8914 on atomic replace](https://github.com/golang/go/issues/8914), [Atomic file writes on Windows](https://antonymale.co.uk/windows-atomic-file-writes.html).

## Questions the research must answer

1. What matters most, in order: disk bytes, file count, write cost, history size, or browsing speed?
2. Must reports stay plain Markdown files that people and agents open directly? If so, directions 2 and 3 still apply and 4 does not.
3. How do the options behave on Windows under crash, sharing and antivirus scanning, and how do they recover?
4. What does each cost at 100,000+ records with code-to-data references?
5. How do existing projects migrate, and how do older Scopework versions coexist?
6. Should the project tools' own ledger (one 47 MB JSON file, plus 100 MB snapshots per apply) move onto the same storage?
