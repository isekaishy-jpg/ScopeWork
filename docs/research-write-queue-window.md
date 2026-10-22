# Research: the write-queue window is half see-through

Status: research, 2026-10-22. TODO item "Write-queue window is half see-through".

## Report

The user reported that the lower half of the Write queue window is see-through.

## Reproduction

A throwaway change to `examples/capture_panels.rs` opened the Write queue over the Project history window and captured it, light and dark, against the fixture home. The change was reverted. In the capture, the window's face colour ends just below the "Waiting" group, about 210 pixels into the window. The window border continues to the window's full reserved height of 300 pixels, and the history window shows through the band between them.

## Cause

`classic::Window::show` (`src/classic.rs`) does two things that disagree for one kind of window:

- The background is the `egui::Frame` fill of the window. For a window that is **not resizable**, egui sizes the window to its content, so the fill covers only the content (plus margins).
- The classic double bevel is drawn around `ui.max_rect().union(ui.min_rect())`. `max_rect` is the space reserved for the window, which for a non-resizable window is its `default_size` (here 520×300), not its content.

When the content is shorter than `default_size`, the bevel encloses an area the fill never paints.

## Scope

| Window | Kind | Affected |
|---|---|---|
| Write queue (`records_ui.rs`) | Non-modal, **not resizable**, `default_size` 520×300 | **Yes**: reproduced |
| Research records, Project history | Non-modal, resizable | No: a resizable window's frame covers its full size, so fill and bevel agree |
| About, Mark complete, New project | Modal | No: a modal sizes itself to its content (captured: frame and fill agree) |

Only the write-queue window is non-modal and not resizable today, but the defect lives in the shared window code, so any future window of that kind would inherit it. The queue window's content also changes height as operations run and finish, so the gap varies.

## Options for the design step

1. Draw the bevel around the content rectangle (`min_rect`) for windows that are not resizable, so the frame hugs what is painted.
2. Paint the face colour over the same rectangle the bevel encloses, before the content.
3. Make the write-queue window resizable or give it a fixed height, which would hide the defect for that one window only.

Options 1 and 2 fix it at the source. A capture of the write-queue window, empty and with running and waiting operations, is the check; the existing capture example can gain the queue shot permanently.
