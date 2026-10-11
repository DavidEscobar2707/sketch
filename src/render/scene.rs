//! Document + camera → screen-space triangle batches.

use super::mesh::{Batch, MeshBuilder};
use crate::editor::{Editor, hit, snap::Guide};
use crate::geom::{Camera, Rect, V2, v2};
use crate::ink;
use crate::model::{Color, Document, NodeId, NodeKind, TextAlign};
use crate::text;
use lyon::tessellation::{FillRule, LineCap};

pub const CANVAS_BG: Color = Color::rgb(0xee, 0xee, 0xf0);
pub const GRID_DOT: Color = Color::rgb(0xcf, 0xcf, 0xd6);
pub const ACCENT: Color = Color::rgb(0x2f, 0x6f, 0xf3);
pub const GUIDE: Color = Color::rgb(0xf2, 0x2f, 0x7a);

pub fn rgba(c: Color) -> [f32; 4] {
    c.to_f32(1.0)
}

/// What to draw and where.
pub struct View<'a> {
    pub doc: &'a Document,
    pub camera: Camera,
    /// Canvas size in logical px.
    pub viewport: V2,
    /// Canvas top-left in the render target, logical px.
    pub offset: V2,
}

impl View<'_> {
    fn to_screen(&self, p: V2) -> V2 {
        self.camera.world_to_screen(p) + self.offset
    }
    fn rect_to_screen(&self, r: &Rect) -> Rect {
        self.camera.world_rect_to_screen(r).translate(self.offset)
    }
    fn canvas_rect(&self) -> Rect {
        Rect::new(
            self.offset.x,
            self.offset.y,
            self.viewport.x,
            self.viewport.y,
        )
    }
}

/// Dot grid whose spacing adapts to the zoom so it never gets too dense or sparse.
pub fn grid(mb: &mut MeshBuilder, v: &View) {
    let z = v.camera.zoom;
    let mut spacing = 20.0;
    while spacing * z < 14.0 {
        spacing *= 5.0;
    }
    while spacing * z > 70.0 {
        spacing /= 5.0;
    }
    let vis = v.camera.visible_world(v.viewport);
    let x0 = (vis.x / spacing).floor() as i64;
    let y0 = (vis.y / spacing).floor() as i64;
    let x1 = (vis.right() / spacing).ceil() as i64;
    let y1 = (vis.bottom() / spacing).ceil() as i64;
    let color = rgba(GRID_DOT);
    let r = 0.9;
    for yi in y0..=y1 {
        for xi in x0..=x1 {
            let s = v.to_screen(v2(xi as f64 * spacing, yi as f64 * spacing));
            mb.rect(Rect::new(s.x - r, s.y - r, 2.0 * r, 2.0 * r), color);
        }
    }
}

/// Draw the given roots (or the whole document) with their subtrees.
pub fn document(mb: &mut MeshBuilder, v: &View, roots: Option<&[NodeId]>) {
    let all = v.doc.root.clone();
    let roots = roots.unwrap_or(&all);
    let base_clip = Some(v.canvas_rect());
    for id in roots {
        let opacity: f64 = v
            .doc
            .ancestors(*id)
            .iter()
            .filter_map(|a| v.doc.get(*a))
            .map(|n| n.style.opacity)
            .product();
        node(mb, v, *id, base_clip, opacity);
    }
    mb.set_clip(None);
}

fn node(mb: &mut MeshBuilder, v: &View, id: NodeId, clip: Option<Rect>, parent_opacity: f64) {
    let Some(n) = v.doc.get(id) else { return };
    if !n.visible {
        return;
    }
    let Some(wr) = v.doc.world_rect(id) else {
        return;
    };
    let sr = v.rect_to_screen(&wr);
    let opacity = parent_opacity * n.style.opacity;
    let z = v.camera.zoom;
    let visible_area = clip.unwrap_or_else(|| v.canvas_rect());
    let pad = match &n.kind {
        NodeKind::Line { .. } | NodeKind::Stroke { .. } => 8.0 + n.style.stroke_width * z,
        _ => n.style.stroke_width * z,
    };
    let on_screen = sr.inflate(pad).intersects(&visible_area);
    mb.set_clip(clip);
    let fill = n.style.fill.map(|c| c.to_f32(opacity));
    let stroke = n.style.stroke.map(|c| c.to_f32(opacity));
    let sw = (n.style.stroke_width * z) as f32;
    match &n.kind {
        NodeKind::Frame { clip: clips } => {
            if on_screen {
                let path = MeshBuilder::rounded_rect_path(sr, n.style.corner_radius * z);
                if let Some(f) = fill {
                    mb.fill_path(&path, f, FillRule::NonZero);
                }
                if let Some(s) = stroke {
                    mb.stroke_path(&path, sw, s, LineCap::Butt);
                }
            }
            let child_clip = if *clips {
                match clip.map_or(Some(sr), |c| c.intersection(&sr)) {
                    Some(c) => Some(c),
                    None => return,
                }
            } else {
                clip
            };
            if *clips && !on_screen {
                return;
            }
            for c in &n.children {
                node(mb, v, *c, child_clip, opacity);
            }
            mb.set_clip(clip);
        }
        _ if !on_screen => {}
        NodeKind::Rect => {
            let path = MeshBuilder::rounded_rect_path(sr, n.style.corner_radius * z);
            if let Some(f) = fill {
                mb.fill_path(&path, f, FillRule::NonZero);
            }
            if let Some(s) = stroke {
                mb.stroke_path(&path, sw, s, LineCap::Butt);
            }
        }
        NodeKind::Ellipse => {
            let path = MeshBuilder::ellipse_path(sr);
            if let Some(f) = fill {
                mb.fill_path(&path, f, FillRule::NonZero);
            }
            if let Some(s) = stroke {
                mb.stroke_path(&path, sw, s, LineCap::Butt);
            }
        }
        NodeKind::Line { a, b } => {
            if let Some(s) = stroke {
                let path =
                    MeshBuilder::polygon_path(&[sr.min() + *a * z, sr.min() + *b * z], false);
                mb.stroke_path(&path, sw.max(0.5), s, LineCap::Round);
            }
        }
        NodeKind::Text {
            text: t,
            font_size,
            auto_width,
            align,
        } => {
            let color = fill.unwrap_or(rgba(Color::INK));
            draw_text(
                mb,
                t,
                *font_size,
                (!auto_width).then_some(n.rect.w),
                *align,
                sr.min(),
                z,
                color,
            );
        }
        NodeKind::Stroke {
            points,
            width,
            thinning,
        } => {
            let color = fill.unwrap_or(rgba(Color::INK));
            let outline = ink::outline(points, *width, *thinning);
            let pts: Vec<V2> = outline.iter().map(|q| sr.min() + *q * z).collect();
            mb.fill_path(
                &MeshBuilder::polygon_path(&pts, true),
                color,
                FillRule::NonZero,
            );
        }
    }
}

/// Draw text with its box's top-left at `origin` (screen px) at screen scale `z`.
#[allow(clippy::too_many_arguments)]
pub fn draw_text(
    mb: &mut MeshBuilder,
    t: &str,
    font_size: f64,
    wrap: Option<f64>,
    align: TextAlign,
    origin: V2,
    z: f64,
    color: [f32; 4],
) {
    let layout = text::layout(t, font_size, wrap, align);
    let px = font_size * z;
    if px < 2.5 {
        // Too small to read: draw "greeked" bars instead of glyphs.
        let lh = text::line_height(font_size) * z;
        for l in &layout.lines {
            if l.width > 0.0 {
                let x = origin.x + l.carets[0] * z;
                mb.rect(
                    Rect::new(x, origin.y + l.top * z + lh * 0.3, l.width * z, lh * 0.4),
                    [color[0], color[1], color[2], color[3] * 0.35],
                );
            }
        }
        return;
    }
    let s = (font_size / text::face().units_per_em() as f64 * z) as f32;
    for g in &layout.glyphs {
        let m = text::glyph_mesh(g.gid);
        if m.indices.is_empty() {
            continue;
        }
        let o = origin + v2(g.x, g.baseline) * z;
        let (ox, oy) = (o.x as f32, o.y as f32);
        mb.triangles(
            m.vertices.iter().map(|v| [ox + v[0] * s, oy - v[1] * s]),
            &m.indices,
            color,
        );
    }
}

/// Interactive overlay: hover, selection, handles, marquee, guides, labels, caret.
pub fn overlay(mb: &mut MeshBuilder, ed: &Editor, offset: V2) {
    let v = View {
        doc: &ed.doc,
        camera: ed.camera,
        viewport: ed.viewport,
        offset,
    };
    let accent = rgba(ACCENT);
    let z = ed.camera.zoom;
    mb.set_clip(Some(v.canvas_rect()));

    // Frame name labels.
    for (id, r) in ed.frame_labels() {
        let n = ed.doc.get(id).unwrap();
        let selected = ed.selection.contains(&id);
        let color = if selected {
            accent
        } else {
            [0.42, 0.42, 0.46, 1.0]
        };
        let r = r.translate(offset);
        let mut name = n.name.clone();
        let max_w = r.w;
        while !name.is_empty() && text::layout(&name, 12.0, None, TextAlign::Left).width > max_w {
            name.pop();
        }
        draw_text(
            mb,
            &name,
            12.0,
            None,
            TextAlign::Left,
            v2(r.x, r.y + 2.0),
            1.0,
            color,
        );
    }

    if let Some(h) = ed.hover.filter(|h| !ed.selection.contains(h))
        && let Some(wr) = ed.doc.world_rect(h)
    {
        outline_node(mb, &v, h, v.rect_to_screen(&wr), accent, 1.5);
    }

    for id in &ed.selection {
        if let Some(wr) = ed.doc.world_rect(*id) {
            outline_node(mb, &v, *id, v.rect_to_screen(&wr), accent, 1.0);
        }
    }

    if let Some(te) = &ed.text_edit {
        if let (Some(n), Some(wr)) = (ed.doc.get(te.id), ed.doc.world_rect(te.id))
            && let NodeKind::Text {
                text: t,
                font_size,
                auto_width,
                align,
            } = &n.kind
        {
            let l = text::layout(t, *font_size, (!auto_width).then_some(n.rect.w), *align);
            let (x, top, h) = l.caret(te.caret);
            let o = v.to_screen(wr.min());
            mb.rect(
                Rect::new(o.x + x * z - 0.75, o.y + top * z, 1.5, h * z),
                accent,
            );
        }
    } else if let Some(b) = ed.selection_bounds() {
        let sb = v.rect_to_screen(&b);
        if ed.selection.len() > 1 {
            mb.rect_outline(sb, 1.0, accent);
        }
        let tiny = sb.w.min(sb.h) < 24.0;
        for (hx, hy) in hit::HANDLES {
            if tiny && (hx == 0.5 || hy == 0.5) {
                continue;
            }
            let c = v2(sb.x + sb.w * hx, sb.y + sb.h * hy);
            let r = Rect::new(c.x - 4.0, c.y - 4.0, 8.0, 8.0);
            mb.rect(r, [1.0, 1.0, 1.0, 1.0]);
            mb.rect_outline(r, 1.0, accent);
        }
        // Size badge below the selection.
        let label = format!("{} × {}", fmt_num(b.w), fmt_num(b.h));
        let w = text::layout(&label, 11.0, None, TextAlign::Left).width;
        let pill = Rect::new(
            sb.center().x - w * 0.5 - 6.0,
            sb.bottom() + 8.0,
            w + 12.0,
            18.0,
        );
        mb.fill_path(
            &MeshBuilder::rounded_rect_path(pill, 4.0),
            accent,
            FillRule::NonZero,
        );
        draw_text(
            mb,
            &label,
            11.0,
            None,
            TextAlign::Left,
            v2(pill.x + 6.0, pill.y + 2.0),
            1.0,
            [1.0; 4],
        );
    }

    if let Some(m) = ed.marquee {
        let sm = v.rect_to_screen(&m);
        mb.rect(sm, [accent[0], accent[1], accent[2], 0.08]);
        mb.rect_outline(sm, 1.0, accent);
    }

    let guide = rgba(GUIDE);
    for g in &ed.guides {
        match *g {
            Guide::V { x, y0, y1 } => {
                mb.line(v.to_screen(v2(x, y0)), v.to_screen(v2(x, y1)), 1.0, guide)
            }
            Guide::H { y, x0, x1 } => {
                mb.line(v.to_screen(v2(x0, y)), v.to_screen(v2(x1, y)), 1.0, guide)
            }
        }
    }
    mb.set_clip(None);
}

fn outline_node(mb: &mut MeshBuilder, v: &View, id: NodeId, sr: Rect, color: [f32; 4], w: f64) {
    let n = v.doc.get(id).unwrap();
    let z = v.camera.zoom;
    match &n.kind {
        NodeKind::Ellipse => {
            mb.stroke_path(
                &MeshBuilder::ellipse_path(sr),
                w as f32,
                color,
                LineCap::Butt,
            );
        }
        NodeKind::Line { a, b } => {
            mb.line(sr.min() + *a * z, sr.min() + *b * z, w, color);
        }
        NodeKind::Stroke { points, .. } if points.len() > 1 => {
            let pts: Vec<V2> = points.iter().map(|q| sr.min() + q.pos() * z).collect();
            mb.stroke_path(
                &MeshBuilder::polygon_path(&pts, false),
                w as f32,
                color,
                LineCap::Round,
            );
            mb.rect_outline(sr, 1.0, [color[0], color[1], color[2], 0.5]);
        }
        _ => mb.rect_outline(sr, w, color),
    }
}

fn fmt_num(x: f64) -> String {
    let r = (x * 10.0).round() / 10.0;
    if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r:.1}")
    }
}

/// Full interactive canvas: background grid, document, overlay.
pub fn canvas(ed: &Editor, offset: V2, show_grid: bool) -> Vec<Batch> {
    let mut mb = MeshBuilder::default();
    let v = View {
        doc: &ed.doc,
        camera: ed.camera,
        viewport: ed.viewport,
        offset,
    };
    mb.set_clip(Some(v.canvas_rect()));
    mb.rect(v.canvas_rect(), rgba(CANVAS_BG));
    if show_grid {
        grid(&mut mb, &v);
    }
    document(&mut mb, &v, None);
    overlay(&mut mb, ed, offset);
    mb.finish()
}
