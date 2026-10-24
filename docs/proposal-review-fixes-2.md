# Proposal: fixes from the second 2026-10-24 code review

Status: implemented 2026-10-24. Research is the review itself: a second full read of `src` by two independent reviewers, after the [first review's fixes](proposal-review-fixes.md). Five findings were reproduced on throwaway research homes. The user asked for every finding to be fixed, including those cut from the ranked list. Two of the findings are regressions from the first round, both in the dashboard's refresh rewrite.

Numbers follow the ranked review; the cut findings come after.

## 1. The binary identity or project name can break every report

**Problem.** Every report's generated header renders the project name and binary identity ahead of the `<!-- research-content -->` marker that `parts()` splits on. `project create` and `project set-binary` check them only for emptiness. Reproduced: one `set-binary` whose value contained a newline and the marker committed, after which every record answered `malformed_report`. `set-binary` itself was then refused too, because it drafts every report first, so only hand edits could recover the project. This is the class the first round closed for record names (`record_name`).

**Design.** The project name (in `create`) and the binary identity (in `create` and `set-binary`) follow the same rule as record names: one line, with no report section markers. Every live project's name and identity is already a single line, so nothing that exists today is refused.

## 2. The refresh records a project's signature after reading it

**Problem.** `snapshot_in` computes the signature it stores for the next poll from disk at the end of the refresh, after reading or reusing each project. Meanwhile the catalog it holds was read earlier. A commit landing during a refresh (seconds for `forever`) is already reflected in the stored signature, so the next polls reuse the pre-commit catalog until the project changes again. The coordinator's own `Server::load` stamps before reading for exactly this reason.

**Design.** The stored signature is the one computed at the start of the refresh, before anything is read. When a project read anew turns out to have a different set of completed records, the next refresh reads it once more and then settles.

## 3. `impl move` loops forever on a cyclic tree

**Problem.** The first round guarded node paths and roll-ups against a hand-edited cyclic `tree.json`, but not the ancestor walk in `impl move`. Reproduced: the command never returns. Through the coordinator, that walk runs on the project's writer while it holds the project lock, so every later write to the project hangs.

**Design.** The walk stops at a node it has already seen, and the move is refused with "the tree has a cycle".

## 4. The reopen walk changes quarantined records

**Problem.** `reopen_affected` reopens complete records reached through parents and prerequisite consumers, and never consults quarantine. Reproduced: complete F depends on complete G; quarantine F; save G. F's stored status becomes In Progress and its acceptance is cleared. Its effective status is still Complete, but after the quarantine is lifted F is In Progress, and the human's attestation is gone. The quarantine proposal states the opposite: a quarantined record's status is unchanged by editing other records, and lifting restores the stored status. A text edit inside a quarantined chunk whose stored status is Complete also reopens that chunk and its completed architecture.

**Design.** The walk takes the set of quarantined records:
- it never reopens a quarantined record and never continues past one, since a quarantined record's effective status cannot change;
- from a quarantined record that was itself edited, it reaches only its prerequisite consumers, whose stamps of its text did change, and not its parent.

Every caller passes the set: report saves, prerequisite changes, batches, removals and import publication.

## 5. Deleting a chunk repaths its functions' ledger rows to a path that does not exist

**Problem.** Removal archives every removed record flat, at `deleted-nodes/revision-N/<id>`. `ledger::repath` uses the first explicit mapping that matches, in list order, so a function under a removed chunk takes the chunk's mapping. Reproduced: the function's row became `revision-4/2/00001000/research.md`, while its report is at `revision-4/3/research.md`. The link rewriter for reports already uses the longest matching folder and gets this right.

**Design.** `repath` resolves explicit mappings by the longest matching folder, as `Rewriter::remap` does.

## 6. A busy or unreadable project makes the dashboard reload everything every 2 s

**Problem.** While any project is busy or unreadable, every refresh publishes a full snapshot even when it read nothing new. Research records then marks itself changed and reloads the open record, which closes the Mark complete confirmation and clears the status message. The dashboard clears the meter selection and reloads the tracker. An unreadable folder makes this permanent, and the "could not be read" message is wiped by the very next record load.

**Design.**
- A refresh that read nothing new answers "unchanged", while still asking the next poll to look again for the busy or unreadable project.
- The unreadable-project note reaches Research records with every answer, and a record load restores it instead of clearing it while the problem lasts.

## 7. The tracker and History views fail at once while a project is busy

**Problem.** The Implementation view and the History window read the project with the GUI's zero lock wait, unlike record loads, which wait 30 s. While an agent holds the lock they show "is busy". The tracker keeps showing it until the next commit, because it reloads only when the catalog changes.

**Design.** Both read with the same 30 s wait as record loads, on their background threads.

## 8. Import publication reopens records without queuing them, and history misses it

**Problem.** Publication marks the import's parents changed and reopens the complete records they reach, but writes no review-queue entries. `node create` of the same function would queue them. History also never shows a publication: it has no receipt among the catalog transactions, only the job's own receipt.

**Design.**
- Publication adds the reopened records to `review-queue.json` as one more file of its journaled publication, verified and applied by recovery like its parent edits.
- `history show` lists each committed import, read from its job receipt: "import J published, N records" at its revision, filtered by node within the created range.

## 9. `node repair-links` reopens every record it touches

**Problem.** A legacy link repair rewrites only links, yet every record whose report it changes counts as structural. Acceptance is cleared, and because nothing counts as mechanical, the ledger's report pins are left on the old texts.

**Design.** Link repair is mechanical, like rename citations: the records keep their completion, and their acceptance and ledger pins follow the new text. Real structural changes in the same batch are still structural.

## 10. `set-binary` with the current value still commits

**Problem.** The "unchanged" check sits only in the amendment branch. Before any function exists, `set-binary` with the current value bumps the revision, the structural epoch and every node's revision, and rewrites every report.

**Design.** An unchanged identity returns without a commit in both branches. The identity is compared exactly as given, as before; it is not trimmed, because batches compare it byte for byte.

## 11. Rename citations can overflow the VA

**Problem.** The citation VA is computed as `image base + RVA` without an overflow check, for an image base of up to 16 hex digits. It panics in debug builds and wraps in release.

**Design.** Checked addition: a VA that cannot be represented gives no citation pattern, and renames still count bare mentions.

## 12. A write command on a mistyped project leaves a lock file

**Problem.** The first round stopped read-only opens of a missing project from taking a lock. Writable opens still take it before checking that the project exists.

**Design.**
- A writable open of a project without a catalog returns no project and takes no lock.
- Only project creation, which must serialize two creations of the same ID, takes the lock of a project that does not exist yet.
- `project create`, `init` and the GUI's New project no longer open, and recover, every project in the home just to create one.

## 13. An empty pending folder looks like a transaction in flight

**Problem.** The coordinator and drift detection treat an existing `catalog-transactions-v4/pending` folder as a transaction in flight. Recovery and the next commit look for `pending/journal.json`, and a commit reuses an empty folder. If a journal write fails after the folder was made, every coordinator read takes the locked path, the cached writer catalog is never reused, and drift judging stops until the next commit.

**Design.** Both test for the journal file, the same rule as recovery.

## 14. After a writer panic the coordinator can publish an old catalog under a new stamp

**Problem.** After a panic, the writer reads the committed catalog and publishes it with a stamp taken afterwards, once the unwind has released the lock. A direct-mode commit in between gives a snapshot whose stamp matches disk while its catalog is older.

**Design.** The stamp is taken before the catalog is read and published with it. A commit in between then makes the snapshot look stale, never current.

## 15. `wait --idle` probes for readers with an exclusive lock

**Problem.** `wait --idle` polls `lock_status` four times a second. Each poll probes for shared readers by taking the project lock exclusively for a moment, which can make a `--wait 0` caller or the dashboard's refresh answer busy. `wait` uses only `writer_active`.

**Design.** `wait` uses the status without the reader probe, as the queue view already does. Only `research locks` still reports readers.

## Cut findings, all fixed

- **`reset-status` and quarantine.** `node reset-status` changed quarantined records' stored status, so a later lift restored Not Started instead of the research state. Explicit IDs of quarantined records are refused, like completion and flags, and `all_in_progress` passes over them.
- **Fair queue.** When a waiter lost the race to lock its own new ticket, it fell back to plain polling and could overtake queued writers. It now takes a new ticket, up to three times, before falling back.
- **Old citation index.** The v3 citation index left `report-references-v2.json` behind in every project. The old file is removed when the new one is written.
- **Seal checks.** The seal check re-read and re-parsed `quarantine.json` once per record it checked. It reads it once.
- **Rename mentions.** The bare-mention count consumed the delimiter after each match, so adjacent mentions undercounted (`Foo Foo` counted 1). A new name containing the old one as a word (`Foo` to `Foo Bar`) was reported as an unrewritten mention. Matches no longer consume their neighbours, and occurrences of the new name are set aside before counting the old one.
- **Publication retries.** Each retried publication left a `candidate-<uuid>.json` behind. A retry removes its candidate.
- **Dead code.** The `journal` arm of the coordinator's command executor could never run, and `Library::refresh` was used only by a test. The arm says so (`unreachable!`, like `import`), and `refresh` is removed with its test rewritten against `Library::open`.

## Measured and left unchanged: the coordinator's per-read catalog clone

Every coordinator read clones the snapshot's catalog into a library view. On the live catalogs, a release build clones `forever` (150,818 nodes, 42 MB) in a median 13.5 ms, against 101 ms to parse it, and 3-3-5a (31,514 nodes) in 3.4 ms. Sharing the catalog would mean holding `Arc<Project>` in `Library`, whose operations mutate `projects[p]` throughout. That is a large change for a cost the reads' own index building already exceeds. It is left as it is, and belongs with the storage backend design, which replaces the in-memory catalog anyway.

## Tests

- Identity and name: a binary or project name with a newline or a section marker is refused by `create` and `set-binary`.
- Refresh: a project committed to between the start of a refresh and its end is read again on the next poll. A refresh that reads nothing answers unchanged while a project is busy.
- Tracker: `impl move` on a cyclic tree is refused at once.
- Quarantine: editing a prerequisite of a quarantined complete record leaves its stored status Complete, and lifting the quarantine restores Complete. An edit inside a stored-Complete quarantined chunk leaves the chunk and its architecture complete.
- Ledger: deleting a chunk repaths its function's row to the function's archive folder.
- Import: a publication that reopens a complete parent queues it for review, and history lists the publication.
- Repair links: completion and acceptance are kept, and ledger pins follow.
- `set-binary` with the current value commits nothing.
- Units: VA overflow, mention counting, a writable open of a missing project leaving no lock file, the empty pending folder, `reset-status` with quarantined records, and the old index file being removed.
