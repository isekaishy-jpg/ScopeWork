# Proposal: unambiguous commit outcomes on Windows

Status: proposed 2026-10-21.

## Problem

Agents saw catalog writes fail with `Cannot replace ...\project.json: Access is denied. (os error 5)` (claim release, node completion) while the change had in fact landed, so every caller had to read back before deciding whether to retry. Complaints: `2026-10-18-audit-repairs-claim-release-access-denied.md`, `2026-10-19-agent-publish-ledger-lock-restores-old-names.md` (its ledger half belongs to the project tools and is fixed there).

## Research

- Every catalog commit (`catalog_batch::commit`) writes the pending journal first, then applies reports, sidecars and `project.json` in `finish`, then turns the journal into a receipt. Recovery rolls a pending journal forward before any other operation on the project.
- Since 2026-10-18 (`284e646`, `39c3cfd`) `atomic` replaces files through `rename_retrying`, which retries Windows access, sharing and lock violations (os errors 5, 32, 33) for 3 seconds. The complaints predate that change; the retry window is still short when a reader holds the file open for longer, for example a project tool hashing every report or a Python script reading `project.json` without delete sharing.
- When `finish` fails after the journal is written, the error string is returned as is. `error_kind` maps it to `operation_failed`, the same kind as a failure before anything was written. The caller cannot tell "nothing happened" from "recorded, will be applied".
- `coordinator_lost` already marks the one genuinely unknown case (the pipe failed after sending the request).

## Design

1. **Retry the apply step.** When `finish` fails after the pending journal is durable, retry it in-process with backoff for up to 20 seconds (the journal makes the step repeatable; recovery code already re-derives it).
2. **Report a definite outcome.** If it still fails, return `commit_pending: the change is recorded in the transaction journal and Scopework applies it before any other operation on this project (<cause>). Do not repeat the request; read the record back after the file is released.` The kind `commit_pending` means committed, application deferred.
3. **Keep the other kinds exact.** A failure before the journal is written stays `operation_failed` and means nothing was applied. `coordinator_lost` stays the only unknown outcome. The CLI guide documents all three.

## Acceptance

A commit whose final `project.json` replace is blocked briefly succeeds without an error. A replace blocked past the retry window returns `commit_pending`, and the next command applies the journal and shows the change. Errors raised before the journal is written still leave the project unchanged and say `operation_failed`.

## Tests

A focused test holds `project.json` open without delete sharing (the way a plain reader does) during a commit: released within the window, the commit succeeds; held past it, the result is `commit_pending` and the next read shows the change applied. The window is shortened for the test through a debug-only environment variable, as the existing fault-injection hooks are.

## Future work

The project tools keep their own retrying writer (`fresh_re.write_json`) and should read `project.json` through a retrying helper; that is tracked in the project's `tools/IMPROVEMENTS.md`.
