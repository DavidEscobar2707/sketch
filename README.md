# paper-rs

A native Rust infinite canvas for planning, working and sketching. It is modeled on the
behavior of [Paper](https://paper.design) and built clean-room on `wgpu` + `winit` + `egui`.

![paper-rs main window](docs/screenshots/app_demo.png)

Frames (artboards), shapes, vector text and pressure-sensitive freehand ink live on an
infinite canvas with smooth pan and zoom. You get selection, move, resize and snapping, a
layers panel, transactional undo/redo, a local `.paper-rs` file format, and PNG/SVG export.

> **Clean-room note.** `analysis/PAPER_ARCHITECTURE.md` documents how Paper Desktop 0.5.15 is
> put together, from a licensed install. The Electron package turned out to be a thin shell
> around the hosted web editor. paper-rs re-implements *behavior* only. No Paper code,
> shaders, icons, fonts or assets are used or committed. The UI and canvas text use
> [Inter](https://rsms.me/inter/) 4.1 (SIL OFL 1.1), bundled in `assets/fonts/`.

## Build and run

You need Rust 1.95 or newer (`rust-version` in `Cargo.toml`; the latest stable is recommended). Install it with [rustup](https://rustup.rs).

```sh
cargo build --release
./target/release/paper-rs                  # empty canvas
./target/release/paper-rs --demo           # sample planning board
./target/release/paper-rs board.paper-rs   # open a document
```

### macOS

```sh
xcode-select --install          # command line tools (linker)
curl https://sh.rustup.rs -sSf | sh
cargo build --release && ./target/release/paper-rs --demo
```

It uses Metal through wgpu. Shortcuts use ⌘, and trackpad pinch zooms. The macOS target is
type-checked in CI-style (`cargo check --target aarch64-apple-darwin`) but has not been run on
Mac hardware yet (see Known gaps).

### Linux

```sh
# Debian/Ubuntu runtime libraries (usually already present on desktops)
sudo apt install libxkbcommon-x11-0 libxcb-xkb1 libvulkan1 mesa-vulkan-drivers
cargo build --release && ./target/release/paper-rs --demo
```

It runs on X11 and Wayland through Vulkan, or GL as a fallback. Shortcuts use Ctrl. File
dialogs use the XDG desktop portal.

### Headless (servers, CI)

No display is needed. wgpu renders offscreen, through Mesa lavapipe when there is no GPU:

```sh
paper-rs --screenshot ui.png --demo --size 1440x900 --scale 2   # full UI
paper-rs --export-png out.png board.paper-rs --scale 2 [--transparent]
paper-rs --export-svg out.svg board.paper-rs
```

## Tests

```sh
cargo test                                # 51 unit/scenario tests + 8 headless GPU tests
PAPER_RS_REQUIRE_GPU=1 cargo test         # fail (instead of skip) when no GPU adapter exists
```

* **Unit tests** cover camera math, ink smoothing and pressure, text layout, snapping,
  transactions, serialization and SVG.
* **Scenario tests** (`src/editor/tests.rs`) drive the editor with synthetic pointer and
  keyboard events. They draw, create, select, drag, resize, snap, nudge, copy/paste, reparent,
  undo and redo.
* **Headless GPU tests** (`tests/headless.rs`) render each milestone through wgpu, assert pixel
  colors at known positions, and write the images below. One test rasterizes the SVG export
  with resvg and requires it to match the GPU PNG export: 0.004% of pixels differ, all on
  anti-aliased edges.

| Milestone | Screenshot(s) |
|---|---|
| 1. Infinite canvas, pan and zoom | `m1_canvas.png`, `m1_zoomed.png`, `m1_zoom_to_fit.png` |
| 2. Freehand sketch with smoothing | `m2_sketch.png` (smoothing 0, 0.5, 0.9; fast vs slow; pen pressure ramp) |
| 3. Shapes, frames, text | `m3_shapes_frames_text.png` (frame clipping, rounded rect, ellipse, line, text) |
| 4. Select/move/resize, multi-select, snapping | `m4_multi_select.png`, `m4_snapping.png` (live guides), `m4_resized.png` |
| 5. Layers panel and undo/redo | `m5_layers_edited.png`, `m5_layers_after_undo.png` |
| 6. Save/load and PNG/SVG export | `m6_launch_plan.paper-rs`, `m6_export_frame@2x.png`, `m6_export_all_transparent.png`, `m6_export.svg`, `m6_export_svg_via_resvg.png` |
| Real window (X11 under Xvfb, lavapipe) | `x11_window_xvfb.png` |

All of these are in `docs/screenshots/`.

## Using it

The tools sit in the floating bar at the bottom of the canvas. Select a layer to edit it in
the **Design** panel on the right. The **Layers** panel on the left shows the tree: click to
select, Shift-click to add, double-click to rename, and use 👁 / 🔒 to hide or lock. The
**Up/Down** buttons reorder.

* Draw a frame around existing shapes and it adopts them. Shapes drawn inside a frame become
  its children, and dragging a layer into or out of a frame re-parents it.
* Top-level frames that contain content act as canvas regions. Drag from their body to
  marquee-select inside them, and drag the frame by its name label.
* The pencil keeps a constant on-screen width at any zoom. With nothing selected, set width,
  smoothing and color in the Design panel.
* Hold ⌘/Ctrl while dragging to suspend snapping.

### Keyboard shortcuts

⌘ on macOS, Ctrl on Linux and Windows. Press `?` in the app for the full list.

| Keys | Action | Keys | Action |
|---|---|---|---|
| V | Select | ⌘Z / ⌘⇧Z / ⌘Y | Undo / redo |
| H, or hold Space | Hand (pan) | ⌘C / ⌘X / ⌘V | Copy / cut / paste |
| F or A | Frame | ⌘D | Duplicate |
| R | Rectangle | ⌘A | Select all (in scope) |
| O | Ellipse | Delete / Backspace | Delete |
| L | Line | ⌘] / ⌘[ | Forward / backward |
| T | Text | ⌘⇧] / ⌘⇧[ | To front / to back |
| P or B | Pencil (freehand) | ⌘⌥G | Wrap selection in frame |
| E | Eraser | ⌘⇧H / ⌘⇧L | Show/hide, lock/unlock |
| Enter | Edit text / select children | ←↑→↓ (⇧ = 10px) | Nudge |
| Esc | Cancel / select parent / deselect | ⌘' | Toggle snapping |
| ⌘= / ⌘- / ⌘0 | Zoom in / out / 100% | ⌘N / ⌘O | New / open |
| ⇧1 / ⇧2 | Zoom to fit / selection | ⌘S / ⌘⇧S | Save / save as |
| Wheel / ⌘+wheel / pinch | Pan / zoom | ⌘⇧E / ⌘⌥E | Export PNG / SVG |

These match Paper wherever Paper's shortcuts are visible: the desktop menu defines ⌘N, ⌘Z,
⌘⇧Z, ⌘X/C/V, ⌘A, and canvas zoom on ⌘+, ⌘-, ⌘0. Paper's tool hotkeys live in its web client,
so the tool letters follow the shared design-tool convention.

## Feature parity with Paper

"Paper evidence" says where the Paper behavior is known from. **Shell** means seen directly in
the desktop bundle. **Inferred** means deduced from the shell's integration points. **Brief**
means requested in the project brief rather than observed.

| Feature | Paper evidence | paper-rs |
|---|---|---|
| Infinite canvas, pan/zoom, ⌘+/-/0 | Shell (`paper-tab.ts`, `application-menu.ts`) | ✅ Zoom about cursor (1%–25600%), adaptive dot grid, fit and fit-selection |
| Artboards / frames | Shell (`create_artboard`) | ✅ Clipping frames, nested frames, name labels, adopt-on-draw, reparent-on-drop |
| Boxes / shapes | Inferred (HTML elements) | ✅ Rectangle (radius), ellipse, line. ❌ CSS box model, auto layout |
| Text | Inferred | ✅ Vector text, wrapping, alignment field, inline editing. ⚠️ One font, no rich text |
| Freehand sketching | Brief | ✅ Smoothing, simulated and real (touch/pen force) pressure, eraser |
| Selection | Shell (`get_selection`) | ✅ Click, deep select, Shift toggle, marquee, select all, Esc to parent |
| Move / resize | Inferred | ✅ 8 handles, Shift aspect, Alt from center, multi-select scaling, nudge |
| Snapping | Brief | ✅ Edges and centers against siblings and frames, guides, pixel rounding. ❌ Spacing guides |
| Layers panel | Brief | ✅ Tree, select, rename, hide, lock, reorder. ❌ Drag-and-drop in panel |
| Undo / redo | Shell (`edit-command`) | ✅ Transactional, one step per gesture, coalesced property edits |
| Style editing | Shell (`update_styles`) | ✅ Fill, stroke, width, radius, opacity, font size, ink width/pressure |
| Copy / paste / duplicate | Shell (menu roles) | ✅ Internal clipboard with subtrees. ❌ System clipboard interop |
| Persistence | Shell: cloud files via sync server | ✅ Local versioned `.paper-rs` JSON, atomic saves. ❌ Cloud sync / multiplayer |
| PNG export | Inferred (`exportImages`) | ✅ 1–3x, selection or all, transparent or white |
| SVG export | — | ✅ Clip paths, outlined glyphs (font-independent) |
| PDF / video export | Shell (`export-pdf`, html-in-canvas) | ❌ |
| Local images | Shell (`paper-asset:` protocol) | ❌ |
| Tabs / multi-window / dashboard | Shell (`window-manager.ts`) | ❌ (one document per window) |
| Pages | Inferred (`pageId`) | ❌ (single page) |
| AI agent bridge (MCP), Figma import, CLI | Shell (`src/mcp`, `open-figma-oauth`, Go CLI) | ❌ (headless CLI for screenshots/export only) |

## Known gaps

* **Paper's document model is HTML/CSS.** Paper nodes are DOM elements with CSS styles and
  layout. paper-rs uses geometric primitives with absolute positions inside frames. There is
  no flex/auto layout, components, variables or code export.
* **Not run on macOS hardware yet.** The macOS build type-checks and uses only portable APIs
  (Metal via wgpu, winit, rfd), but it has not been launched on a Mac. On Linux, the real
  window was exercised under Xvfb; the screenshot is `x11_window_xvfb.png`.
* **Pen pressure.** winit 0.30 does not expose tablet pressure for pen events that arrive as
  mouse input, which is common with macOS and Wacom tablets. Pressure comes from touch/pen
  `force` when the platform reports it. Otherwise it is simulated from stroke velocity.
* **Text** is single-font (Inter Regular) with no shaping, so there are no ligatures, no complex
  scripts and no RTL. There is no in-text selection range and no up/down caret movement, and
  IME preedit is not displayed (committed text works).
* **No rotation, boolean operations, bezier pen tool or images.** Opacity on a frame
  multiplies into its children on the GPU. True group compositing only happens in SVG export.
* **Undo granularity.** Document renames from the Design panel are not undoable, and the
  selection is not part of history.
* **Performance.** The visible scene is re-tessellated whenever the view changes, with no
  per-node mesh cache. That is smooth for thousands of nodes, but not tuned for 100k or more.
* **Linux file dialogs** need an XDG desktop portal. Without one, Save/Open dialogs return
  nothing. The headless CLI is unaffected.

## Project layout

```
src/geom.rs        vectors, rects, camera
src/model/         Node / NodeKind / Document arena
src/history.rs     transactions, undo/redo
src/ink.rs         stroke smoothing, pressure, outline
src/text.rs        glyph layout and outlines
src/editor/        tools, hit testing, snapping, shortcuts, scenario tests
src/render/        lyon → wgpu canvas, headless capture, full-frame painter
src/io/            .paper-rs, PNG, SVG
src/ui.rs          egui panels
src/app.rs         winit application
analysis/          reverse-engineering findings and design (raw dumps are gitignored)
docs/screenshots/  headless renders produced by `cargo test`
```

## License

MIT OR Apache-2.0 for the code in this repository. Inter (Regular, Medium, SemiBold) is
© The Inter Project Authors under the SIL Open Font License 1.1; see `assets/fonts/OFL.txt`.
