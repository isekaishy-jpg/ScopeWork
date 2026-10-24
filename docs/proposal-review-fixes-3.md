# Proposal: fixes from the third 2026-10-24 code review

Status: implemented 2026-10-24. Research is the review itself: a third read of `src` by two independent reviewers, after the [first](proposal-review-fixes.md) and [second](proposal-review-fixes-2.md) rounds. Of 15 reported findings, the user asked for the 10 real bugs to be fixed. The other five are policy questions or unproven timing, listed at the end and left as they are.

Two of the ten are gaps in the second round's own fixes: `node reopen` (round 2 sealed `reset-status` but not reopen) and the unknown-project guard (placed after six commands that return earlier). Most of the rest come from two invariants that are enforced separately at each call site: "a change reopens only what depends on it" and "a quarantined record keeps its stored status and structure". The fixes put each invariant in one place where possible, instead of patching one more caller.

Numbers follow the severity ranking given to the user.

## 1. An unchanged or evidence-only save reopens a completed record (high)

**Problem.** `save_reports` skips a report whose rendered text is unchanged only when the record is not Complete. A Complete record with identical text goes through `mark_changed` and is reopened, with its acceptance cleared. That includes `evidence_only` saves, whose contract is to attach evidence without changing status. A tool that re-saves evidence over finished functions reopens every one of them, and nothing is queued. The Complete exception existed because `node reopen` and the GUI's Reopen button are implemented as a save of the current text.

**Design.** An unchanged report is never a change: `save_reports` skips it whatever its status. Reopening becomes an explicit operation, `Library::reopen`, used by `node reopen` and the GUI's Reopen button. It checks the draft token and passes an internal `reopen` flag, so the unchanged text still reopens that one record. `node save` with the current text therefore leaves a completed record complete. Its help text says so.

## 2. Editing a context-only record reopens its completed ancestors (high)

**Problem.** The reopen walk follows the parent edge from every changed record. A parent's completion ignores its context-only children (they are inventory, not required research), and its report does not change when a child's text does. Yet a note saved on a context-only function reopens its chunk, the architecture and the App, clearing the human acceptance of each synthesis.

**Design.** From a context-only record, the walk reaches its prerequisite consumers but not its current parent, the same rule the walk already uses for quarantined records. Structural changes are unaffected: a move or a membership change rewrites the parent's report, and the batch marks that parent changed itself.

## 3. An unreadable `quarantine.json` silently lifts every quarantine (medium)

**Problem.** Every seal and status check reads the quarantine file with "unreadable means none". A truncated file, or one in a newer format, unseals every record: prerequisite changes, flags, claims and completion on a cyber-stop record are accepted without direction. `node batch` and `node delete` happen to fail, but with a parse error rather than a seal message.

**Design.** Writes fail closed and reads keep working:
- Every catalog commit that goes through `commit_or_stage` (reports, prerequisites, flags, claims, quarantine changes, project fields, tracker writes) first parses the quarantine file. A file that exists but cannot be read refuses the write with `quarantine_unreadable`, naming the file and the parse error.
- `check_seal` reads the file the same way, so seal refusals name the real problem.
- Import publication reads it the same way.
- Reads still treat an unreadable file as no quarantine, so the dashboard and `node show` keep working while the file is repaired by hand.

## 4. `node reopen` changes a quarantined record's stored status (medium)

**Problem.** `reset-status`, completion, flags and claims refuse quarantined records, but reopen does not. Reopening a complete quarantined record clears its stored Complete and its acceptance, so lifting the quarantine later restores In Progress instead of the human's attestation.

**Design.** `Library::reopen` (section 1) refuses a quarantined record, like `reset-status`. Saving changed text into a quarantined record stays allowed (report text is editable); changed text invalidates the old acceptance anyway.

## 5. Deleting a quarantined record's prerequisite changes its prerequisites without direction (medium)

**Problem.** Removal checks the seal on the removed records only, yet drops every prerequisite edge touching them. Deleting a function that a quarantined record depends on changes the sealed record's own prerequisites, which the seal rule says needs `--directed-by`. The receipt records no direction.

**Design.** Removal also checks the seal on every surviving record that loses a prerequisite edge ("Removing a prerequisite of").

## 6. `deps set --directed-by` is not recorded in history (low)

**Problem.** The quarantine proposal says a directed change stores the human's instruction in the journal and history. Batches and removals do; `deps set` builds its journal without it.

**Design.** `set_dependencies_directed` passes its direction to the journal it commits, so `history show` lists it.

## 7. Every save in Research records re-reads every project (low)

**Problem.** After each GUI action the dashboard clears both the refresh fingerprint and every project's signature, so the next refresh re-reads, re-parses and re-meters every project in the home (42 MB for `forever`), undoing the per-project refresh for the most common path.

**Design.** After an action, only the fingerprint is cleared: the action's commit changes the project's own signature, so only that project is read again. The explicit **Refresh** menu item still re-reads everything.

## 8. Several commands skip the unknown-project check (low)

**Problem.** The second round's guard runs after `journal`, `wait`, `locks`, `queue`, `job status`, `tools audit` and the `import` commands, which return earlier. `journal typo` leaves `typo.lock` (and `typo.holder` with `--compact`) and fails with "path not found"; `wait typo --idle` answers success at once; the read-only ones answer as if an empty project existed. `import start typo` takes the missing project's lock.

**Design.** The CLI resolves the project a command names once, at the start of `run`, and answers "Unknown project" before any command runs. Journal maintenance also builds its lock holder before reading the catalog, so a catalog it cannot read no longer leaves the holder file behind.

## 9. A long prerequisite chain overflows the stack when the project opens (low)

**Problem.** Dependency validation runs on every open and every `deps set`. Its cycle check recurses once per prerequisite and scans every edge for each record it visits. A chain of about 30,000 functions overflows the stack, so no command can open the project, recovery included; many edges make every open quadratic.

**Design.** The check builds a map from each function to its prerequisites once and walks it iteratively with an explicit stack. Same answers, linear time, no recursion.

## 10. The CLI panics when its output pipe closes early (low)

**Problem.** Answers are printed with `println!`, which panics when the reader has gone (`research ... | head -c 300`). The command's work is already done, but the panic prints a backtrace note and the exit code is 101.

**Design.** Answers are written to stdout without panicking; a closed pipe ends the output quietly. The exit code stays the command's own.

## Not changed: policy questions and unproven timing

- Reordering an architecture's chunks moves a quarantined chunk among its siblings without direction. Whether a sibling order counts as the sealed record's structure is a policy question for the user.
- A directed import or `node create` into a quarantined chunk marks the chunk changed, which reopens its stored status. Whether a directed membership change should keep the sealed chunk's status is a policy question.
- An import checks the seal when it reserves, not again when it publishes. A chunk quarantined in between receives the import's records. Rare; a policy question about what a reservation promises.
- Publication writes `review-queue.json` just before the catalog, outside a catalog journal, so a drift poll in between could record false drift. The race is plausible but unreproduced.
- A blank `--directed-by` flag overrides a valid `directed_by` in the request file and is then refused. That is bad input, not a bug.

## Tests

- Saves: an identical save, and an identical evidence-only save, of a completed record keep it complete; `node reopen` with the current text still reopens it.
- Context-only: a draft saved on a context-only function leaves its completed chunk, architecture and App complete.
- Quarantine: a truncated `quarantine.json` refuses a report save, `deps set` and completion with `quarantine_unreadable`, while `node show` still answers; `node reopen` of a quarantined record is refused and its stored status is unchanged; deleting a sealed record's prerequisite is refused without direction and accepted with it; a directed `deps set` lists its direction in history.
- CLI: `journal`, `wait`, `locks`, `queue`, `job status`, `tools audit` and `import start` on an unknown project answer "Unknown project" and leave no lock or holder file.
- Units: dependency validation of a 50,000-function chain answers without overflowing, and still finds cycles; the GUI's after-action refresh keeps unchanged projects' signatures.
