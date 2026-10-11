//! The editor: document + history + camera + selection + tool state machine.
//!
//! It consumes platform-agnostic [`InputEvent`]s, so every interaction can be scripted in tests.

pub mod hit;
pub mod input;
pub mod shortcuts;
pub mod snap;

use crate::geom::{Camera, Rect, V2, next_zoom_step, v2};
use crate::history::{History, Recording, Tx};
use crate::ink::{self, InkBuilder};
use crate::model::{Color, Document, Node, NodeId, NodeKind, StrokePoint, TextAlign};
use crate::text;
pub use input::{Button, InputEvent, Key, Modifiers};
use snap::Guide;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    Select,
    Hand,
    Frame,
    Rect,
    Ellipse,
    Line,
    Text,
    Pencil,
    Eraser,
}

impl Tool {
    pub const ALL: [Tool; 9] = [
        Tool::Select,
        Tool::Hand,
        Tool::Frame,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Line,
        Tool::Text,
        Tool::Pencil,
        Tool::Eraser,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Hand => "Hand",
            Tool::Frame => "Frame",
            Tool::Rect => "Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Line => "Line",
            Tool::Text => "Text",
            Tool::Pencil => "Pencil",
            Tool::Eraser => "Eraser",
        }
    }
    pub fn shortcut(self) -> &'static str {
        match self {
            Tool::Select => "V",
            Tool::Hand => "H",
            Tool::Frame => "F",
            Tool::Rect => "R",
            Tool::Ellipse => "O",
            Tool::Line => "L",
            Tool::Text => "T",
            Tool::Pencil => "P",
            Tool::Eraser => "E",
        }
    }
}

/// Requests the editor can't fulfil itself (dialogs, file system, clipboard).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppRequest {
    New,
    Open,
    Save,
    SaveAs,
    ExportPng,
    ExportSvg,
}

#[derive(Debug, Clone)]
pub struct TextEdit {
    pub id: NodeId,
    /// Caret as a char index.
    pub caret: usize,
}

#[derive(Debug)]
enum Gesture {
    None,
    Pan {
        last: V2,
    },
    Marquee {
        start: V2,
        scope: Option<NodeId>,
        base: Vec<NodeId>,
    },
    /// Pressed on a node; becomes Move once the pointer travels far enough.
    Press {
        start_screen: V2,
        start: V2,
        click_select: Option<NodeId>,
    },
    Move {
        start: V2,
        origins: Vec<(NodeId, Rect)>,
        bounds0: Rect,
    },
    Resize {
        handle: (f64, f64),
        start: V2,
        bounds0: Rect,
        origins: Vec<(NodeId, Rect)>,
    },
    Create {
        tool: Tool,
        start: V2,
        id: NodeId,
    },
    Draw {
        builder: Box<InkBuilder>,
        id: NodeId,
    },
    Erase,
}

/// Drag distance (screen px) before a press turns into a move.
const DRAG_THRESHOLD: f64 = 3.0;
const PASTE_OFFSET: f64 = 16.0;

pub struct Editor {
    pub doc: Document,
    pub history: History,
    pub camera: Camera,
    /// Canvas viewport size in logical pixels.
    pub viewport: V2,
    pub selection: Vec<NodeId>,
    pub tool: Tool,
    pub mods: Modifiers,
    pub space_down: bool,
    pub text_edit: Option<TextEdit>,
    pub guides: Vec<Guide>,
    /// World-space marquee rectangle while dragging.
    pub marquee: Option<Rect>,
    pub hover: Option<NodeId>,
    pub pointer: V2,
    pub ink_width: f64,
    pub ink_smoothing: f64,
    pub ink_color: Color,
    pub snapping: bool,
    pub file_path: Option<PathBuf>,
    pub requests: Vec<AppRequest>,
    pub status: String,
    clipboard: Vec<Node>,
    clipboard_roots: Vec<NodeId>,
    gesture: Gesture,
    rec: Option<(Recording, Option<String>)>,
    saved_marker: Option<usize>,
    last_click: Option<(V2, f64)>,
    /// Bumps on every document mutation; lets renderers cache.
    pub revision: u64,
    /// Fit the camera to the content once the real viewport size is known.
    pub pending_fit: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Editor::new(Document::default())
    }
}

impl Editor {
    pub fn new(doc: Document) -> Editor {
        Editor {
            doc,
            history: History::default(),
            camera: Camera::default(),
            viewport: v2(1280.0, 800.0),
            selection: Vec::new(),
            tool: Tool::Select,
            mods: Modifiers::NONE,
            space_down: false,
            text_edit: None,
            guides: Vec::new(),
            marquee: None,
            hover: None,
            pointer: V2::ZERO,
            ink_width: 3.0,
            ink_smoothing: 0.5,
            ink_color: Color::INK,
            snapping: true,
            file_path: None,
            requests: Vec::new(),
            status: String::new(),
            clipboard: Vec::new(),
            clipboard_roots: Vec::new(),
            gesture: Gesture::None,
            rec: None,
            saved_marker: Some(0),
            last_click: None,
            revision: 0,
            pending_fit: false,
        }
    }

    // ------------------------------------------------------------------ transactions

    /// Open a transaction (committing any open one first).
    pub fn begin(&mut self, label: &str) {
        self.begin_coalescing(label, None);
    }

    pub fn begin_coalescing(&mut self, label: &str, key: Option<&str>) {
        self.commit();
        self.rec = Some((Recording::new(label, &self.doc), key.map(String::from)));
    }

    /// Mutable access to the document inside the open transaction (opens one if needed).
    pub fn tx(&mut self) -> Tx<'_> {
        if self.rec.is_none() {
            self.rec = Some((Recording::new("Edit", &self.doc), None));
        }
        self.revision += 1;
        let (rec, _) = self.rec.as_mut().unwrap();
        Tx::new(&mut self.doc, rec)
    }

    pub fn commit(&mut self) {
        if let Some((rec, key)) = self.rec.take()
            && self.history.commit(&self.doc, rec, key)
        {
            self.revision += 1;
        }
    }

    /// Run one edit as a complete transaction.
    pub fn edit<R>(&mut self, label: &str, f: impl FnOnce(&mut Tx) -> R) -> R {
        self.begin(label);
        let r = f(&mut self.tx());
        self.commit();
        r
    }

    pub fn edit_coalescing<R>(
        &mut self,
        label: &str,
        key: &str,
        f: impl FnOnce(&mut Tx) -> R,
    ) -> R {
        self.begin_coalescing(label, Some(key));
        let r = f(&mut self.tx());
        self.commit();
        r
    }

    pub fn undo(&mut self) {
        self.cancel_gesture();
        self.end_text_edit();
        self.commit();
        if let Some(l) = self.history.undo(&mut self.doc) {
            self.status = format!("Undo {l}");
        }
        self.after_history();
    }

    pub fn redo(&mut self) {
        self.cancel_gesture();
        self.end_text_edit();
        self.commit();
        if let Some(l) = self.history.redo(&mut self.doc) {
            self.status = format!("Redo {l}");
        }
        self.after_history();
    }

    fn after_history(&mut self) {
        self.revision += 1;
        self.selection.retain(|id| self.doc.nodes.contains_key(id));
    }

    pub fn is_dirty(&self) -> bool {
        self.saved_marker != Some(self.history.undo_len()) || self.rec.is_some()
    }

    pub fn mark_saved(&mut self) {
        self.history.seal();
        self.saved_marker = Some(self.history.undo_len());
    }

    /// Replace the document (open/new).
    pub fn load(&mut self, doc: Document, camera: Option<Camera>, path: Option<PathBuf>) {
        *self = Editor {
            viewport: self.viewport,
            ink_width: self.ink_width,
            ..Editor::new(doc)
        };
        self.file_path = path;
        match camera {
            Some(c) => self.camera = c,
            None => {
                self.zoom_to_fit();
                self.pending_fit = true;
            }
        }
        self.revision += 1;
    }

    // ------------------------------------------------------------------ queries

    pub fn screen_to_world(&self, p: V2) -> V2 {
        self.camera.screen_to_world(p)
    }

    /// Selected nodes without those whose ancestor is also selected.
    pub fn top_selection(&self) -> Vec<NodeId> {
        self.selection
            .iter()
            .copied()
            .filter(|id| self.doc.nodes.contains_key(id))
            .filter(|id| {
                !self
                    .doc
                    .ancestors(*id)
                    .iter()
                    .any(|a| self.selection.contains(a))
            })
            .collect()
    }

    pub fn selection_bounds(&self) -> Option<Rect> {
        let rects: Vec<Rect> = self
            .top_selection()
            .iter()
            .filter_map(|id| self.doc.world_rect(*id))
            .collect();
        Rect::union_all(&rects)
    }

    pub fn select(&mut self, ids: Vec<NodeId>) {
        if self
            .text_edit
            .as_ref()
            .is_some_and(|t| !ids.contains(&t.id))
        {
            self.end_text_edit();
        }
        self.selection = ids;
    }

    fn snap_threshold(&self) -> f64 {
        snap::SNAP_PX / self.camera.zoom
    }

    fn snapping_active(&self) -> bool {
        self.snapping && !self.mods.command()
    }

    /// Rects of nodes that moving `ids` may snap against: siblings, the parent frame, top-level frames.
    fn snap_candidates(&self, ids: &[NodeId]) -> Vec<Rect> {
        let excluded: Vec<NodeId> = ids.iter().flat_map(|id| self.doc.subtree(*id)).collect();
        let vis = self
            .camera
            .visible_world(self.viewport)
            .inflate(200.0 / self.camera.zoom);
        let mut parents: Vec<Option<NodeId>> = ids
            .iter()
            .map(|id| self.doc.get(*id).and_then(|n| n.parent))
            .collect();
        parents.dedup();
        let mut out = Vec::new();
        let mut push = |id: NodeId| {
            if !excluded.contains(&id)
                && self.doc.is_effectively_visible(id)
                && let Some(r) = self.doc.world_rect(id).filter(|r| r.intersects(&vis))
            {
                out.push(r);
            }
        };
        for p in parents.iter().copied() {
            for c in self.doc.children(p) {
                push(*c);
            }
            if let Some(p) = p {
                push(p);
            }
        }
        for id in self.doc.root.clone() {
            if self.doc.get(id).is_some_and(|n| n.is_frame()) {
                push(id);
            }
        }
        out.dedup();
        out
    }

    /// Topmost frame (deepest first) containing world point `p`, excluding `exclude` subtrees.
    pub fn frame_at(&self, p: V2, exclude: &[NodeId]) -> Option<NodeId> {
        let excluded: Vec<NodeId> = exclude
            .iter()
            .flat_map(|id| self.doc.subtree(*id))
            .collect();
        self.doc.paint_order().into_iter().rev().find(|id| {
            !excluded.contains(id)
                && self.doc.get(*id).is_some_and(|n| n.is_frame())
                && self.doc.is_effectively_visible(*id)
                && self.doc.world_rect(*id).is_some_and(|r| r.contains(p))
                && hit::inside_ancestor_clips(&self.doc, *id, p)
        })
    }

    /// Screen-space frame label rectangles for top-level frames (also used by the renderer).
    pub fn frame_labels(&self) -> Vec<(NodeId, Rect)> {
        self.doc
            .root
            .iter()
            .filter_map(|id| {
                let n = self.doc.get(*id)?;
                if !n.is_frame() || !n.visible {
                    return None;
                }
                let sr = self.camera.world_rect_to_screen(&self.doc.world_rect(*id)?);
                let w = text::layout(&n.name, 12.0, None, TextAlign::Left).width;
                Some((*id, hit::frame_label_rect(&sr, w.min(sr.w.max(24.0)))))
            })
            .collect()
    }

    fn label_at(&self, p: V2) -> Option<NodeId> {
        self.frame_labels()
            .into_iter()
            .rev()
            .find(|(id, r)| r.contains(p) && !self.doc.is_effectively_locked(*id))
            .map(|(id, _)| id)
    }

    // ------------------------------------------------------------------ input

    pub fn handle(&mut self, ev: InputEvent) {
        match ev {
            InputEvent::Modifiers(m) => self.mods = m,
            InputEvent::PointerDown {
                pos,
                button,
                pressure,
                time_ms,
            } => {
                self.pointer = pos;
                self.pointer_down(pos, button, pressure, time_ms)
            }
            InputEvent::PointerMove {
                pos,
                pressure,
                time_ms,
            } => {
                self.pointer = pos;
                self.pointer_move(pos, pressure, time_ms)
            }
            InputEvent::PointerUp {
                pos,
                button,
                time_ms,
            } => {
                self.pointer = pos;
                self.pointer_up(pos, button, time_ms)
            }
            InputEvent::Scroll { delta, pos } => {
                if self.mods.ctrl || self.mods.logo {
                    let factor = (-delta.y * 0.01).exp();
                    self.camera.zoom_about(pos, self.camera.zoom * factor);
                } else {
                    self.camera.pan_by_screen(-delta);
                }
                self.revision += 1;
            }
            InputEvent::Zoom { factor, pos } => {
                self.camera.zoom_about(pos, self.camera.zoom * factor);
                self.revision += 1;
            }
            InputEvent::KeyDown { key, repeat } => self.key_down(key, repeat),
            InputEvent::KeyUp { key } => {
                if key == Key::Space {
                    self.space_down = false;
                }
            }
            InputEvent::Text(s) => {
                if self.text_edit.is_some() && !self.mods.command() {
                    for c in s.chars().filter(|c| !c.is_control()) {
                        self.insert_text(&c.to_string());
                    }
                }
            }
        }
    }

    fn pointer_down(&mut self, pos: V2, button: Button, pressure: Option<f64>, time_ms: f64) {
        let world = self.screen_to_world(pos);
        if button == Button::Middle
            || (button == Button::Primary && (self.space_down || self.tool == Tool::Hand))
        {
            self.gesture = Gesture::Pan { last: pos };
            return;
        }
        if button != Button::Primary {
            return;
        }
        let double = self
            .last_click
            .is_some_and(|(p, t)| p.dist(pos) < 5.0 && time_ms - t < 400.0);
        self.last_click = Some((pos, time_ms));

        match self.tool {
            Tool::Select | Tool::Hand => self.select_down(pos, world, double),
            Tool::Pencil => {
                self.end_text_edit();
                self.begin("Draw");
                let zoom = self.camera.zoom;
                let mut builder = InkBuilder::new(self.ink_smoothing, 0.75 / zoom);
                builder.push(world, pressure, time_ms, zoom);
                let parent = self.frame_at(world, &[]);
                let width = self.ink_width / zoom;
                let color = self.ink_color;
                let id = {
                    let mut tx = self.tx();
                    let id = tx.alloc_id();
                    let mut n = Node::new(
                        id,
                        NodeKind::Stroke {
                            points: Vec::new(),
                            width,
                            thinning: 0.6,
                        },
                        Rect::default(),
                    );
                    n.style.fill = Some(color);
                    tx.insert(n, parent, None)
                };
                self.update_stroke_node(id, builder.points().to_vec());
                self.gesture = Gesture::Draw {
                    builder: Box::new(builder),
                    id,
                };
            }
            Tool::Eraser => {
                self.end_text_edit();
                self.begin("Erase");
                self.gesture = Gesture::Erase;
                self.erase_at(world);
            }
            Tool::Text => {
                if let Some(id) = hit::hit_test(&self.doc, world, self.camera.zoom)
                    .filter(|id| matches!(self.doc.get(*id).unwrap().kind, NodeKind::Text { .. }))
                {
                    self.start_text_edit(id, Some(world));
                    return;
                }
                self.end_text_edit();
                self.begin("Add text");
                let parent = self.frame_at(world, &[]);
                let origin = self.doc.parent_origin(parent);
                let size = (16.0 / self.camera.zoom.max(0.25)).round().max(1.0);
                let lh = text::line_height(size);
                let p = (world - origin - v2(0.0, lh * 0.5)).round();
                let id = {
                    let mut tx = self.tx();
                    let id = tx.alloc_id();
                    let n = Node::new(
                        id,
                        NodeKind::Text {
                            text: String::new(),
                            font_size: size,
                            auto_width: true,
                            align: TextAlign::Left,
                        },
                        Rect::new(p.x, p.y, 1.0, lh),
                    );
                    tx.insert(n, parent, None)
                };
                self.selection = vec![id];
                self.text_edit = Some(TextEdit { id, caret: 0 });
                self.tool = Tool::Select;
            }
            Tool::Frame | Tool::Rect | Tool::Ellipse | Tool::Line => {
                self.end_text_edit();
                let label = format!("Add {}", self.tool.label().to_lowercase());
                self.begin(&label);
                let start = if self.snapping { world.round() } else { world };
                let parent = self.frame_at(start, &[]);
                let origin = self.doc.parent_origin(parent);
                let kind = match self.tool {
                    Tool::Frame => NodeKind::Frame { clip: true },
                    Tool::Rect => NodeKind::Rect,
                    Tool::Ellipse => NodeKind::Ellipse,
                    _ => NodeKind::Line {
                        a: V2::ZERO,
                        b: V2::ZERO,
                    },
                };
                let frame_count = self.doc.nodes.values().filter(|n| n.is_frame()).count();
                let id = {
                    let mut tx = self.tx();
                    let id = tx.alloc_id();
                    let mut n = Node::new(
                        id,
                        kind,
                        Rect::new(start.x - origin.x, start.y - origin.y, 0.0, 0.0),
                    );
                    if n.is_frame() {
                        n.name = format!("Frame {}", frame_count + 1);
                    }
                    tx.insert(n, parent, None)
                };
                self.selection = vec![id];
                self.gesture = Gesture::Create {
                    tool: self.tool,
                    start,
                    id,
                };
            }
        }
    }

    fn select_down(&mut self, pos: V2, world: V2, double: bool) {
        // Resize handles of the current selection take priority.
        if self.text_edit.is_none()
            && let Some(b) = self.selection_bounds()
        {
            let bs = self.camera.world_rect_to_screen(&b);
            if let Some(handle) = hit::handle_at(&bs, pos) {
                let origins = self.top_selection_world();
                if !origins.is_empty() {
                    self.begin("Resize");
                    self.gesture = Gesture::Resize {
                        handle,
                        start: world,
                        bounds0: b,
                        origins,
                    };
                    return;
                }
            }
        }
        if let Some(id) = self.label_at(pos) {
            if !self.selection.contains(&id) {
                self.select(if self.mods.shift {
                    [self.selection.clone(), vec![id]].concat()
                } else {
                    vec![id]
                });
            }
            self.gesture = Gesture::Press {
                start_screen: pos,
                start: world,
                click_select: None,
            };
            return;
        }
        let hit = hit::hit_test(&self.doc, world, self.camera.zoom);
        // Editing text: clicks inside the text move the caret.
        if let (Some(te), Some(h)) = (&self.text_edit, hit)
            && te.id == h
        {
            self.start_text_edit(h, Some(world));
            return;
        }
        self.end_text_edit();
        let Some(mut target) = hit else {
            self.start_marquee(world, None);
            return;
        };
        // Top-level frames with content act as canvas regions: their body starts a marquee.
        let node = self.doc.get(target).unwrap();
        if node.is_frame()
            && node.parent.is_none()
            && !node.children.is_empty()
            && !self.selection.contains(&target)
        {
            self.start_marquee(world, Some(target));
            return;
        }
        if double && matches!(node.kind, NodeKind::Text { .. }) {
            self.start_text_edit(target, Some(world));
            return;
        }
        // If an ancestor is already selected, keep dragging the ancestor.
        if let Some(a) = self
            .doc
            .ancestors(target)
            .into_iter()
            .find(|a| self.selection.contains(a))
        {
            target = a;
        }
        let click_select = if self.mods.shift {
            if self.selection.contains(&target) {
                Some(target) // toggle off on release if no drag
            } else {
                self.selection.push(target);
                None
            }
        } else if self.selection.contains(&target) {
            // Pressing an already-selected node keeps a multi-selection for dragging,
            // and narrows to it on a click without drag.
            (self.selection.len() > 1).then_some(target)
        } else {
            self.select(vec![target]);
            None
        };
        self.gesture = Gesture::Press {
            start_screen: pos,
            start: world,
            click_select,
        };
    }

    fn start_marquee(&mut self, world: V2, scope: Option<NodeId>) {
        let base = if self.mods.shift {
            self.selection.clone()
        } else {
            Vec::new()
        };
        if !self.mods.shift {
            self.select(Vec::new());
        }
        self.gesture = Gesture::Marquee {
            start: world,
            scope,
            base,
        };
    }

    fn top_selection_world(&self) -> Vec<(NodeId, Rect)> {
        self.top_selection()
            .into_iter()
            .filter(|id| !self.doc.is_effectively_locked(*id))
            .filter_map(|id| Some((id, self.doc.world_rect(id)?)))
            .collect()
    }

    fn pointer_move(&mut self, pos: V2, pressure: Option<f64>, time_ms: f64) {
        let world = self.screen_to_world(pos);
        match std::mem::replace(&mut self.gesture, Gesture::None) {
            Gesture::None => {
                self.hover = if self.tool == Tool::Select {
                    self.label_at(pos)
                        .or_else(|| hit::hit_test(&self.doc, world, self.camera.zoom))
                } else {
                    None
                };
            }
            Gesture::Pan { last } => {
                self.camera.pan_by_screen(pos - last);
                self.revision += 1;
                self.gesture = Gesture::Pan { last: pos };
            }
            Gesture::Press {
                start_screen,
                start,
                click_select,
            } => {
                if pos.dist(start_screen) < DRAG_THRESHOLD {
                    self.gesture = Gesture::Press {
                        start_screen,
                        start,
                        click_select,
                    };
                    return;
                }
                let origins = self.top_selection_world();
                if origins.is_empty() {
                    return;
                }
                let bounds0 = Rect::union_all(origins.iter().map(|(_, r)| r)).unwrap();
                self.begin("Move");
                self.gesture = Gesture::Move {
                    start,
                    origins,
                    bounds0,
                };
                self.pointer_move(pos, pressure, time_ms);
            }
            Gesture::Move {
                start,
                origins,
                bounds0,
            } => {
                let mut d = world - start;
                if self.mods.shift {
                    // Constrain to the dominant axis.
                    if d.x.abs() > d.y.abs() {
                        d.y = 0.0
                    } else {
                        d.x = 0.0
                    }
                }
                let mut moved = bounds0.translate(d);
                self.guides.clear();
                if self.snapping_active() {
                    let ids: Vec<NodeId> = origins.iter().map(|(id, _)| *id).collect();
                    let cands = self.snap_candidates(&ids);
                    let (sx, sy, guides) = snap::snap_rect(
                        &moved,
                        [true; 3],
                        [true; 3],
                        &cands,
                        self.snap_threshold(),
                    );
                    moved = moved.translate(v2(sx, sy));
                    if sx == 0.0 {
                        moved.x = moved.x.round();
                    }
                    if sy == 0.0 {
                        moved.y = moved.y.round();
                    }
                    self.guides = guides;
                }
                let d = moved.min() - bounds0.min();
                for (id, r0) in &origins {
                    let parent = self.doc.get(*id).and_then(|n| n.parent);
                    let o = self.doc.parent_origin(parent);
                    let r = r0.translate(d - o);
                    if let Some(n) = self.tx().node_mut(*id) {
                        n.rect = r;
                    }
                }
                self.gesture = Gesture::Move {
                    start,
                    origins,
                    bounds0,
                };
            }
            Gesture::Resize {
                handle,
                start,
                bounds0,
                origins,
            } => {
                let nb = self.resize_bounds(handle, start, world, bounds0);
                let sx = if bounds0.w > 1e-9 {
                    nb.w / bounds0.w
                } else {
                    1.0
                };
                let sy = if bounds0.h > 1e-9 {
                    nb.h / bounds0.h
                } else {
                    1.0
                };
                for (id, r0) in &origins {
                    let wr = Rect::new(
                        nb.x + (r0.x - bounds0.x) * sx,
                        nb.y + (r0.y - bounds0.y) * sy,
                        (r0.w * sx).max(0.0),
                        (r0.h * sy).max(0.0),
                    );
                    let parent = self.doc.get(*id).and_then(|n| n.parent);
                    let o = self.doc.parent_origin(parent);
                    if let Some(n) = self.tx().node_mut(*id) {
                        n.set_rect_scaling(wr.translate(V2::ZERO - o));
                    }
                    self.fit_text(*id);
                }
                self.gesture = Gesture::Resize {
                    handle,
                    start,
                    bounds0,
                    origins,
                };
            }
            Gesture::Marquee { start, scope, base } => {
                let r = Rect::from_points(start, world);
                self.marquee = Some(r);
                let pool: Vec<NodeId> = self.doc.children(scope).to_vec();
                let mut sel = base.clone();
                for id in pool {
                    let n = self.doc.get(id).unwrap();
                    if !n.visible || n.locked || sel.contains(&id) {
                        continue;
                    }
                    let Some(wr) = self.doc.world_rect(id) else {
                        continue;
                    };
                    // Top-level frames must be fully enclosed; everything else just intersect.
                    let hit = if n.is_frame() && scope.is_none() {
                        r.contains_rect(&wr)
                    } else {
                        r.intersects(&wr)
                    };
                    if hit {
                        sel.push(id);
                    }
                }
                self.selection = sel;
                self.gesture = Gesture::Marquee { start, scope, base };
            }
            Gesture::Create { tool, start, id } => {
                let mut end = if self.snapping { world.round() } else { world };
                if self.mods.shift && tool != Tool::Line {
                    let d = end - start;
                    let s = d.x.abs().max(d.y.abs());
                    end = start + v2(s * d.x.signum(), s * d.y.signum());
                }
                if self.mods.shift && tool == Tool::Line {
                    // Snap the angle to 45° increments.
                    let d = end - start;
                    let a = (d.y.atan2(d.x) / std::f64::consts::FRAC_PI_4).round()
                        * std::f64::consts::FRAC_PI_4;
                    end = start + v2(a.cos(), a.sin()) * d.len();
                }
                let (a, b) = if self.mods.alt {
                    (start - (end - start), end)
                } else {
                    (start, end)
                };
                let wr = Rect::from_points(a, b);
                let parent = self.doc.get(id).and_then(|n| n.parent);
                let o = self.doc.parent_origin(parent);
                if let Some(n) = self.tx().node_mut(id) {
                    n.rect = wr.translate(V2::ZERO - o);
                    if let NodeKind::Line { a: la, b: lb } = &mut n.kind {
                        *la = a - wr.min();
                        *lb = b - wr.min();
                    }
                }
                self.gesture = Gesture::Create { tool, start, id };
            }
            Gesture::Draw { mut builder, id } => {
                builder.push(world, pressure, time_ms, self.camera.zoom);
                self.update_stroke_node(id, builder.points().to_vec());
                self.gesture = Gesture::Draw { builder, id };
            }
            Gesture::Erase => {
                self.erase_at(world);
                self.gesture = Gesture::Erase;
            }
        }
    }

    fn resize_bounds(&self, (hx, hy): (f64, f64), start: V2, world: V2, b0: Rect) -> Rect {
        let d = world - start;
        let (mut x0, mut y0, mut x1, mut y1) = (b0.x, b0.y, b0.right(), b0.bottom());
        let sym = self.mods.alt;
        if hx == 0.0 {
            x0 += d.x;
            if sym {
                x1 -= d.x;
            }
        } else if hx == 1.0 {
            x1 += d.x;
            if sym {
                x0 -= d.x;
            }
        }
        if hy == 0.0 {
            y0 += d.y;
            if sym {
                y1 -= d.y;
            }
        } else if hy == 1.0 {
            y1 += d.y;
            if sym {
                y0 -= d.y;
            }
        }
        if self.mods.shift && b0.w > 0.0 && b0.h > 0.0 {
            let aspect = b0.w / b0.h;
            let (w, h) = ((x1 - x0).abs(), (y1 - y0).abs());
            let edge_x = hy == 0.5;
            let edge_y = hx == 0.5;
            let (nw, nh) = if edge_x || (!edge_y && w / aspect >= h) {
                (w, w / aspect)
            } else {
                (h * aspect, h)
            };
            // Keep the anchor (opposite side) fixed.
            if hx == 0.0 {
                x0 = x1 - nw
            } else if hx == 1.0 {
                x1 = x0 + nw
            } else {
                let c = (x0 + x1) * 0.5;
                x0 = c - nw * 0.5;
                x1 = c + nw * 0.5
            }
            if hy == 0.0 {
                y0 = y1 - nh
            } else if hy == 1.0 {
                y1 = y0 + nh
            } else {
                let c = (y0 + y1) * 0.5;
                y0 = c - nh * 0.5;
                y1 = c + nh * 0.5
            }
        }
        let mut r = Rect::from_points(v2(x0, y0), v2(x1, y1));
        if self.snapping_active() && !self.mods.shift {
            let ids: Vec<NodeId> = self.top_selection();
            let cands = self.snap_candidates(&ids);
            let ux = [hx == 0.0, false, hx == 1.0];
            let uy = [hy == 0.0, false, hy == 1.0];
            let (sx, sy, _) = snap::snap_rect(&r, ux, uy, &cands, self.snap_threshold());
            if hx == 0.0 {
                r.x += sx;
                r.w -= sx
            } else if hx == 1.0 {
                r.w += sx
            }
            if hy == 0.0 {
                r.y += sy;
                r.h -= sy
            } else if hy == 1.0 {
                r.h += sy
            }
            let round = |v: f64| v.round();
            if sx == 0.0 && hx != 0.5 {
                let (l, rr) = (round(r.x), round(r.right()));
                r.x = l;
                r.w = rr - l;
            }
            if sy == 0.0 && hy != 0.5 {
                let (t, bb) = (round(r.y), round(r.bottom()));
                r.y = t;
                r.h = bb - t;
            }
        }
        r.w = r.w.max(1.0 / self.camera.zoom);
        r.h = r.h.max(1.0 / self.camera.zoom);
        r
    }

    fn pointer_up(&mut self, pos: V2, button: Button, _time_ms: f64) {
        let world = self.screen_to_world(pos);
        let _ = button;
        match std::mem::replace(&mut self.gesture, Gesture::None) {
            Gesture::None | Gesture::Pan { .. } => {}
            Gesture::Press { click_select, .. } => {
                if let Some(id) = click_select {
                    if self.mods.shift {
                        self.selection.retain(|s| *s != id);
                    } else {
                        self.selection = vec![id];
                    }
                }
            }
            Gesture::Move { origins, .. } => {
                self.reparent_after_move(origins.iter().map(|(id, _)| *id).collect());
                self.guides.clear();
                self.commit();
            }
            Gesture::Resize { .. } => {
                self.guides.clear();
                self.commit();
            }
            Gesture::Marquee { .. } => {
                self.marquee = None;
            }
            Gesture::Create { tool, start, id } => {
                let r = self.doc.get(id).map(|n| n.rect).unwrap_or_default();
                // A click without dragging creates a default-sized shape at the pointer.
                if r.w * self.camera.zoom < 2.0 && r.h * self.camera.zoom < 2.0 {
                    let (w, h) = match tool {
                        Tool::Frame => (400.0, 300.0),
                        Tool::Line => (100.0, 0.0),
                        _ => (100.0, 100.0),
                    };
                    let parent = self.doc.get(id).and_then(|n| n.parent);
                    let o = self.doc.parent_origin(parent);
                    if let Some(n) = self.tx().node_mut(id) {
                        n.rect = Rect::new(start.x - o.x, start.y - o.y, w, h);
                        if let NodeKind::Line { a, b } = &mut n.kind {
                            *a = V2::ZERO;
                            *b = v2(w, 0.0);
                        }
                    }
                }
                if tool == Tool::Frame {
                    self.adopt_into_new_frame(id);
                }
                self.commit();
                self.tool = Tool::Select;
            }
            Gesture::Draw { builder, id } => {
                let zoom = self.camera.zoom;
                let pts = builder.finish(Some(world), 1.5 / zoom);
                self.update_stroke_node(id, pts);
                self.commit();
            }
            Gesture::Erase => self.commit(),
        }
    }

    /// When a frame is drawn around existing siblings, they move into it.
    fn adopt_into_new_frame(&mut self, frame: NodeId) {
        let Some(fr) = self.doc.world_rect(frame) else {
            return;
        };
        let parent = self.doc.get(frame).and_then(|n| n.parent);
        let inside: Vec<NodeId> = self
            .doc
            .children(parent)
            .iter()
            .copied()
            .filter(|id| *id != frame)
            .filter(|id| {
                self.doc
                    .world_rect(*id)
                    .is_some_and(|r| fr.contains_rect(&r))
            })
            .collect();
        let mut tx = self.tx();
        for id in inside {
            tx.reparent(id, Some(frame), None);
        }
    }

    fn reparent_after_move(&mut self, ids: Vec<NodeId>) {
        for id in ids.iter().copied() {
            let Some(r) = self.doc.world_rect(id) else {
                continue;
            };
            let current = self.doc.get(id).and_then(|n| n.parent);
            let target = self.frame_at(r.center(), &ids);
            // Only move out of the current parent if fully outside it; move into a frame on center.
            let stay = current.is_some_and(|p| {
                target.is_none_or(|t| t == p || self.doc.is_ancestor(t, p))
                    && self.doc.world_rect(p).is_some_and(|pr| pr.intersects(&r))
            });
            if stay || target == current {
                continue;
            }
            self.tx().reparent(id, target, None);
        }
    }

    fn update_stroke_node(&mut self, id: NodeId, world_pts: Vec<StrokePoint>) {
        let Some(NodeKind::Stroke {
            width, thinning, ..
        }) = self.doc.get(id).map(|n| n.kind.clone())
        else {
            return;
        };
        let parent = self.doc.get(id).and_then(|n| n.parent);
        let o = self.doc.parent_origin(parent);
        let b = ink::bounds(&world_pts, width, thinning);
        let local: Vec<StrokePoint> = world_pts
            .iter()
            .map(|p| StrokePoint {
                x: p.x - b.x,
                y: p.y - b.y,
                p: p.p,
            })
            .collect();
        if let Some(n) = self.tx().node_mut(id) {
            n.rect = b.translate(V2::ZERO - o);
            if let NodeKind::Stroke { points, .. } = &mut n.kind {
                *points = local;
            }
        }
    }

    fn erase_at(&mut self, world: V2) {
        let zoom = self.camera.zoom;
        let victims: Vec<NodeId> = self
            .doc
            .paint_order()
            .into_iter()
            .filter(|id| matches!(self.doc.get(*id).unwrap().kind, NodeKind::Stroke { .. }))
            .filter(|id| {
                self.doc.is_effectively_visible(*id) && !self.doc.is_effectively_locked(*id)
            })
            .filter(|id| hit::node_contains(&self.doc, *id, world, zoom / 2.0))
            .collect();
        for v in victims {
            self.tx().remove(v);
            self.selection.retain(|s| *s != v);
        }
    }

    // ------------------------------------------------------------------ text

    /// Recompute a text node's box from its content.
    pub fn fit_text(&mut self, id: NodeId) {
        let Some(n) = self.doc.get(id) else { return };
        let NodeKind::Text {
            text: t,
            font_size,
            auto_width,
            align,
        } = &n.kind
        else {
            return;
        };
        let l = text::layout(t, *font_size, (!auto_width).then_some(n.rect.w), *align);
        let (w, h) = (
            if *auto_width {
                l.width.max(1.0)
            } else {
                n.rect.w
            },
            l.height,
        );
        let r = n.rect;
        if ((r.w - w).abs() > 1e-9 || (r.h - h).abs() > 1e-9)
            && let Some(n) = self.tx().node_mut(id)
        {
            n.rect.w = w;
            n.rect.h = h;
        }
    }

    pub fn start_text_edit(&mut self, id: NodeId, at_world: Option<V2>) {
        let Some(n) = self.doc.get(id) else { return };
        let NodeKind::Text {
            text: t,
            font_size,
            auto_width,
            align,
        } = &n.kind
        else {
            return;
        };
        let caret = match at_world {
            Some(p) => {
                let l = text::layout(t, *font_size, (!auto_width).then_some(n.rect.w), *align);
                let local = p - self.doc.world_rect(id).unwrap().min();
                l.hit(local)
            }
            None => t.chars().count(),
        };
        if self.text_edit.as_ref().is_some_and(|te| te.id == id) {
            self.text_edit.as_mut().unwrap().caret = caret;
            return;
        }
        self.end_text_edit();
        self.begin("Edit text");
        self.selection = vec![id];
        self.text_edit = Some(TextEdit { id, caret });
    }

    /// Finish text editing; empty text nodes are removed.
    pub fn end_text_edit(&mut self) {
        let Some(te) = self.text_edit.take() else {
            return;
        };
        let empty = matches!(self.doc.get(te.id).map(|n| &n.kind), Some(NodeKind::Text { text, .. }) if text.trim().is_empty());
        if empty {
            self.tx().remove(te.id);
            self.selection.retain(|s| *s != te.id);
        }
        self.commit();
    }

    fn with_text(&mut self, f: impl FnOnce(&mut String, &mut usize)) {
        let Some(te) = self.text_edit.clone() else {
            return;
        };
        let mut caret = te.caret;
        if let Some(n) = self.tx().node_mut(te.id)
            && let NodeKind::Text { text, .. } = &mut n.kind
        {
            f(text, &mut caret);
        }
        self.text_edit = Some(TextEdit { id: te.id, caret });
        self.fit_text(te.id);
    }

    pub fn insert_text(&mut self, s: &str) {
        self.with_text(|t, caret| {
            let byte = t.char_indices().nth(*caret).map_or(t.len(), |(b, _)| b);
            t.insert_str(byte, s);
            *caret += s.chars().count();
        });
    }

    fn text_key(&mut self, key: &Key) -> bool {
        match key {
            Key::Escape => {
                self.end_text_edit();
                true
            }
            Key::Enter if self.mods.command() => {
                self.end_text_edit();
                true
            }
            Key::Enter => {
                self.insert_text("\n");
                true
            }
            Key::Backspace => {
                self.with_text(|t, c| {
                    if *c > 0 {
                        let (b, ch) = t.char_indices().nth(*c - 1).unwrap();
                        t.replace_range(b..b + ch.len_utf8(), "");
                        *c -= 1;
                    }
                });
                true
            }
            Key::Delete => {
                self.with_text(|t, c| {
                    if let Some((b, ch)) = t.char_indices().nth(*c) {
                        t.replace_range(b..b + ch.len_utf8(), "");
                    }
                });
                true
            }
            Key::Left | Key::Right | Key::Home | Key::End => {
                if let Some(te) = &mut self.text_edit {
                    let len = match self.doc.get(te.id).map(|n| &n.kind) {
                        Some(NodeKind::Text { text, .. }) => text.chars().count(),
                        _ => 0,
                    };
                    te.caret = match key {
                        Key::Left => te.caret.saturating_sub(1),
                        Key::Right => (te.caret + 1).min(len),
                        Key::Home => 0,
                        _ => len,
                    };
                }
                true
            }
            Key::Space | Key::Char(_) | Key::Up | Key::Down | Key::Tab => true,
        }
    }

    // ------------------------------------------------------------------ keyboard

    fn key_down(&mut self, key: Key, repeat: bool) {
        if self.text_edit.is_some() && !(self.mods.command() && matches!(key, Key::Char(_))) {
            self.text_key(&key);
            return;
        }
        if let Some(action) = shortcuts::lookup(&key, self.mods) {
            if !repeat || action.repeatable() {
                self.run(action);
            }
            return;
        }
        if key == Key::Space && !repeat {
            self.space_down = true;
        }
    }

    pub fn run(&mut self, action: shortcuts::Action) {
        use shortcuts::Action as A;
        match action {
            A::Tool(t) => {
                self.end_text_edit();
                self.tool = t;
            }
            A::Escape => {
                if !matches!(self.gesture, Gesture::None) {
                    self.cancel_gesture();
                } else if self.tool != Tool::Select {
                    self.tool = Tool::Select;
                } else {
                    // Select parent, then nothing.
                    let parents: Vec<NodeId> = self
                        .selection
                        .iter()
                        .filter_map(|id| self.doc.get(*id).and_then(|n| n.parent))
                        .collect();
                    self.selection = if parents.is_empty() {
                        Vec::new()
                    } else {
                        dedup(parents)
                    };
                }
            }
            A::EditText => {
                if let [id] = self.selection[..] {
                    if matches!(
                        self.doc.get(id).map(|n| &n.kind),
                        Some(NodeKind::Text { .. })
                    ) {
                        self.start_text_edit(id, None);
                    } else if self.doc.get(id).is_some_and(|n| !n.children.is_empty()) {
                        self.selection = self.doc.get(id).unwrap().children.clone();
                    }
                }
            }
            A::Delete => self.delete_selection(),
            A::Undo => self.undo(),
            A::Redo => self.redo(),
            A::SelectAll => {
                let scope = self
                    .selection
                    .first()
                    .and_then(|id| self.doc.get(*id))
                    .and_then(|n| n.parent);
                self.selection = self
                    .doc
                    .children(scope)
                    .iter()
                    .copied()
                    .filter(|id| self.doc.get(*id).unwrap().visible)
                    .collect();
            }
            A::Copy => self.copy(),
            A::Cut => {
                self.copy();
                self.delete_selection();
            }
            A::Paste => self.paste(v2(PASTE_OFFSET, PASTE_OFFSET)),
            A::Duplicate => {
                let saved = (self.clipboard.clone(), self.clipboard_roots.clone());
                self.copy();
                self.paste(v2(PASTE_OFFSET, PASTE_OFFSET));
                (self.clipboard, self.clipboard_roots) = saved;
            }
            A::Forward => self.reorder(|i, _| i + 1),
            A::Backward => self.reorder(|i, _| i.saturating_sub(1)),
            A::ToFront => self.reorder(|_, len| len),
            A::ToBack => self.reorder(|_, _| 0),
            A::ZoomIn => self.zoom_step(true),
            A::ZoomOut => self.zoom_step(false),
            A::Zoom100 => {
                self.camera.zoom_about(self.viewport * 0.5, 1.0);
                self.revision += 1;
            }
            A::ZoomFit => self.zoom_to_fit(),
            A::ZoomSelection => {
                if let Some(b) = self.selection_bounds() {
                    self.camera.fit(&b, self.viewport, 64.0, 8.0);
                    self.revision += 1;
                }
            }
            A::ToggleVisible => {
                let ids = self.top_selection();
                let show = ids.iter().any(|id| !self.doc.get(*id).unwrap().visible);
                self.edit(if show { "Show" } else { "Hide" }, |tx| {
                    for id in ids {
                        tx.node_mut(id).unwrap().visible = show;
                    }
                });
            }
            A::ToggleLock => {
                let ids = self.top_selection();
                let lock = ids.iter().any(|id| !self.doc.get(*id).unwrap().locked);
                self.edit(if lock { "Lock" } else { "Unlock" }, |tx| {
                    for id in ids {
                        tx.node_mut(id).unwrap().locked = lock;
                    }
                });
                if lock {
                    self.selection.clear();
                }
            }
            A::Nudge(dx, dy) => {
                let ids: Vec<NodeId> = self
                    .top_selection()
                    .into_iter()
                    .filter(|id| !self.doc.is_effectively_locked(*id))
                    .collect();
                if !ids.is_empty() {
                    self.edit_coalescing("Nudge", "nudge", |tx| {
                        for id in ids {
                            let n = tx.node_mut(id).unwrap();
                            n.rect.x += dx;
                            n.rect.y += dy;
                        }
                    });
                }
            }
            A::WrapInFrame => self.wrap_in_frame(),
            A::New => self.requests.push(AppRequest::New),
            A::Open => self.requests.push(AppRequest::Open),
            A::Save => self.requests.push(AppRequest::Save),
            A::SaveAs => self.requests.push(AppRequest::SaveAs),
            A::ExportPng => self.requests.push(AppRequest::ExportPng),
            A::ExportSvg => self.requests.push(AppRequest::ExportSvg),
            A::ToggleSnapping => {
                self.snapping = !self.snapping;
                self.status = format!("Snapping {}", if self.snapping { "on" } else { "off" });
            }
        }
    }

    fn cancel_gesture(&mut self) {
        match std::mem::replace(&mut self.gesture, Gesture::None) {
            Gesture::Move { .. }
            | Gesture::Resize { .. }
            | Gesture::Create { .. }
            | Gesture::Draw { .. }
            | Gesture::Erase => {
                // Roll back the open transaction.
                if let Some((rec, key)) = self.rec.take() {
                    let mut h = History::default();
                    if h.commit(&self.doc, rec, key) {
                        h.undo(&mut self.doc);
                    }
                    self.after_history();
                }
            }
            _ => {}
        }
        self.guides.clear();
        self.marquee = None;
    }

    pub fn zoom_step(&mut self, up: bool) {
        let z = next_zoom_step(self.camera.zoom, up);
        self.camera.zoom_about(self.viewport * 0.5, z);
        self.revision += 1;
    }

    pub fn zoom_to_fit(&mut self) {
        match self.doc.content_bounds() {
            Some(b) => self.camera.fit(&b, self.viewport, 64.0, 1.0),
            None => {
                self.camera = Camera {
                    pan: v2(-self.viewport.x * 0.5, -self.viewport.y * 0.5),
                    zoom: 1.0,
                }
            }
        }
        self.revision += 1;
    }

    pub fn delete_selection(&mut self) {
        let ids: Vec<NodeId> = self
            .top_selection()
            .into_iter()
            .filter(|id| !self.doc.is_effectively_locked(*id))
            .collect();
        if ids.is_empty() {
            return;
        }
        self.end_text_edit();
        self.edit("Delete", |tx| {
            for id in ids {
                tx.remove(id);
            }
        });
        self.selection.clear();
    }

    fn copy(&mut self) {
        let roots = self.top_selection();
        if roots.is_empty() {
            return;
        }
        self.clipboard = roots
            .iter()
            .flat_map(|r| self.doc.subtree(*r))
            .filter_map(|id| self.doc.get(id).cloned())
            .collect();
        self.clipboard_roots = roots;
        self.status = format!("Copied {} layer(s)", self.clipboard_roots.len());
    }

    fn paste(&mut self, offset: V2) {
        if self.clipboard_roots.is_empty() {
            return;
        }
        let nodes = self.clipboard.clone();
        let roots = self.clipboard_roots.clone();
        let new_roots = self.edit("Paste", |tx| {
            let mut map = std::collections::HashMap::new();
            for n in &nodes {
                map.insert(n.id, tx.alloc_id());
            }
            let mut new_roots = Vec::new();
            for r in &roots {
                let n = nodes.iter().find(|n| n.id == *r).unwrap();
                // Paste next to the original if its parent still exists, else at the root.
                let parent = n.parent.filter(|p| tx.doc().nodes.contains_key(p));
                let world = n.rect.translate(tx.doc().parent_origin(n.parent));
                let mut copy = n.clone();
                copy.id = map[&n.id];
                copy.children = n.children.iter().map(|c| map[c]).collect();
                copy.rect = world.translate(offset - tx.doc().parent_origin(parent));
                let id = tx.insert(copy, parent, None);
                for d in nodes
                    .iter()
                    .filter(|d| d.id != n.id && is_desc(&nodes, d.id, n.id))
                {
                    let mut c = d.clone();
                    c.id = map[&d.id];
                    c.parent = d.parent.map(|p| map[&p]);
                    c.children = d.children.iter().map(|x| map[x]).collect();
                    tx.put(c);
                }
                new_roots.push(id);
            }
            new_roots
        });
        self.selection = new_roots;
        self.status = format!("Pasted {} layer(s)", self.selection.len());
    }

    fn reorder(&mut self, f: impl Fn(usize, usize) -> usize) {
        let ids = self.top_selection();
        if ids.is_empty() {
            return;
        }
        self.edit("Reorder", |tx| {
            for id in ids {
                let parent = tx.doc().get(id).and_then(|n| n.parent);
                let list = tx.doc().children(parent);
                let i = list.iter().position(|x| *x == id).unwrap();
                let len = list.len();
                let target = f(i, len - 1).min(len - 1);
                tx.move_to_index(id, target);
            }
        });
    }

    fn wrap_in_frame(&mut self) {
        let ids = self.top_selection();
        let Some(b) = self.selection_bounds() else {
            return;
        };
        let parent = self.doc.get(ids[0]).and_then(|n| n.parent);
        if ids
            .iter()
            .any(|id| self.doc.get(*id).and_then(|n| n.parent) != parent)
        {
            self.status = "Selection must share a parent".into();
            return;
        }
        let index = ids
            .iter()
            .filter_map(|id| self.doc.children(parent).iter().position(|x| x == id))
            .max()
            .map(|i| i + 1);
        let n_frames = self.doc.nodes.values().filter(|n| n.is_frame()).count();
        let frame = self.edit("Frame selection", |tx| {
            let fid = tx.alloc_id();
            let o = tx.doc().parent_origin(parent);
            let mut f = Node::new(
                fid,
                NodeKind::Frame { clip: true },
                b.translate(V2::ZERO - o),
            );
            f.name = format!("Frame {}", n_frames + 1);
            f.style.fill = None;
            tx.insert(f, parent, index);
            for id in &ids {
                tx.reparent(*id, Some(fid), None);
            }
            fid
        });
        self.selection = vec![frame];
    }

    // ------------------------------------------------------------------ programmatic API

    /// Add a node at world position (used by tests, demos and the properties panel).
    pub fn add_node(&mut self, mut node: Node, parent: Option<NodeId>) -> NodeId {
        let label = format!("Add {}", node.kind.type_name().to_lowercase());
        self.begin(&label);
        let id = {
            let mut tx = self.tx();
            node.id = tx.alloc_id();
            tx.insert(node, parent, None)
        };
        self.fit_text(id);
        self.commit();
        id
    }
}

fn is_desc(nodes: &[Node], id: NodeId, ancestor: NodeId) -> bool {
    let mut cur = nodes.iter().find(|n| n.id == id).and_then(|n| n.parent);
    while let Some(p) = cur {
        if p == ancestor {
            return true;
        }
        cur = nodes.iter().find(|n| n.id == p).and_then(|n| n.parent);
    }
    false
}

fn dedup(mut v: Vec<NodeId>) -> Vec<NodeId> {
    let mut seen = std::collections::HashSet::new();
    v.retain(|x| seen.insert(*x));
    v
}

#[cfg(test)]
mod tests;
