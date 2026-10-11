//! egui chrome: menu bar, layers panel, design (properties) panel, floating toolbar, and the
//! projects home screen.

pub mod home;
pub mod inspector;
pub mod pages;

use crate::editor::shortcuts::{self, Action};
use crate::editor::{Editor, Tool};
use crate::model::{Color, NodeId, NodeKind};
use crate::theme::{self, Icon};
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
    /// The window has native chrome: a system menu bar (so no egui menu) and traffic lights
    /// inset into the toolbar / sidebar.
    pub native_chrome: bool,
    /// The sidebar sits over an NSVisualEffectView, so paint it transparent.
    pub translucent_sidebar: bool,
    /// When set, the projects home screen is shown instead of the editor.
    pub home: Option<home::HomeView>,
    /// Name of the open project (shown in the toolbar).
    pub project_name: String,
    /// Autosave state for the toolbar subtitle, e.g. "Saved" or "Saving…".
    pub save_state: String,
    pub pages: pages::PagesUi,
}

pub fn setup_style(ctx: &egui::Context) {
    theme::apply(ctx);
}

pub(crate) fn c32(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}
pub(crate) fn from32(c: Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color::rgba(r, g, b, a)
}

/// Section header, as in a macOS sidebar or inspector: 11pt semibold, secondary colour.
pub(crate) fn section(ui: &mut Ui, title: &str) {
    ui.label(
        RichText::new(title)
            .font(theme::semibold(theme::SECONDARY))
            .color(theme::pal(ui).secondary),
    );
}

pub(crate) fn title_text(text: impl Into<String>) -> RichText {
    RichText::new(text).font(theme::semibold(theme::BODY))
}

/// Lay out the chrome and return the remaining canvas rectangle (logical px).
pub fn draw(ui: &mut Ui, ed: &mut Editor, st: &mut UiState) -> egui::Rect {
    if st.export_scale == 0 {
        st.export_scale = 2;
    }
    if let Some(hv) = &mut st.home {
        home::draw(ui, hv, st.native_chrome, st.translucent_sidebar);
        return egui::Rect::NOTHING;
    }
    let p = theme::pal(ui);
    let sidebar_fill = if st.translucent_sidebar {
        Color32::TRANSPARENT
    } else {
        p.sidebar
    };
    egui::Panel::left("layers")
        .resizable(true)
        .default_size(240.0)
        .size_range(180.0..=420.0)
        .frame(
            egui::Frame::NONE
                .fill(sidebar_fill)
                .inner_margin(egui::Margin::symmetric(10, 8)),
        )
        .show(ui, |ui| {
            if st.native_chrome {
                // Room for the traffic lights, which sit over the top of the sidebar.
                ui.add_space(theme::TOOLBAR_H - 8.0);
            }
            layers(ui, ed, st)
        });
    egui::Panel::top("toolbar")
        .exact_size(if st.native_chrome {
            theme::TOOLBAR_H
        } else {
            34.0
        })
        .frame(
            egui::Frame::NONE
                .fill(p.window)
                .inner_margin(egui::Margin::symmetric(12, 0)),
        )
        .show(ui, |ui| top_bar(ui, ed, st));
    egui::Panel::right("design")
        .resizable(true)
        .default_size(260.0)
        .size_range(220.0..=420.0)
        .frame(
            egui::Frame::NONE
                .fill(p.inspector)
                .inner_margin(egui::Margin::symmetric(14, 10)),
        )
        .show(ui, |ui| inspector::panel(ui, ed, st));
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

/// The unified toolbar: document title on the left, zoom and help on the right. Without native
/// chrome (Linux) it also carries an egui menu bar.
fn top_bar(ui: &mut Ui, ed: &mut Editor, st: &mut UiState) {
    let p = theme::pal(ui);
    ui.horizontal_centered(|ui| {
        if !st.native_chrome {
            menu_bar(ui, ed, st);
            ui.add_space(8.0);
        }
        if theme::icon_button(ui, theme::Icon::Grid, 26.0, false)
            .on_hover_text("All projects")
            .clicked()
        {
            ed.requests.push(crate::editor::AppRequest::Home);
        }
        ui.add_space(4.0);
        let title = if st.project_name.is_empty() {
            ed.doc.name.clone()
        } else {
            st.project_name.clone()
        };
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add_space(if st.native_chrome { 6.0 } else { 1.0 });
            ui.label(title_text(title));
            let state = if !st.save_state.is_empty() {
                st.save_state.clone()
            } else if ed.is_dirty() {
                "Edited".into()
            } else {
                String::new()
            };
            let sub = if state.is_empty() {
                ed.page_name().to_string()
            } else {
                format!("{} — {state}", ed.page_name())
            };
            ui.label(RichText::new(sub).size(theme::SECONDARY).color(p.secondary));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if theme::icon_button(ui, theme::Icon::Question, 26.0, st.show_help)
                .on_hover_text("Keyboard shortcuts")
                .clicked()
            {
                st.show_help = !st.show_help;
            }
            ui.add_space(8.0);
            if theme::icon_button(ui, theme::Icon::Plus, 26.0, false)
                .on_hover_text("Zoom in")
                .clicked()
            {
                ed.run(Action::ZoomIn);
            }
            let z = ui.add(
                egui::Button::new(
                    RichText::new(format!("{:.0}%", ed.camera.zoom * 100.0))
                        .font(theme::medium(theme::SECONDARY + 1.0)),
                )
                .frame(false)
                .min_size(egui::vec2(44.0, 22.0)),
            );
            if z.on_hover_text("Zoom to fit").clicked() {
                ed.run(Action::ZoomFit);
            }
            if theme::icon_button(ui, theme::Icon::Minus, 26.0, false)
                .on_hover_text("Zoom out")
                .clicked()
            {
                ed.run(Action::ZoomOut);
            }
        });
    });
    // 1px separator under the toolbar.
    let r = ui.max_rect();
    ui.painter().hline(
        r.x_range(),
        r.bottom() - 0.5,
        egui::Stroke::new(1.0, p.separator),
    );
}

/// egui menus for platforms without a native menu bar (inline, so the toolbar row keeps
/// the title and zoom controls).
fn menu_bar(ui: &mut Ui, ed: &mut Editor, st: &mut UiState) {
    {
        ui.menu_button("File", |ui| {
            menu_item(ui, ed, "New project", Action::New);
            menu_item(ui, ed, "Import…", Action::Open);
            menu_item(ui, ed, "Save now", Action::Save);
            menu_item(ui, ed, "Duplicate project", Action::SaveAs);
            if ui.button("All projects").clicked() {
                ed.requests.push(crate::editor::AppRequest::Home);
                ui.close();
            }
            ui.separator();
            menu_item(ui, ed, "Export PNG…", Action::ExportPng);
            menu_item(ui, ed, "Export SVG…", Action::ExportSvg);
            export_options(ui, st);
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
    }
}

fn export_options(ui: &mut Ui, st: &mut UiState) {
    ui.horizontal(|ui| {
        ui.label("PNG scale");
        for s in [1u8, 2, 3] {
            ui.selectable_value(&mut st.export_scale, s, format!("{s}x"));
        }
    });
    ui.checkbox(&mut st.export_transparent, "Transparent PNG background");
}

fn layers(ui: &mut Ui, ed: &mut Editor, st: &mut UiState) {
    pages::panel(ui, ed, st);
    ui.add_space(4.0);
    let r = ui.available_rect_before_wrap();
    ui.painter().hline(
        r.x_range(),
        r.top(),
        egui::Stroke::new(1.0, theme::pal(ui).separator),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        section(ui, "Layers");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let some = !ed.selection.is_empty();
            ui.add_enabled_ui(some, |ui| {
                if theme::icon_button(ui, Icon::ChevronDown, 20.0, false)
                    .on_hover_text("Send backward")
                    .clicked()
                {
                    ed.run(Action::Backward);
                }
                if theme::icon_button(ui, Icon::ChevronUp, 20.0, false)
                    .on_hover_text("Bring forward")
                    .clicked()
                {
                    ed.run(Action::Forward);
                }
            });
        });
    });
    ui.add_space(2.0);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            let roots = ed.doc.root.clone();
            if roots.is_empty() {
                ui.label(
                    RichText::new("Nothing here yet. Pick a tool below\nand draw on the canvas.")
                        .size(theme::SECONDARY)
                        .color(theme::pal(ui).secondary),
                );
            }
            for id in roots.iter().rev() {
                layer_row(ui, ed, st, *id, 0);
            }
        });
}

fn node_icon(n: &crate::model::Node) -> Icon {
    if n.component {
        return Icon::Component;
    }
    if n.instance.is_some() && n.parent.is_none_or(|_| n.is_frame()) && n.is_frame() {
        return Icon::Instance;
    }
    if n.style
        .fills
        .iter()
        .any(|p| matches!(p, crate::model::Paint::Image { .. }))
        && matches!(n.kind, NodeKind::Rect)
    {
        return Icon::Image;
    }
    kind_icon(&n.kind)
}

fn kind_icon(k: &NodeKind) -> Icon {
    match k {
        NodeKind::Group => Icon::Group,
        NodeKind::Path { .. } => Icon::Pen,
        NodeKind::Polygon { .. } => Icon::Polygon,
        NodeKind::Star { .. } => Icon::Star,
        NodeKind::Frame { .. } => Icon::Frame,
        NodeKind::Rect => Icon::Rect,
        NodeKind::Ellipse => Icon::Ellipse,
        NodeKind::Line { .. } => Icon::Line,
        NodeKind::Text { .. } => Icon::Text,
        NodeKind::Stroke { .. } => Icon::Ink,
    }
}

/// One sidebar row, drawn like an NSOutlineView row: 24pt tall, 6pt radius selection in the
/// accent colour, subtle hover, disclosure chevron, symbol, name, and eye/lock on hover.
fn layer_row(ui: &mut Ui, ed: &mut Editor, st: &mut UiState, id: NodeId, depth: usize) {
    let Some(n) = ed.doc.get(id).cloned() else {
        return;
    };
    let p = theme::pal(ui);
    let selected = ed.selection.contains(&id);
    let width = ui.available_width();
    let (row, resp) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::click());
    let hovered = ui.rect_contains_pointer(row);
    if selected {
        ui.painter().rect_filled(row, 6.0, theme::accent());
    } else if hovered {
        ui.painter().rect_filled(row, 6.0, p.hover);
    }
    let fg = if selected {
        Color32::WHITE
    } else if n.visible {
        p.text
    } else {
        p.secondary
    };
    let fg2 = if selected {
        Color32::from_white_alpha(200)
    } else {
        p.secondary
    };
    let mut x = row.left() + 4.0 + depth as f32 * 14.0;
    let cy = row.center().y;
    let sq = |x: f32, s: f32| {
        egui::Rect::from_center_size(egui::pos2(x + s / 2.0, cy), egui::vec2(s, s))
    };

    // Disclosure chevron.
    if !n.children.is_empty() {
        let open = !st.collapsed.contains(&id);
        let r = sq(x, 14.0);
        let cr = ui.interact(
            r.expand(2.0),
            ui.id().with(("disclose", id)),
            egui::Sense::click(),
        );
        theme::paint_icon(
            ui.painter(),
            r.shrink(2.0),
            if open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            fg2,
        );
        if cr.clicked() {
            if open {
                st.collapsed.insert(id);
            } else {
                st.collapsed.remove(&id);
            }
        }
    }
    x += 16.0;
    theme::paint_icon(
        ui.painter(),
        sq(x, 14.0),
        node_icon(&n),
        if selected {
            Color32::WHITE
        } else {
            theme::accent()
        },
    );
    x += 20.0;

    // Trailing eye / lock buttons: always shown when hidden or locked, otherwise on hover.
    let mut right = row.right() - 4.0;
    let mut trailing = |ui: &mut Ui, icon: Icon, tip: &str, show: bool| -> bool {
        let r = egui::Rect::from_center_size(egui::pos2(right - 9.0, cy), egui::vec2(18.0, 18.0));
        right -= 20.0;
        if !show {
            return false;
        }
        let b = ui.interact(r, ui.id().with((tip, id)), egui::Sense::click());
        if b.hovered() {
            ui.painter().rect_filled(
                r,
                5.0,
                if selected {
                    Color32::from_white_alpha(40)
                } else {
                    p.hover
                },
            );
        }
        theme::paint_icon(ui.painter(), r.shrink(3.0), icon, fg2);
        b.on_hover_text(tip).clicked()
    };
    let eye = if n.visible { Icon::Eye } else { Icon::EyeSlash };
    if trailing(
        ui,
        eye,
        if n.visible { "Hide" } else { "Show" },
        hovered || !n.visible,
    ) {
        ed.edit(if n.visible { "Hide" } else { "Show" }, |tx| {
            tx.node_mut(id).unwrap().visible = !n.visible
        });
    }
    let lock = if n.locked { Icon::Lock } else { Icon::LockOpen };
    if trailing(
        ui,
        lock,
        if n.locked { "Unlock" } else { "Lock" },
        hovered || n.locked,
    ) {
        ed.edit(if n.locked { "Unlock" } else { "Lock" }, |tx| {
            tx.node_mut(id).unwrap().locked = !n.locked
        });
    }

    // Name, or an inline rename field.
    let name_rect =
        egui::Rect::from_min_max(egui::pos2(x, row.top()), egui::pos2(right, row.bottom()));
    if let Some((rid, name)) = &mut st.renaming
        && *rid == id
    {
        let r = ui.put(
            name_rect.shrink2(egui::vec2(0.0, 2.0)),
            egui::TextEdit::singleline(name),
        );
        r.request_focus();
        if r.lost_focus() {
            let new = name.trim().to_string();
            st.renaming = None;
            if !new.is_empty() && new != n.name {
                ed.edit("Rename", |tx| tx.node_mut(id).unwrap().name = new);
            }
        }
    } else {
        let font = if n.is_frame() {
            theme::medium(theme::BODY)
        } else {
            egui::FontId::proportional(theme::BODY)
        };
        let galley = ui.painter().layout(n.name.clone(), font, fg, f32::INFINITY);
        let clip = ui
            .painter()
            .with_clip_rect(name_rect.intersect(ui.clip_rect()));
        clip.galley(egui::pos2(x, cy - galley.size().y / 2.0), galley, fg);
    }

    if resp.double_clicked() {
        st.renaming = Some((id, n.name.clone()));
    } else if resp.clicked() {
        ed.end_text_edit();
        if ui.input(|i| i.modifiers.shift || i.modifiers.command) {
            if selected {
                ed.selection.retain(|s| *s != id);
            } else {
                ed.selection.push(id);
            }
        } else {
            ed.selection = vec![id];
        }
    }
    if hovered {
        ed.hover = Some(id);
    }
    if !st.collapsed.contains(&id) {
        for c in n.children.iter().rev() {
            layer_row(ui, ed, st, *c, depth + 1);
        }
    }
}

fn tool_icon(t: Tool) -> Icon {
    match t {
        Tool::Select => Icon::Pointer,
        Tool::Hand => Icon::Hand,
        Tool::Frame => Icon::Frame,
        Tool::Rect => Icon::Rect,
        Tool::Ellipse => Icon::Ellipse,
        Tool::Polygon => Icon::Polygon,
        Tool::Star => Icon::Star,
        Tool::Pen => Icon::Pen,
        Tool::Line => Icon::Line,
        Tool::Text => Icon::Text,
        Tool::Pencil => Icon::Pencil,
        Tool::Eraser => Icon::Eraser,
    }
}

/// Floating tool palette at the bottom of the canvas, styled as a macOS popover.
fn toolbar(ctx: &egui::Context, ed: &mut Editor, canvas: egui::Rect) {
    egui::Area::new("toolbar".into())
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(egui::pos2(canvas.center().x, canvas.bottom() - 16.0))
        .show(ctx, |ui| {
            let p = theme::pal(ui);
            egui::Frame::popup(ui.style())
                .fill(p.popover)
                .corner_radius(10.0)
                .inner_margin(5.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for t in Tool::ALL {
                            if matches!(t, Tool::Frame | Tool::Text) {
                                let (r, _) = ui.allocate_exact_size(
                                    egui::vec2(9.0, 28.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().vline(
                                    r.center().x,
                                    r.y_range().shrink(6.0),
                                    egui::Stroke::new(1.0, p.separator),
                                );
                            }
                            if theme::icon_button(ui, tool_icon(t), 30.0, ed.tool == t)
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
                            ui.label(
                                RichText::new(k)
                                    .font(theme::medium(12.0))
                                    .color(theme::pal(ui).secondary),
                            );
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

/// Hand-drawn style controls (filled in with the Excalidraw milestone).
pub(crate) fn sketch_section(
    _ui: &mut Ui,
    _ed: &mut Editor,
    _ids: &[NodeId],
    _first: &crate::model::Node,
    _st: &mut UiState,
) {
}
