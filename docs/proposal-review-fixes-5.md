# Proposal: fixes from the review of the fourth review's fixes

Status: implemented 2026-10-27. Research is the review itself: a high-effort read of `378ca59..16fad2a`, the fixes from [the fourth round](proposal-review-fixes-4.md). It reported eight findings, and the user asked for all eight to be fixed. Numbers follow the review's ranking.

## 1. Re-pooling a function covered by its chunk escapes the pool (high)

**Problem.** `pool_records` guards only records that sit directly in a pool. A function covered through its pooled chunk has no record of its own. A non-member can add it to a new pool, join that pool, and change the function, because a direct record takes precedence over the chunk's.

**Design.** Every record that is covered, directly or through its chunk, is guarded. Giving it an entry of its own is a change to a pooled record.

## 2. Import publication skips the pool guard (medium)

**Problem.** An import checks pools when it reserves, then publishes through its own `Publication` journal, which the journal guard never sees. Pooling a chunk does not change the structural epoch, so a chunk pooled between reservation and publication receives new records from a non-member.

**Design.**
- The reservation keeps the batch's `--directed-by`.
- Publication checks the parents of the records it creates against the pools, with that direction, before it commits.
- A refusal leaves the import in the same state as any other publication conflict.

## 3. No-op sidecar writes create files outside a transaction (low)

**Problem.** `sidecar_text` writes a missing sidecar's empty form before the caller knows whether anything changes. When nothing does, the file appears with no revision bump, and the coordinator records it as drift. The same happens when clearing a flag that does not exist, or restating the default proof policy.

**Design.**
- Reading a sidecar never writes: a missing one reads as its empty form. That covers `sidecar_text` and the claims reader.
- The commit's apply step replaces existing sidecar files. So `commit_or_stage` writes a missing sidecar's empty form only after the pool guard has passed, right before the commit that changes it.

## 4. A repeated quarantine migration commits nothing new (low)

**Design.** When `quarantine.json` holds no records, migration commits nothing and answers `migrated: []`. A file whose records name only deleted nodes is still emptied, since that is a change.

## 5. The coordinator's queue hides direct-mode lock holders (low)

**Problem.** With a coordinator running, a job started with `SCOPEWORK_COORDINATOR=off` holds resource lock files that coordinated jobs wait for. The coordinator's queue view shows neither.

**Design.** The coordinator's project queue answer adds `resources` and `shared_resources` from the lock files, as the direct-mode answer does, and the dashboard already reads them.

## 6. Every queue poll scans the locks folder twice per project (low)

**Design.** One scan of `.project-locks` gathers the resource locks of every scope it is asked for: the project's own and the shared ones.

## 7. Batches check pools twice (cleanup)

**Problem.** `apply_batch` runs an early `check_pool` and then the journal guard on the same set. Each parses `pools.json` and maps every node's parent.

**Design.** The early check goes, and the journal guard is the only one.
- Before its commit, a batch only computes in memory; the only file it writes is the review-queue bootstrap right before committing.
- A refused batch therefore costs only that in-memory work, and leaves nothing behind.
- The guard checks `changes` and the batch's direction before anything is written.

## 8. The dashboard cannot pool a chunk-covered function on its own (UX)

**Design.** For a function covered through its chunk, the Research pool group names the chunk and offers **Put in pool**, so the human can give the function a pool of its own.

## Acceptance

- A non-member's `pool add` of a function covered by its chunk is refused with `pooled`.
- An import whose parent chunk was pooled after its reservation fails to publish for a non-member and publishes with `--directed-by`.
- A no-op flag clear or proof policy leaves no new sidecar file and no drift.
- A second migration commits nothing.
- The coordinator's queue answer carries `resources` and `shared_resources`.
- The full suite, Clippy and fmt are clean.
