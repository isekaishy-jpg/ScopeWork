# Research Dashboard

A compact Rust desktop dashboard for binary research, with a separate tab for Stock and Forever.

Each project fits on one page: category names, classic segmented resource bars, and accepted/total function counts. Green means accepted, red means in progress, and grey means unreviewed. Hover or click a function group to see its name and count.

All current categories, counts, and statuses are labeled sample data. The research inventory, deliverables, and acceptance rules will be defined later. Existing research claims have not been imported.

## Run

```powershell
cargo run --release
```

Windows executable: `target/release/research-dashboard.exe`.

## Research boundaries

Keep Stock and Forever evidence and acceptance records separate. Pin real inventories to exact binary identities. Groups can reference shared functions, but totals must count unique functions. Completion must eventually be supported by agreed deliverables and evidence, not function counts alone. Preserve an unclassified category until binary evidence supports classification.
