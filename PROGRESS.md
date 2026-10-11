# Progress

Last updated: 2026-10-10.

## Phase 3: local-first design and whiteboard tool

Goal: grow paper-rs into a Paper / Figma / Excalidraw-class tool. Figma and Paper are studied
clean-room (behavior only, `analysis/FIGMA_NOTES.md`, `analysis/PAPER_ARCHITECTURE.md`).
Excalidraw is MIT and studied from source (`analysis/EXCALIDRAW_NOTES.md`); ported algorithms
are credited in `THIRD_PARTY.md`.

| Milestone | Status | Evidence |
|---|---|---|
| Analysis notes | ✅ done | `analysis/FIGMA_NOTES.md`, `analysis/EXCALIDRAW_NOTES.md`, fixtures in `tests/fixtures/excalidraw/` |
| P1: local projects, home screen, autosave, atomic writes, versioned format | ✅ done | `src/project/`, `src/session.rs`, `src/io/atomic.rs`, `tests/projects.rs::m1_*`, `milestone1_home.png`, `milestone1_trash.png` |
| P2: multiple pages, infinite canvas per page | ⏳ | |
| P3: Figma core | ⏳ | |
| P4: Excalidraw core | ⏳ | |
| P5: inspector, layers drag-reorder, command palette, shortcuts | ⏳ | |
| P6: import/export (PNG/SVG/PDF, .excalidraw, SVG), cross-project paste | ⏳ | |

### P1 notes

* Projects live in `~/Documents/paper-rs/<name>/` (`PAPER_RS_HOME` overrides): `project.json`
  (manifest: id, name, created/modified, ordered pages, active page), `pages/<id>.json`, and
  `thumbnail.png`. Deleting moves the folder to `.trash/` with a `trashinfo.json`, restorable from
  the Trash view.
* Every write is temp file + fsync + rename + directory fsync (`io::atomic`). Stale temp files
  from a crash are removed when the project opens.
* Formats carry `format` + `version`; loading runs a chain of `Value -> Value` migrations.
  paper-rs 0.1 `.paper-rs` files are page format v1 and migrate to v2 (tested).
* Autosave: changes are written 1 s after the first unsaved edit (bounded latency even while
  editing, never during a drag), on page switch, when leaving the project and on quit. The event
  loop sleeps until the autosave deadline, so idle CPU stays at zero. Thumbnails refresh at most
  every 20 s and when leaving the project.
* Known gap: typing inside an open text edit is saved when the edit ends (it is one undo step).

## Phase 2 (paper-rs 0.1) record

| Phase / milestone | Status | Evidence |
|---|---|---|
| Phase 1: reverse engineering | ✅ done | `analysis/PAPER_ARCHITECTURE.md`. REA scoped run is summarized in the appendix; raw output in `analysis/raw/` (gitignored). |
| Phase 2: design | ✅ done | `analysis/DESIGN.md` |
| M1: window + infinite canvas, smooth pan/zoom | ✅ done | `tests/headless.rs::m1_*`, editor scenario tests, `m1_*.png`, `x11_window_xvfb.png` |
| M2: freehand sketch with smoothing | ✅ done | `ink.rs` tests, `m2_freehand_sketch_with_smoothing`, `m2_sketch.png` |
| M3: shapes, frames, text | ✅ done | scenario tests, `m3_shapes_frames_and_text`, `m3_*.png` |
| M4: select/move/resize, multi-select, snapping | ✅ done | scenario tests, `m4_select_move_resize_snap`, `m4_*.png` |
| M5: layers panel, undo/redo | ✅ done | `history.rs` tests, `undo_redo_round_trips_a_whole_session`, `m5_layers_and_undo_redo`, `m5_*.png` |
| M6: save/load `.paper-rs`, PNG/SVG export | ✅ done | `io/*` tests, `m6_save_load_and_export`, `m6_svg_export_matches_gpu_render`, `m6_*` |
| M7: keyboard shortcuts | ✅ done | `editor/shortcuts.rs` (conflict test), README table, in-app `?` help |
| README (build/run, parity table, gaps) | ✅ done | `README.md` |

## Verification log

* `cargo build --release` passes with no warnings. `cargo clippy --all-targets` is clean.
* `cargo test` runs 51 unit/scenario tests and 8 headless GPU tests, all passing. The GPU tests
  ran on Mesa lavapipe (llvmpipe, Vulkan) on the headless VPS.
* The real windowed app was launched under Xvfb (X11) with the release binary and screenshotted.
  It needed `libxkbcommon-x11` and `libxcb-xkb` placed in `~/.local/lib`, since the VPS has no
  root access.
* `cargo check --release --target aarch64-apple-darwin` and `--target x86_64-pc-windows-gnu`
  both pass.
* The SVG export was rasterized with resvg and compared with the GPU PNG export: 0.004% of
  pixels differ.

## Notes

* Commit history: the core model, the editor and the app/renderer/I/O layer landed as three
  feature commits. Each one builds in release mode and has its own tests. Milestones 1–7 are
  spread across them because the layers depend on each other: the app needs I/O, and the
  overlay needs the editor.
* REA's full-asar analysis stalled on `node_modules/typescript/lib/_tsc.js` after 20+ minutes
  at 2.6 GB RSS. A scoped run on Paper's own sources finished in 65 s.

## Next steps (not started)

1. Image nodes (textured quads) and drag-and-drop of image files, to match `paper-asset:`.
2. Per-node mesh cache keyed by revision and zoom bucket.
3. Rotation, plus a bezier pen tool.
4. Multiple pages, and system clipboard interop (SVG/PNG/text).
5. Run and tune on macOS hardware, especially trackpad scroll feel and tablet pressure.
