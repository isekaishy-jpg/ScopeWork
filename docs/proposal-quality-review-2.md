# Proposal: app-wide code-quality review, round 2

Status: implemented 2026-10-27. Round 2 re-reads what [round 1](proposal-quality-review-1.md) changed and reads the modules round 1 covered only by search: paths, version, build identity, tidy, crash log, settings, coordination, the catalog index, the ledger, history, the record panel and the coordinator's manifest and leases. Fixes stay the obvious ones.

## Findings and fixes

1. **A doc comment displaced by round 1's bug fixes.** In `coordination.rs`, `ensure`'s doc now sits above `empty_text`, so two unrelated docs are stacked and `ensure` has none. `current_or_empty` and `ensure` also write the empty sidecar with `write_map` instead of `empty_text`. The docs go back to their functions, and both use `empty_text`.
2. **The history day range is parsed twice.** `history show --from/--to` in the CLI and the history window both turn a `--to` day into the next day's start (`+ 86_400`). `history::day_range` does it once.
3. **The report markers are spelled out in six places.** Rendering, the body/evidence split and the checks that keep markers out of report text and headers all repeat `<!-- research-content -->`, `<!-- research-evidence -->` and the `<!-- research-` prefix. Three constants in `catalog_index.rs` name them.
4. **The 1 MB lease payload limit is repeated.** The CLI repeats the limit and its message (`1 << 20`) beside `leases::PAYLOAD_BYTES`; it uses the constant.
5. **History kinds and tool names are validated twice, differently.** `manifest::valid_name` says it checks "lowercase words joined by hyphens, as for history kinds". `append_history` checks a looser rule that also accepts `-x` and `a--b`, contrary to its own error message. It uses `valid_name`, so a kind is what its message says.

Reviewed and left as they are:
- the dashboard's and the record panel's different status wording ("Not done" and "Not started" are deliberate);
- the date conversions in `crash_log.rs` and `history.rs`, which are inverses, not copies;
- long functions, where splitting them is not an obvious fix.

## Acceptance

- No behaviour change, except that a history kind with a leading, trailing or doubled hyphen is refused, as its message already says.
- The full suite, Clippy and fmt are clean.
