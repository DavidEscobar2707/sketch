//! egui chrome: menu bar, layers panel, design (properties) panel, floating toolbar.

use crate::editor::shortcuts::{self, Action};
use crate::editor::{Editor, Tool};
use crate::model::{Color, NodeId, NodeKind};
use egui::{Align2, Color32, RichText, Ui};
use std::collections::HashSet;

#[derive(Default)]
pub struct UiState {
    pub collapsed: HashSet<NodeId>,
    pub renaming: Option<(NodeId, String)>,
    pub show_help: bool,
    pub show_grid_off: bool,
    pub export_scale: u8,
    pub export_transparent: bool,
}

const ACCENT: Color32 = Color32::from_rgb(0x2f, 0x6f, 0xf3);

pub fn setup_style(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::light());
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.visuals.selection.bg_fill = ACCENT;
        s.visuals.panel_fill = Color32::from_rgb(0xfa, 0xfa, 0xfb);
    });
}

fn c32(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}
fn from32(c: Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color::rgba(r, g, b, a)
}

/// Lay out the chrome and return the remaining canvas rectangle (logical px).
pub fn draw(ui: &mut Ui, ed: &mut Editor, st: &mut UiState) -> egui::Rect {
    if st.export_scale == 0 {
        st.export_scale = 2;
    }
    egui::Panel::top("menu").show(ui, |ui| menu_bar(ui, ed, st));
    egui::Panel::left("layers")
        .resizable(true)
        .default_size(240.0)
        .show(ui, |ui| layers(ui, ed, st));
    egui::Panel::right("design")
        .resizable(true)
        .default_size(260.0)
        .show(ui, |ui| design(ui, ed, st));
    let canvas = ui.available_rect_before_wrap();
    toolbar(ui.ctx(), ed, canvas);
    status(ui.ctx(), ed, canvas);
    if st.show_help {
        help(ui.ctx(), st);
    }
    canvas
}

fn menu_item(ui: &mut Ui, ed: &mut Editor, label: &str, action: Action) {
    let hint = shortcuts::help_table()
        .into_iter()
        .find(|(_, d)| shortcuts::lookup_desc(action).is_some_and(|ad| ad == *d))
        .map(|(k, _)| k.split(" / ").next().unwrap_or("").to_string())
        .unwrap_or_default();
    if ui
        .add(egui::Button::new(label).shortcut_text(hint))
        .clicked()
    {
        ed.run(action);
        ui.close();
    }
}

fn menu_bar(ui: &mut Ui, ed: &mut Editor, st: &mut UiState) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            menu_item(ui, ed, "New", Action::New);
            menu_item(ui, ed, "Open…", Action::Open);
            menu_item(ui, ed, "Save", Action::Save);
            menu_item(ui, ed, "Save as…", Action::SaveAs);
            ui.separator();
            menu_item(ui, ed, "Export PNG…", Action::ExportPng);
            menu_item(ui, ed, "Export SVG…", Action::ExportSvg);
            ui.horizontal(|ui| {
                ui.label("PNG scale");
                for s in [1u8, 2, 3] {
                    ui.selectable_value(&mut st.export_scale, s, format!("{s}x"));
                }
            });
            ui.checkbox(&mut st.export_transparent, "Transparent PNG background");
        });
        ui.menu_button("Edit", |ui| {
            menu_item(ui, ed, "Undo", Action::Undo);
            menu_item(ui, ed, "Redo", Action::Redo);
            ui.separator();
            menu_item(ui, ed, "Cut", Action::Cut);
            menu_item(ui, ed, "Copy", Action::Copy);
            menu_item(ui, ed, "Paste", Action::Paste);
            menu_item(ui, ed, "Duplicate", Action::Duplicate);
            menu_item(ui, ed, "Delete", Action::Delete);
            menu_item(ui, ed, "Select all", Action::SelectAll);
            ui.separator();
            menu_item(ui, ed, "Bring forward", Action::Forward);
            menu_item(ui, ed, "Send backward", Action::Backward);
            menu_item(ui, ed, "Bring to front", Action::ToFront);
            menu_item(ui, ed, "Send to back", Action::ToBack);
            menu_item(ui, ed, "Wrap in frame", Action::WrapInFrame);
        });
        ui.menu_button("View", |ui| {
            menu_item(ui, ed, "Zoom in", Action::ZoomIn);
            menu_item(ui, ed, "Zoom out", Action::ZoomOut);
            menu_item(ui, ed, "Zoom to 100%", Action::Zoom100);
            menu_item(ui, ed, "Zoom to fit", Action::ZoomFit);
            menu_item(ui, ed, "Zoom to selection", Action::ZoomSelection);
            ui.separator();
            ui.checkbox(&mut ed.snapping, "Snapping");
            let mut grid = !st.show_grid_off;
            if ui.checkbox(&mut grid, "Dot grid").changed() {
                st.show_grid_off = !grid;
            }
            ui.checkbox(&mut st.show_help, "Keyboard shortcuts");
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("?").on_hover_text("Keyboard shortcuts").clicked() {
                st.show_help = !st.show_help;
            }
            ui.label(format!("{:.0}%", ed.camera.zoom * 100.0));
            let dirty = if ed.is_dirty() { " •" } else { "" };
            let file = ed
                .file_path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "not saved".into());
            ui.label(RichText::new(format!("{}{dirty}  —  {file}", ed.doc.name)).strong());
        });
    });
}

fn layers(ui: &mut Ui, ed: &mut Editor, st: &mut UiState) {
    ui.horizontal(|ui| {
        ui.heading("Layers");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let some = !ed.selection.is_empty();
            if ui
                .add_enabled(some, egui::Button::new("Down").small())
                .on_hover_text("Send backward")
                .clicked()
            {
                ed.run(Action::Backward);
            }
            if ui
                .add_enabled(some, egui::Button::new("Up").small())
                .on_hover_text("Bring forward")
                .clicked()
            {
                ed.run(Action::Forward);
            }
        });
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let roots = ed.doc.root.clone();
            if roots.is_empty() {
                ui.weak("Nothing here yet. Pick a tool below\nand draw on the canvas.");
            }
            for id in roots.iter().rev() {
                layer_row(ui, ed, st, *id, 0);
            }
        });
}

fn layer_row(ui: &mut Ui, ed: &mut Editor, st: &mut UiState, id: NodeId, depth: usize) {
    let Some(n) = ed.doc.get(id).cloned() else {
        return;
    };
    let selected = ed.selection.contains(&id);
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 14.0);
        if n.children.is_empty() {
            ui.add_space(16.0);
        } else {
            let open = !st.collapsed.contains(&id);
            if ui
                .add(
                    egui::Button::new(if open { "-" } else { "+" })
                        .frame(false)
                        .small(),
                )
                .clicked()
            {
                if open {
                    st.collapsed.insert(id);
                } else {
                    st.collapsed.remove(&id);
                }
            }
        }
        let icon = match n.kind {
            NodeKind::Frame { .. } => "#",
            NodeKind::Rect => "R",
            NodeKind::Ellipse => "O",
            NodeKind::Line { .. } => "/",
            NodeKind::Text { .. } => "T",
            NodeKind::Stroke { .. } => "~",
        };
        ui.label(RichText::new(icon).weak());
        if let Some((rid, name)) = &mut st.renaming
            && *rid == id
        {
            let r = ui.add(egui::TextEdit::singleline(name).desired_width(120.0));
            r.request_focus();
            if r.lost_focus() {
                let new = name.trim().to_string();
                st.renaming = None;
                if !new.is_empty() && new != n.name {
                    ed.edit("Rename", |tx| tx.node_mut(id).unwrap().name = new);
                }
            }
            return;
        }
        let mut text = RichText::new(&n.name);
        if !n.visible {
            text = text.weak();
        }
        if n.is_frame() {
            text = text.strong();
        }
        let r = ui.add(egui::Button::selectable(selected, text).frame_when_inactive(false));
        if r.double_clicked() {
            st.renaming = Some((id, n.name.clone()));
        } else if r.clicked() {
            ed.end_text_edit();
            if ui.input(|i| i.modifiers.shift) {
                if selected {
                    ed.selection.retain(|s| *s != id);
                } else {
                    ed.selection.push(id);
                }
            } else {
                ed.selection = vec![id];
            }
        }
        if r.hovered() {
            ed.hover = Some(id);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let lock = if n.locked { "🔒" } else { "🔓" };
            let lr = ui.add(
                egui::Button::new(RichText::new(lock).size(11.0))
                    .frame(false)
                    .small(),
            );
            if lr
                .on_hover_text(if n.locked { "Unlock" } else { "Lock" })
                .clicked()
            {
                ed.edit(if n.locked { "Unlock" } else { "Lock" }, |tx| {
                    tx.node_mut(id).unwrap().locked = !n.locked
                });
            }
            let eye = if n.visible {
                RichText::new("👁").size(11.0)
            } else {
                RichText::new("👁").size(11.0).weak().strikethrough()
            };
            let er = ui.add(egui::Button::new(eye).frame(false).small());
            if er
                .on_hover_text(if n.visible { "Hide" } else { "Show" })
                .clicked()
            {
                ed.edit(if n.visible { "Hide" } else { "Show" }, |tx| {
                    tx.node_mut(id).unwrap().visible = !n.visible
                });
            }
        });
    });
    if !st.collapsed.contains(&id) {
        for c in n.children.iter().rev() {
            layer_row(ui, ed, st, *c, depth + 1);
        }
    }
}

/// Apply a property edit as a coalesced transaction keyed by field + selection.
fn apply(ed: &mut Editor, field: &str, ids: &[NodeId], f: impl Fn(&mut crate::model::Node)) {
    let key = format!("{field}:{ids:?}");
    ed.edit_coalescing(&format!("Change {field}"), &key, |tx| {
        for id in ids {
            if let Some(n) = tx.node_mut(*id) {
                f(n);
            }
        }
    });
    for id in ids {
        ed.fit_text(*id);
    }
    ed.commit();
}

fn seal_on_release(ed: &mut Editor, r: &egui::Response) {
    if r.drag_stopped() || r.lost_focus() {
        ed.history.seal();
    }
}

fn num(
    ui: &mut Ui,
    ed: &mut Editor,
    label: &str,
    value: f64,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
) -> Option<f64> {
    let mut v = value;
    let r = ui
        .horizontal(|ui| {
            ui.label(RichText::new(label).weak());
            ui.add(
                egui::DragValue::new(&mut v)
                    .speed(speed)
                    .range(range)
                    .max_decimals(2),
            )
        })
        .inner;
    seal_on_release(ed, &r);
    (r.changed() && v != value).then_some(v)
}

fn color_row(ui: &mut Ui, ed: &mut Editor, label: &str, c: Option<Color>) -> Option<Option<Color>> {
    let mut out = None;
    ui.horizontal(|ui| {
        let mut on = c.is_some();
        if ui.checkbox(&mut on, label).changed() {
            out = Some(if on {
                Some(c.unwrap_or(Color::rgb(0xd9, 0xd9, 0xd9)))
            } else {
                None
            });
        }
        if let Some(col) = c {
            let mut c2 = c32(col);
            let r = ui.color_edit_button_srgba(&mut c2);
            if r.changed() {
                out = Some(Some(from32(c2)));
            }
            ui.label(RichText::new(col.to_hex()).monospace().weak());
            seal_on_release(ed, &r);
        }
    });
    out
}

fn design(ui: &mut Ui, ed: &mut Editor, _st: &mut UiState) {
    ui.heading("Design");
    ui.separator();
    let ids = ed.top_selection();
    if ids.is_empty() {
        ui.label(RichText::new("Pencil").strong());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Width").weak());
            ui.add(egui::Slider::new(&mut ed.ink_width, 0.5..=24.0));
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Smoothing").weak());
            ui.add(egui::Slider::new(&mut ed.ink_smoothing, 0.0..=1.0));
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Color").weak());
            let mut c = c32(ed.ink_color);
            if ui.color_edit_button_srgba(&mut c).changed() {
                ed.ink_color = from32(c);
            }
        });
        ui.separator();
        ui.label(RichText::new("Document").strong());
        let mut name = ed.doc.name.clone();
        if ui.text_edit_singleline(&mut name).changed() {
            ed.doc.name = name;
            ed.revision += 1;
        }
        ui.label(format!("{} layers", ed.doc.nodes.len()));
        return;
    }
    let first = ed.doc.get(ids[0]).cloned().unwrap();
    if ids.len() > 1 {
        ui.label(format!("{} layers selected", ids.len()));
    } else {
        ui.label(RichText::new(format!("{} · {}", first.kind.type_name(), first.name)).strong());
    }
    ui.separator();

    // Geometry (single selection, coordinates relative to the parent frame).
    if ids.len() == 1 {
        let r = first.rect;
        egui::Grid::new("geom").num_columns(2).show(ui, |ui| {
            if let Some(v) = num(ui, ed, "X", r.x, 1.0, f64::MIN..=f64::MAX) {
                apply(ed, "x", &ids, |n| n.rect.x = v);
            }
            if let Some(v) = num(ui, ed, "Y", r.y, 1.0, f64::MIN..=f64::MAX) {
                apply(ed, "y", &ids, |n| n.rect.y = v);
            }
            ui.end_row();
            if let Some(v) = num(ui, ed, "W", r.w, 1.0, 0.0..=1e6) {
                apply(ed, "width", &ids, |n| {
                    let mut nr = n.rect;
                    nr.w = v;
                    n.set_rect_scaling(nr)
                });
            }
            if let Some(v) = num(ui, ed, "H", r.h, 1.0, 0.0..=1e6)
                && !matches!(first.kind, NodeKind::Text { .. })
            {
                apply(ed, "height", &ids, |n| {
                    let mut nr = n.rect;
                    nr.h = v;
                    n.set_rect_scaling(nr)
                });
            }
            ui.end_row();
        });
        ui.separator();
    }

    // Kind-specific.
    match &first.kind {
        NodeKind::Text {
            text, font_size, ..
        } if ids.len() == 1 => {
            let mut t = text.clone();
            let r = ui.add(
                egui::TextEdit::multiline(&mut t)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            if r.changed() {
                apply(ed, "text", &ids, |n| {
                    if let NodeKind::Text { text, .. } = &mut n.kind {
                        *text = t.clone();
                    }
                });
            }
            seal_on_release(ed, &r);
            if let Some(v) = num(ui, ed, "Font size", *font_size, 0.5, 1.0..=1000.0) {
                apply(ed, "font size", &ids, |n| {
                    if let NodeKind::Text { font_size, .. } = &mut n.kind {
                        *font_size = v;
                    }
                });
            }
        }
        NodeKind::Stroke {
            width, thinning, ..
        } => {
            if let Some(v) = num(ui, ed, "Ink width", *width, 0.1, 0.1..=500.0) {
                apply(ed, "ink width", &ids, |n| {
                    if let NodeKind::Stroke { width, .. } = &mut n.kind {
                        *width = v;
                    }
                });
            }
            if let Some(v) = num(ui, ed, "Pressure", *thinning, 0.01, 0.0..=1.0) {
                apply(ed, "pressure", &ids, |n| {
                    if let NodeKind::Stroke { thinning, .. } = &mut n.kind {
                        *thinning = v;
                    }
                });
            }
        }
        NodeKind::Frame { clip } => {
            let mut c = *clip;
            if ui.checkbox(&mut c, "Clip content").changed() {
                apply(ed, "clip", &ids, |n| n.kind = NodeKind::Frame { clip: c });
            }
        }
        _ => {}
    }

    // Style.
    let label = if matches!(first.kind, NodeKind::Text { .. } | NodeKind::Stroke { .. }) {
        "Color"
    } else {
        "Fill"
    };
    if !matches!(first.kind, NodeKind::Line { .. })
        && let Some(c) = color_row(ui, ed, label, first.style.fill)
    {
        apply(ed, "fill", &ids, |n| n.style.fill = c);
    }
    if !matches!(first.kind, NodeKind::Text { .. } | NodeKind::Stroke { .. }) {
        if let Some(c) = color_row(ui, ed, "Stroke", first.style.stroke) {
            apply(ed, "stroke", &ids, |n| n.style.stroke = c);
        }
        if first.style.stroke.is_some()
            && let Some(v) = num(
                ui,
                ed,
                "Stroke width",
                first.style.stroke_width,
                0.1,
                0.0..=500.0,
            )
        {
            apply(ed, "stroke width", &ids, |n| n.style.stroke_width = v);
        }
    }
    if matches!(first.kind, NodeKind::Rect | NodeKind::Frame { .. })
        && let Some(v) = num(
            ui,
            ed,
            "Radius",
            first.style.corner_radius,
            0.5,
            0.0..=10000.0,
        )
    {
        apply(ed, "radius", &ids, |n| n.style.corner_radius = v);
    }
    let mut op = first.style.opacity * 100.0;
    let r = ui
        .horizontal(|ui| {
            ui.label(RichText::new("Opacity").weak());
            ui.add(egui::Slider::new(&mut op, 0.0..=100.0).suffix("%"))
        })
        .inner;
    if r.changed() {
        apply(ed, "opacity", &ids, |n| n.style.opacity = op / 100.0);
    }
    seal_on_release(ed, &r);
}

fn toolbar(ctx: &egui::Context, ed: &mut Editor, canvas: egui::Rect) {
    egui::Area::new("toolbar".into())
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(egui::pos2(canvas.center().x, canvas.bottom() - 16.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .corner_radius(10.0)
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for t in Tool::ALL {
                            let active = ed.tool == t;
                            let text = RichText::new(t.label()).color(if active {
                                Color32::WHITE
                            } else {
                                Color32::from_gray(40)
                            });
                            let b = egui::Button::new(text)
                                .fill(if active { ACCENT } else { Color32::TRANSPARENT })
                                .corner_radius(6.0);
                            if ui
                                .add(b)
                                .on_hover_text(format!("{} ({})", t.label(), t.shortcut()))
                                .clicked()
                            {
                                ed.run(Action::Tool(t));
                            }
                        }
                    });
                });
        });
}

fn status(ctx: &egui::Context, ed: &Editor, canvas: egui::Rect) {
    if ed.status.is_empty() {
        return;
    }
    egui::Area::new("status".into())
        .pivot(Align2::LEFT_BOTTOM)
        .fixed_pos(egui::pos2(canvas.left() + 12.0, canvas.bottom() - 12.0))
        .interactable(false)
        .show(ctx, |ui| {
            ui.label(RichText::new(&ed.status).weak().small());
        });
}

fn help(ctx: &egui::Context, st: &mut UiState) {
    let mut open = st.show_help;
    egui::Window::new("Keyboard shortcuts")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height(420.0)
                .show(ui, |ui| {
                    egui::Grid::new("keys").striped(true).show(ui, |ui| {
                        for (k, d) in shortcuts::help_table() {
                            ui.label(RichText::new(k).monospace());
                            ui.label(d);
                            ui.end_row();
                        }
                        ui.label(RichText::new("Space+drag").monospace());
                        ui.label("Pan");
                        ui.end_row();
                        ui.label(RichText::new("Wheel / ⌘+Wheel").monospace());
                        ui.label("Pan / zoom");
                        ui.end_row();
                    });
                });
        });
    st.show_help = open;
}
