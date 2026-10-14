# Research Dashboard

A compact Rust desktop dashboard for binary research, with a separate tab for Stock and Forever.

Each project uses one classic Task Manager-style page: small Tahoma text, grey beveled tabs and framing, and a tightly stacked bank of thin horizontal black meters. Each meter represents one function chunk, with green for done, red for in progress, and grey for not done. Chunk function counts sit beside the meters; architecture totals appear once per group. The bottom strip shows project totals and the current or selected chunk count. Hover or click a meter to inspect it.

The current sample page fits 24 chunk meters in a single 800 x 600 window.

All current categories, counts, and statuses are labeled sample data. The research inventory, deliverables, and acceptance rules will be defined later. Existing research claims have not been imported.

## Run

```powershell
cargo run --release
```

Windows executable: `target/release/research-dashboard.exe`.

## Research boundaries

Keep Stock and Forever evidence and acceptance records separate. Pin real inventories to exact binary identities. Groups can reference shared functions, but totals must count unique functions. Completion must eventually be supported by agreed deliverables and evidence, not function counts alone. Preserve an unclassified category until binary evidence supports classification.
