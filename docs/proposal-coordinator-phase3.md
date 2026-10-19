# Proposal: coordinator phase 3, storage

Phase 3 of [the coordinator proposal](proposal-coordinator.md): what the coordinator should change about how the catalog is stored and how derived state is computed, now that one process owns every write. Research and proposal are complete; nothing here is implemented. Two parts are gated on measurements that only a released phase 1 can produce. The gates are stated as acceptance prerequisites, not left open.

## Research

### Where a commit's time goes now

Measured on a copy of the live catalog (31,603 records, 7.8 MB compact) in phase 0 research and after phase 0:

| Step | Time | Paid |
| --- | --- | --- |
| Open: parse 21 ms, validate, build the index 30 ms | about 55 ms | Once per batch: the writer re-opens the project from disk for every batch |
| Journal delta, including SHA-256 of the before and after catalog (28 ms each) and the before-catalog serialisation (9 ms) | about 112 ms | Once per commit |
| Write and fsync `project.json` | 26–27 ms | Once per commit |
| Report and sidecar writes | per file | Per changed file |

Findings:
1. **Rewriting the whole catalog is the smallest fixed part:** 27 ms of roughly 200 ms. Incremental catalog writes would save at most that.
2. **The two catalog fingerprints cost about 65 ms**, a third of the fixed cost. The before fingerprint still needs a serialisation, because catalogs written by older versions are pretty-printed (phase 0 deviation).
3. **Re-opening per batch costs about 55 ms** plus a lock round. Yet the coordinator already holds the exact catalog in memory as its snapshot whenever no outside process has committed since.
4. **Merging (phase 1) already divides all of this by the batch size**, and 93% of live commits are single completions arriving about a second apart. How much fixed cost remains per operation depends on the batch sizes phase 1 achieves on the live load. Phase 1 does not yet log them.

### Where a read's time goes

`status`, `node list` and the other effective-status reads evaluate effective status for the whole catalog. Each one stats every accepted report and rehashes the changed ones (0.8.0 qualification: warm `status` about 0.43 s on the live catalog). Through the coordinator this repeats for every read, against the same snapshot.

Findings:
5. **Effective status is a pure function of the snapshot and the report hashes.** Within one snapshot, only report files changed outside Scopework can change it. Saves change reports only through commits, which create a new snapshot.
6. **How often such reads happen is unknown.** Phase 1's read log now counts them (the `project` class). No released build has collected data yet.

### Storage format options weighed

| Option | Saves | Compatibility (C5) |
| --- | --- | --- |
| A. Append-only delta log beside `project.json`, checkpointed periodically | Up to 27 ms per commit | Breaks: 0.8.0 and earlier, and direct mode in older versions, read a stale `project.json` |
| B. Catalog split into shards (for example per architecture) | Write and parse cost proportional to the shard | Breaks every older reader and the journal format |
| C. Keep the format; cut the fixed costs around it | Up to about 120 ms per commit (open, fingerprints) | Unchanged on disk; journal compatibility is handled below |

**Decision: C.** A and B trade correctness for older versions (C1, C5) for at most 27 ms per commit. The research shows the time lies elsewhere. The phase 1 outline's "incremental catalog writes" therefore becomes "incremental commit work". The on-disk catalog stays one compact `project.json`.

## Proposal

### 3.1 A resident catalog in the writer

The writer keeps its `Library` between batches instead of re-opening the project for each one.

- **Before each batch** it takes the project lock as today (D10 coexistence) and compares the `project.json` stamp with the one it last wrote.
  - The same stamp, and no new transaction receipt: it continues with the resident catalog.
  - Anything else: an outside commit happened. It re-opens exactly as today and publishes the result first, as phase 1 already does.
- **After a failed commit** the resident catalog is discarded. The next batch re-opens.

This saves about 55 ms per batch. It uses the stamp phase 1 already keeps, and receipt numbering already identifies outside commits.

An in-process commit does not re-check `project.json` on disk: it checks reports and sidecars against the journal, and relies on having held the lock since opening. With a resident catalog, the stamp and receipt check is therefore the guard. Every Scopework commit, from any version or process, adds a receipt, so it always catches those. The one change it can miss is a hand edit of `project.json` that keeps the same size and modification time. Two rules close that gap:
- **Racy stamps:** if the stamp's modification time is within 2 s of the writer's own last write, the writer hashes the file before trusting it.
- **Periodic re-open:** it re-opens fully at least once a minute.

### 3.2 A before fingerprint without serialising

When a catalog is opened, the coordinator records whether its bytes are compact: the bytes it read match its own serialisation. Every catalog written by phase 0 or later is compact.
- **Compact catalog:** the before fingerprint is the hash of the bytes read, or the after fingerprint of the previous commit while resident. No serialisation is needed.
- **Pretty-printed legacy catalog:** today's path. Its first commit rewrites it compactly, so this happens once per project.

Saves about 37 ms per commit (one SHA-256 pass plus a serialisation). The journal format is unchanged, so recovery and older versions are unaffected.

### 3.3 Faster fingerprints (gated)

Replace SHA-256 with XXH3-128 for catalog fingerprints, as pending-delta schema 2 (today's pending deltas are `"kind": "pending-catalog-delta", "schema": 1`). These are identity checks for crash recovery, not a security boundary (phase 1 research).
- **Saving:** about 25 ms per remaining hash pass.
- **Cost:** a 0.8.0 process that finds a schema-2 pending delta cannot recover it. It refuses with its existing "Unrecognized pending journal" error until a newer version opens the project. Completed receipts are unaffected.

**Gate:** implement only if the phase 1 commit log shows fixed commit cost above 100 ms per operation at the live median batch size after 3.1 and 3.2. Otherwise the compatibility cost is not worth it.

### 3.4 Effective-status cache per snapshot (gated)

Each snapshot caches effective status for the whole catalog. It is computed on first use and shared by every read of that snapshot.
- **Commits:** a commit's new snapshot starts from its predecessor's cache. Only the records phase 0's `reopen_affected` reports are recomputed: the changed records, their ancestors and prerequisite consumers. A snapshot loaded after an outside commit starts empty.
- **Outside report edits:** reads reuse the cached report stamps for up to 2 s. After that, the next read runs the stat pass again (parallel, about the cost of today's warm pass) and recomputes only the records whose report changed.
- **Answers:** a cached answer says `"hashes_checked_ms_ago": n`.
- **Writes never use the cache.** Completion, saves and tokens still evaluate exact hashes inside the commit, as today. A stale read can therefore never cause a wrong acceptance (C1).

**Gate:** implement when the read log shows `project`-class reads above 1 per second at peak on the live project, or when `status` latency through the coordinator exceeds 200 ms at the median. Below that, the cache saves little and adds a staleness window.

### 3.5 Commit metrics (ungated, needed by the gates)

The writer adds to `coordinator status`, and to a daily `coordinator-commits` tooling entry:
- batch size (operations per commit) as median and p90;
- fixed commit time split into open, fingerprints, delta, write and fsync;
- queue wait per class.

These make the 3.3 and 3.4 gates measurable. They also confirm or refute finding 4.

### Module plan

| Module | Change |
| --- | --- |
| `src/coordinator/mod.rs` | Resident `Library` per project writer, stamp and receipt check, commit metrics |
| `src/journal_history.rs`, `src/catalog_batch.rs` | Before fingerprint from compact bytes or the previous after fingerprint; (3.3) pending-delta schema 2 with XXH3-128, recovery reading schemas 1 and 2 |
| `src/research.rs` | Compactness flag recorded at open |
| `src/coordinator/status_cache.rs` (new, 3.4) | Snapshot cache, incremental recompute from `reopen_affected`, stamp window |

### Validation plan

Small fixtures only (at most 100 functions). Catalog-scale timing is measured in memory on a copy of the catalog, as in phase 0.
- **Resident catalog:** a direct-mode commit between two coordinator batches is detected, and the next batch starts from it. A failed commit discards the resident state. Results match re-opening for every operation in the existing coordinator tests.
- **Before fingerprint:** a pretty-printed catalog takes the legacy path once and is compact afterwards. Crash-recovery tests pass unchanged: the journal is identical byte for byte to one built with serialisation.
- **(3.3) Schema 2:** recovery of schema-1 and schema-2 pending deltas. The older refusal is checked against its error text.
- **(3.4) Cache:**
  - cached and uncached answers are equal for every read in the existing coordinator tests, before and after commits;
  - an outside report edit appears within the window;
  - writes ignore the cache: a completion after an outside edit inside the window still sees the new hash.
- **Metrics:** the commit metrics appear in `coordinator status` and in the daily entry.

### Rollout

1. **3.1, 3.2 and 3.5** ship together. They change nothing on disk.
2. **The gates are evaluated** after one week of live use with the metrics.
3. **3.3 and 3.4** each ship alone if their gate is met, each with its own changelog entry.

### Acceptance criteria

- **3.1 and 3.2:** fixed commit time per batch drops by at least 80 ms on the in-memory measurement of the live catalog copy. All coordinator and recovery tests pass.
- **3.4, if built:** median `status` latency through the coordinator under 50 ms on a warm snapshot, with answers equal to uncached ones.
- No change to `project.json`'s format. Older versions open every catalog that phase 3 writes.

### Tradeoffs

- **Storage stays monolithic.** Writes keep paying about 27 ms for the whole catalog, in exchange for every older reader staying correct.
- **The cache allows up to 2 s of staleness** for report files edited outside Scopework, on reads only.
- **The gates delay 3.3 and 3.4** until live data exists, rather than building them on assumptions.

### Risks

- **Resident-state bugs.** A missed outside change would make the writer build on an old catalog and overwrite that change. Mitigations:
  - the project lock is still taken per batch;
  - the stamp and receipt check catches every Scopework commit;
  - the racy-stamp hash and the once-a-minute re-open bound the hand-edit case;
  - phase 2's drift detection records any such edit.
- **Cache invalidation errors.** Mitigation: the cache is never used by writes, the equality tests above, and a `SCOPEWORK_STATUS_CACHE=off` switch for comparison.
