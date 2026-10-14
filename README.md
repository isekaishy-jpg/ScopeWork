# Research Dashboard

A compact Rust desktop dashboard for binary research, with a separate tab for Stock and Forever.

The entire application uses classic Task Manager styling: custom blue title bar and window buttons, grey beveled window frame, File/Options/View/Help menus, research tabs, recessed panels, small Tahoma text, and a three-part status strip. Drag the title bar to move the window; double-click it or use the maximize button to maximize. The lower-right grip resizes it.

Each project page has one bar per research item. **Each pip is one research chunk**, independently colored green (done), red (in progress), or grey (not done). Pips have equal size regardless of their function counts. Empty bar space represents no chunks, not unfinished work. Click or hover an individual pip to inspect that chunk.

Every bar has its research-item header above it, and chunk completion, total functions, and the current/selected chunk's function count below it. The sample has six research items containing four chunks each. Project function counts and chunk counts are separate; no function percentage determines meter fill.

Dark mode is the default: charcoal surfaces, soft light text, subdued borders, and the same colored meter segments. Options > Dark mode switches between modern dark and classic light palettes; the choice is saved locally on normal exit. Menus also provide Exit, Always on top, Clear chunk selection, and About.

All current categories, counts, and statuses are labeled sample data. The research inventory, deliverables, and acceptance rules will be defined later. Existing research claims have not been imported.

## Run

```powershell
cargo run --release
```

Windows executable: `target/release/research-dashboard.exe`.

## Research boundaries

Keep Stock and Forever evidence and acceptance records separate. Pin real inventories to exact binary identities. Groups can reference shared functions, but totals must count unique functions. Completion must eventually be supported by agreed deliverables and evidence, not function counts alone. Preserve an unclassified category until binary evidence supports classification.
