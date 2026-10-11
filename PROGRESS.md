# Progress

Last updated: 2026-10-10.

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
