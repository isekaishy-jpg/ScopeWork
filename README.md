# Research Dashboard

A compact Rust desktop dashboard for binary research, with a separate tab for Stock and Forever.

The entire application uses classic Task Manager styling: custom blue title bar and window buttons, grey beveled window frame, File/Options/View/Help menus, research tabs, recessed panels, small Tahoma text, and a three-part status strip. Drag the title bar to move the window; double-click it or use the maximize button to maximize. The lower-right grip resizes it.

Each meter contains its own chunk records. Segments within a single meter can be a mix of bright green (complete), yellow (in progress), and dim red (not complete). Each segment represents one chunk. Labels beneath each meter separately show that meter's completed/total chunks and completed/total functions. Function counts do not determine segment colors or counts. The existing 24-meter layout and headers are retained; sample meters currently contain 12 illustrative chunks each.

Dark mode is the default: charcoal surfaces, soft light text, subdued borders, and the same colored meter segments. Options > Dark mode switches between modern dark and classic light palettes; the choice is saved locally on normal exit. Menus also provide Exit, Always on top, Clear chunk selection, and About.

All current categories, counts, and statuses are labeled sample data. The research inventory, deliverables, and acceptance rules will be defined later. Existing research claims have not been imported.

## Run

```powershell
cargo run --release
```

Windows executable: `target/release/research-dashboard.exe`.

## Research boundaries

Keep Stock and Forever evidence and acceptance records separate. Pin real inventories to exact binary identities. Groups can reference shared functions, but totals must count unique functions. Completion must eventually be supported by agreed deliverables and evidence, not function counts alone. Preserve an unclassified category until binary evidence supports classification.
