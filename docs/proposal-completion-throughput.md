# Proposal: completion throughput

Status: researched and decided 2026-10-21. No app change; the fix belongs to the project tools that drive completion.

## Problem

Completing the 251 members of chunk 31036 in `wow-335a-12340` took over 10 minutes, and a 76-member publish cycle over 2 minutes (TODO, 2026-10-17). The question was whether the time goes to the app (process spawns, completion tokens, report hashing, journal commits) and whether a batch completion command is needed.

## Research

Measured 2026-10-21 on the live project (31,000+ records, 22,513 reports cited by the annotation ledger) while the solo session's republish-and-complete chain was running.

- **App cost per completion.** The coordinator's commit statistics over 7,186 single-operation batches: median open 23 ms, delta 62 ms, journal 6 ms, apply 40 ms, about 130 ms in total. `node show` costs 0.14 s per call including the process spawn.
- **Wall time per member.** Receipt folder times of the last 400 commits (13.6 minutes): 330 gaps under 2 s, median 0.4 s, 132 s in total. That is `node show` plus `node complete` per member: a 251-member chunk now costs about 100 s of member completion.
- **Fixed cost per chunk.** The other 69 gaps add up to 649 s, 83% of the wall time. They are the project tools' work between members: gates (about 2.6 s per chunk: depth, hedge, contract and review checks at 0.5–0.9 s each), and whole-ledger stale scans at 17–24 s each.
- **The whole-ledger scan.** `function_research.stale_reports()` pins all 22,513 cited reports. Breakdown: `Path.resolve()` in `fresh_re.pin`'s cache key about 8.5 s (it opens every file), `stat` 4.4 s, read and SHA-256 about 4 s. The in-process cache never helps a one-shot command. One `complete.sh` run triggers it three to five times: `repin_all.py` before completion, `sync-names` before and after a rename, and `complete-chunk` before completing, where the result is never used. A republish adds another `repin_all.py`.
- **Earlier contention.** The 10-minute figure came from a period with three sessions writing at once (queue wait p90 4 s, lease hold p90 27 s, both still visible in the coordinator statistics). The long project-tool lease holds are the same whole-ledger scans, run while holding the ledger lease.

## Decision

- **App: no change now.** A batch completion command would save at most the spawn and token cost per member (about 0.3 s, under 20% of the wall time measured above) and would need a new journaled operation. Revisit if member completion becomes the dominant cost after the tools fix.
- **Project tools** (`tools/scripts`, recorded in `tools/IMPROVEMENTS.md`, audit item D11 and D12):
  1. `complete-chunk` drops its unused whole-ledger scan.
  2. `stale_reports(scope)` checks only the given reports. `sync-names` and `move` scope their before and after scans to the reports they touch: the renamed or moved records and their old and new parents.
  3. `fresh_re.pin` keys its cache by the absolute path string (no `resolve()`) and persists it in `tools/workspaces/pin-cache-v1.json` by size and modification time, the rule Scopework's own report-hash cache uses. Whole-ledger reads that remain (status, the stale queue) cost a `stat` per report instead of an open and a hash.
  4. The solo pipeline stops running `repin_all.py` (blind re-pinning, audit A2). Its own chunk is re-pinned by `complete-chunk`; foreign stale rows go to the stale-pin queue.

## Acceptance

`stale_reports()` over the whole ledger takes under 6 s on a warm cache. A scoped scan takes well under a second. A `complete.sh` run without renames makes no whole-ledger scan. The coordination checks (`verify_scopework_coordination.py`) still pass.
