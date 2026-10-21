# Proposal: four CLI papercuts

Status: proposed 2026-10-21 (research and design below; implementation follows on the maintenance branch).

Four small complaints from 2026-10-19, each with a narrow fix. They share one proposal because none needs a format change beyond an optional request flag.

## 1. `ready_for_review` counts never-started records

**Research.** `status` (cli.rs) marks a record ready when it is not Complete, not flagged, has no incomplete required children or prerequisites, and is a function or has children. A function with no report at all is therefore "ready for review". On `populous-amiga` all 332 freshly enrolled functions were ready; on `forever` the count equals the whole inventory. The existing test `status_and_list_filters` encodes that behaviour.

**Design.** "Ready for review" means there is written research to review. Add one condition: the record's effective status is not NotStarted (status becomes InProgress once a body or evidence is saved). Parents whose children are all complete but whose own report is still empty are not ready for review; they are ready for synthesis. Report them separately as `ready_for_synthesis` (per record and as a summary count), so an agent looking for synthesis work still finds them. `--ready` keeps meaning ready for review; a new `--ready-for-synthesis` filter lists the others.

**Acceptance.** A fresh enrolment reports `ready_for_review` 0. A function with a saved report and no blockers is ready. A chunk with all children complete and an empty report is `ready_for_synthesis`, not ready for review. Flags still exclude both.

## 2. `node prune` previews zero candidates without a reason

**Research.** `remove_nodes` (catalog_remove.rs) silently skips a node when it is not an architecture or chunk, is in `keep`, falls outside `ids`, fails the `context_only` or `name_prefix` filter, still has children, has authored body or evidence, or has extra files in its folder. The preview only lists candidates, so an empty answer gives no clue.

**Design.** The preview and the applied result gain `excluded`: for every ID listed in `ids` (and, when `ids` is empty, for at most 50 skipped architectures and chunks), the rule that excluded it: `kind`, `keep`, `context-filter`, `name-prefix`, `has-children` (with the remaining child IDs), `authored-content` (body or evidence), or `extra-files` (with up to five file names). No change to which nodes are removed.

**Acceptance.** Requesting an ID that has authored content returns it in `excluded` with `authored-content`; a node with remaining children lists them; a successful prune lists no excluded requested IDs.

## 3. `project set-binary` takes no revision precondition

**Research.** Every other metadata write is token- or revision-checked (`set-scope` takes `--expect-revision`), but `set-binary` writes unconditionally, so a concurrent identity edit can be overwritten.

**Design.** Add `--expect-revision N` to `set-binary`, checked against the project revision exactly as `set-scope` does, with the same error. It stays optional for compatibility; the CLI guide recommends it.

**Acceptance.** A stale revision is refused and nothing changes; the current revision succeeds.

## 4. Binary identity cannot be amended after enrolment

**Research.** `set_binary` refuses once any function exists, because the identity defines what every RVA means. The identity is free text such as `WoW 3.3.5a build 12340; SHA-256 aa63…; PE32 x86; image base 0x00400000`. Changing it re-renders the header of every report (the `Binary:` line). The current code then clears every accepted hash and reopens every completed record, which is correct for a new binary and wrong for a wording fix. Layout migration already shows the safe pattern: rewrite reports mechanically, and move an accepted hash (and prerequisite stamps) from the old text to the new text when the old one was accepted.

**Design.** With functions present, `set-binary` accepts the new identity only as an **amendment**: every SHA-256 (64 hex digits) and every hexadecimal base or mapping value (`0x…`) in the old identity must appear unchanged in the new one; new text and new mappings may be added. An amendment re-renders every report header, re-accepts each accepted hash and prerequisite stamp whose old value matched the original text, re-pins schema-1 annotation ledger rows (`report_sha256`) the same way, and records the old and new identity in the journal and history. Statuses and completion stay as they were. An identity that drops or changes a pinned value is still refused with the existing message, extended to name the missing value. `--expect-revision` (item 3) is required for amendments.

**Acceptance.** Adding a mapping or rewording keeps every Complete record Complete and every report's authored text byte-identical; changing a SHA-256 or base is refused naming it; the journal recovers an interrupted amendment.

## Tests

Extend `tests/cli_small_answers.rs` (status readiness, set-binary precondition) and add focused tests for prune exclusion reasons and identity amendment in small temporary projects. Run build, Clippy with `-D warnings` and the full test suite.

## Remaining and future work

None of the four changes the catalog format. Prune exclusions could later feed the GUI's organize view.
