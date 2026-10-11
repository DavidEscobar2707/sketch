//! SVG export. Text is emitted as glyph outlines so the file renders identically without fonts.

use crate::geom::{Rect, V2, v2};
use crate::ink;
use crate::model::{Color, Document, NodeId, NodeKind};
use crate::text;
use std::fmt::Write;

fn paint(attr: &str, c: Option<Color>) -> String {
    match c {
        None => format!(" {attr}=\"none\""),
        Some(c) if c.a == 255 => format!(" {attr}=\"#{:02x}{:02x}{:02x}\"", c.r, c.g, c.b),
        Some(c) => format!(
            " {attr}=\"#{:02x}{:02x}{:02x}\" {attr}-opacity=\"{:.3}\"",
            c.r,
            c.g,
            c.b,
            c.a as f64 / 255.0
        ),
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn n(x: f64) -> String {
    let r = (x * 100.0).round() / 100.0;
    if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

/// Bounds used for export: node rects plus stroke overhang.
pub fn export_bounds(doc: &Document, roots: &[NodeId]) -> Option<Rect> {
    let rects: Vec<Rect> = roots
        .iter()
        .filter(|id| doc.get(**id).is_some_and(|n| n.visible))
        .filter_map(|id| {
            let n = doc.get(*id)?;
            let pad = match n.kind {
                NodeKind::Frame { .. } | NodeKind::Text { .. } | NodeKind::Stroke { .. } => {
                    if n.style.stroke.is_some() {
                        n.style.stroke_width * 0.5
                    } else {
                        0.0
                    }
                }
                _ => n.style.stroke_width * 0.5,
            };
            Some(doc.world_rect(*id)?.inflate(pad))
        })
        .collect();
    Rect::union_all(&rects).map(|r| {
        Rect::new(
            r.x.floor(),
            r.y.floor(),
            (r.right().ceil() - r.x.floor()).max(1.0),
            (r.bottom().ceil() - r.y.floor()).max(1.0),
        )
    })
}

/// Export the given roots (or all top-level nodes) to an SVG string.
pub fn export(
    doc: &Document,
    roots: Option<&[NodeId]>,
    background: Option<Color>,
) -> Option<String> {
    let roots: Vec<NodeId> = roots
        .map(|r| r.to_vec())
        .unwrap_or_else(|| doc.root.clone());
    let b = export_bounds(doc, &roots)?;
    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="{} {} {} {}">"#,
        n(b.w),
        n(b.h),
        n(b.x),
        n(b.y),
        n(b.w),
        n(b.h)
    );
    let _ = writeln!(s, "<title>{}</title>", esc(&doc.name));
    if let Some(bg) = background {
        let _ = writeln!(
            s,
            r#"<rect x="{}" y="{}" width="{}" height="{}"{}/>"#,
            n(b.x),
            n(b.y),
            n(b.w),
            n(b.h),
            paint("fill", Some(bg))
        );
    }
    for id in roots {
        node(doc, id, &mut s, 0);
    }
    s.push_str("</svg>\n");
    Some(s)
}

fn node(doc: &Document, id: NodeId, s: &mut String, depth: usize) {
    let Some(nd) = doc.get(id) else { return };
    if !nd.visible {
        return;
    }
    let r = doc.world_rect(id).unwrap();
    let ind = "  ".repeat(depth + 1);
    let opacity = if nd.style.opacity < 1.0 {
        format!(" opacity=\"{}\"", n(nd.style.opacity))
    } else {
        String::new()
    };
    let stroke = if nd.style.stroke.is_some() {
        format!(
            "{} stroke-width=\"{}\"",
            paint("stroke", nd.style.stroke),
            n(nd.style.stroke_width)
        )
    } else {
        String::new()
    };
    let rx = if nd.style.corner_radius > 0.0 {
        format!(" rx=\"{}\"", n(nd.style.corner_radius))
    } else {
        String::new()
    };
    let label = esc(&nd.name);
    match &nd.kind {
        NodeKind::Frame { clip } => {
            let _ = writeln!(
                s,
                "{ind}<g id=\"n{}\" data-name=\"{label}\"{opacity}>",
                id.0
            );
            let _ = writeln!(
                s,
                "{ind}  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{rx}{}{stroke}/>",
                n(r.x),
                n(r.y),
                n(r.w),
                n(r.h),
                paint("fill", nd.style.fill)
            );
            if *clip {
                let _ = writeln!(
                    s,
                    "{ind}  <clipPath id=\"clip{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{rx}/></clipPath>",
                    id.0,
                    n(r.x),
                    n(r.y),
                    n(r.w),
                    n(r.h)
                );
                let _ = writeln!(s, "{ind}  <g clip-path=\"url(#clip{})\">", id.0);
            }
            for c in &nd.children {
                node(doc, *c, s, depth + 2);
            }
            if *clip {
                let _ = writeln!(s, "{ind}  </g>");
            }
            let _ = writeln!(s, "{ind}</g>");
        }
        NodeKind::Rect => {
            let _ = writeln!(
                s,
                "{ind}<rect data-name=\"{label}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{rx}{}{stroke}{opacity}/>",
                n(r.x),
                n(r.y),
                n(r.w),
                n(r.h),
                paint("fill", nd.style.fill)
            );
        }
        NodeKind::Ellipse => {
            let c = r.center();
            let _ = writeln!(
                s,
                "{ind}<ellipse data-name=\"{label}\" cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{}{stroke}{opacity}/>",
                n(c.x),
                n(c.y),
                n(r.w * 0.5),
                n(r.h * 0.5),
                paint("fill", nd.style.fill)
            );
        }
        NodeKind::Line { a, b } => {
            let (a, b) = (r.min() + *a, r.min() + *b);
            let _ = writeln!(
                s,
                "{ind}<line data-name=\"{label}\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{} stroke-width=\"{}\" stroke-linecap=\"round\"{opacity}/>",
                n(a.x),
                n(a.y),
                n(b.x),
                n(b.y),
                paint("stroke", nd.style.stroke.or(Some(Color::INK))),
                n(nd.style.stroke_width)
            );
        }
        NodeKind::Text {
            text: t,
            font_size,
            auto_width,
            align,
        } => {
            let l = text::layout(t, *font_size, (!auto_width).then_some(nd.rect.w), *align);
            let _ = writeln!(
                s,
                "{ind}<g data-name=\"{label}\" aria-label=\"{}\"{}{opacity}>",
                esc(t),
                paint("fill", nd.style.fill.or(Some(Color::INK)))
            );
            for g in &l.glyphs {
                let d = text::glyph_svg_path(g.gid, r.min() + v2(g.x, g.baseline), *font_size);
                if !d.is_empty() {
                    let _ = writeln!(s, "{ind}  <path d=\"{d}\"/>");
                }
            }
            let _ = writeln!(s, "{ind}</g>");
        }
        NodeKind::Stroke {
            points,
            width,
            thinning,
        } => {
            let o = ink::outline(points, *width, *thinning);
            let mut d = String::new();
            for (i, q) in o.iter().enumerate() {
                let q: V2 = r.min() + *q;
                let _ = write!(d, "{}{} {}", if i == 0 { "M" } else { "L" }, n(q.x), n(q.y));
            }
            d.push('Z');
            let _ = writeln!(
                s,
                "{ind}<path data-name=\"{label}\" d=\"{d}\"{}{opacity}/>",
                paint("fill", nd.style.fill.or(Some(Color::INK)))
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    #[test]
    fn svg_is_well_formed_and_contains_all_kinds() {
        let ed = demo::showcase();
        let svg = export(&ed.doc, None, Some(Color::WHITE)).unwrap();
        for tag in [
            "<svg", "<rect", "<ellipse", "<line", "<path", "clipPath", "</svg>",
        ] {
            assert!(svg.contains(tag), "missing {tag}");
        }
        // Balanced <g> groups.
        assert_eq!(svg.matches("<g").count(), svg.matches("</g>").count());
        assert!(!svg.contains("NaN"));
    }

    #[test]
    fn escapes_names() {
        let mut ed = crate::editor::Editor::default();
        let mut n =
            crate::model::Node::new(NodeId(0), NodeKind::Rect, Rect::new(0.0, 0.0, 5.0, 5.0));
        n.name = "<a & \"b\">".into();
        ed.add_node(n, None);
        let svg = export(&ed.doc, None, None).unwrap();
        assert!(svg.contains("&lt;a &amp; &quot;b&quot;&gt;"));
    }
}
