//! Text layout and glyph outlines.
//!
//! Text is rendered as vector glyph outlines, so it stays crisp at every zoom level and
//! exports to SVG without font dependencies. The face is egui's bundled Ubuntu Light (OFL/UFL),
//! not anything taken from Paper.

use crate::geom::{V2, v2};
use crate::model::TextAlign;
use lyon::path::Path;
use lyon::tessellation::{BuffersBuilder, FillOptions, FillTessellator, FillVertex, VertexBuffers};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub static FONT_BYTES: &[u8] = epaint_default_fonts::UBUNTU_LIGHT;

pub fn face() -> &'static ttf_parser::Face<'static> {
    static FACE: OnceLock<ttf_parser::Face<'static>> = OnceLock::new();
    FACE.get_or_init(|| ttf_parser::Face::parse(FONT_BYTES, 0).expect("bundled font parses"))
}

fn units_per_em() -> f64 {
    face().units_per_em() as f64
}

pub fn line_height(font_size: f64) -> f64 {
    let f = face();
    let h = (f.ascender() as f64 - f.descender() as f64 + f.line_gap() as f64) / units_per_em();
    font_size * h.max(1.1)
}

pub fn ascent(font_size: f64) -> f64 {
    face().ascender() as f64 / units_per_em() * font_size
}

#[derive(Debug, Clone, Copy)]
pub struct PlacedGlyph {
    pub gid: u16,
    /// Pen position of the glyph origin (baseline), relative to the text box.
    pub x: f64,
    pub baseline: f64,
    /// Char index into the source string this glyph comes from.
    pub char_index: usize,
}

#[derive(Debug, Clone)]
pub struct Line {
    pub top: f64,
    pub width: f64,
    /// Char index range `[start, end)`; `end` excludes the line break.
    pub start: usize,
    pub end: usize,
    /// Left x of each char boundary in the line (len = end - start + 1).
    pub carets: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct TextLayout {
    pub glyphs: Vec<PlacedGlyph>,
    pub lines: Vec<Line>,
    pub width: f64,
    pub height: f64,
    pub font_size: f64,
}

fn advance(c: char, font_size: f64) -> (u16, f64) {
    let f = face();
    let gid = f
        .glyph_index(c)
        .or_else(|| f.glyph_index('?'))
        .map(|g| g.0)
        .unwrap_or(0);
    let adv = f.glyph_hor_advance(ttf_parser::GlyphId(gid)).unwrap_or(0) as f64;
    (gid, adv / units_per_em() * font_size)
}

/// Lay out text with optional wrapping at `max_width` and horizontal alignment within `box_width`.
pub fn layout(text: &str, font_size: f64, max_width: Option<f64>, align: TextAlign) -> TextLayout {
    let lh = line_height(font_size);
    let chars: Vec<char> = text.chars().collect();
    // Break into lines: hard breaks on '\n', soft breaks at spaces when wrapping.
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    while start <= chars.len() {
        let hard_end = chars[start..]
            .iter()
            .position(|c| *c == '\n')
            .map_or(chars.len(), |p| start + p);
        let mut line_start = start;
        if let Some(maxw) = max_width {
            let mut x = 0.0;
            let mut last_space: Option<usize> = None;
            let mut i = line_start;
            while i < hard_end {
                let (_, a) = advance(chars[i], font_size);
                if chars[i] == ' ' {
                    last_space = Some(i);
                }
                if x + a > maxw && i > line_start {
                    let brk = match last_space {
                        Some(s) if s > line_start => s,
                        _ => i,
                    };
                    let skip_space = chars.get(brk) == Some(&' ');
                    ranges.push((line_start, brk));
                    line_start = if skip_space { brk + 1 } else { brk };
                    i = line_start;
                    x = 0.0;
                    last_space = None;
                    continue;
                }
                x += a;
                i += 1;
            }
        }
        ranges.push((line_start, hard_end));
        start = hard_end + 1;
    }

    let mut lines = Vec::new();
    let mut glyphs = Vec::new();
    let mut max_line_w: f64 = 0.0;
    for (li, (s, e)) in ranges.iter().copied().enumerate() {
        let mut x = 0.0;
        let mut carets = vec![0.0];
        for (i, c) in chars.iter().enumerate().take(e).skip(s) {
            let (gid, a) = advance(*c, font_size);
            glyphs.push(PlacedGlyph {
                gid,
                x,
                baseline: li as f64 * lh + ascent(font_size),
                char_index: i,
            });
            x += a;
            carets.push(x);
        }
        max_line_w = max_line_w.max(x);
        lines.push(Line {
            top: li as f64 * lh,
            width: x,
            start: s,
            end: e,
            carets,
        });
    }
    let box_w = max_width.unwrap_or(max_line_w);
    if align != TextAlign::Left {
        let k = if align == TextAlign::Center { 0.5 } else { 1.0 };
        for line in &mut lines {
            let dx = (box_w - line.width) * k;
            for c in &mut line.carets {
                *c += dx;
            }
            for g in glyphs
                .iter_mut()
                .filter(|g| g.char_index >= line.start && g.char_index < line.end)
            {
                g.x += dx;
            }
        }
    }
    TextLayout {
        glyphs,
        height: lines.len() as f64 * lh,
        lines,
        width: max_line_w,
        font_size,
    }
}

impl TextLayout {
    /// Caret (x, top, height) for a char index into the source string.
    pub fn caret(&self, index: usize) -> (f64, f64, f64) {
        let lh = line_height(self.font_size);
        for (i, l) in self.lines.iter().enumerate() {
            let next_start = self.lines.get(i + 1).map(|n| n.start);
            if index >= l.start && index <= l.end && next_start.is_none_or(|ns| index < ns) {
                return (l.carets[index - l.start], l.top, lh);
            }
        }
        let l = self.lines.last().unwrap();
        (*l.carets.last().unwrap(), l.top, lh)
    }

    /// Char index nearest to a local point.
    pub fn hit(&self, p: V2) -> usize {
        let lh = line_height(self.font_size);
        let li = ((p.y / lh).floor().max(0.0) as usize).min(self.lines.len() - 1);
        let l = &self.lines[li];
        let mut best = 0;
        for (i, x) in l.carets.iter().enumerate() {
            if (x - p.x).abs() < (l.carets[best] - p.x).abs() {
                best = i;
            }
        }
        l.start + best
    }
}

/// A glyph tessellated once in font units (y up).
pub struct GlyphMesh {
    pub vertices: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

pub fn glyph_mesh(gid: u16) -> Arc<GlyphMesh> {
    static CACHE: OnceLock<Mutex<HashMap<u16, Arc<GlyphMesh>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(m) = cache.lock().unwrap().get(&gid) {
        return m.clone();
    }
    let mesh = Arc::new(tessellate_glyph(gid));
    cache.lock().unwrap().insert(gid, mesh.clone());
    mesh
}

fn tessellate_glyph(gid: u16) -> GlyphMesh {
    let mut b = LyonOutline {
        b: Path::builder(),
        open: false,
    };
    face().outline_glyph(ttf_parser::GlyphId(gid), &mut b);
    if b.open {
        b.b.end(true);
    }
    let path = b.b.build();
    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    let tol = (units_per_em() / 2000.0) as f32;
    let _ = FillTessellator::new().tessellate_path(
        &path,
        &FillOptions::tolerance(tol).with_fill_rule(lyon::tessellation::FillRule::NonZero),
        &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| v.position().to_array()),
    );
    GlyphMesh {
        vertices: buffers.vertices,
        indices: buffers.indices,
    }
}

struct LyonOutline {
    b: lyon::path::path::Builder,
    open: bool,
}

impl ttf_parser::OutlineBuilder for LyonOutline {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.open {
            self.b.end(true);
        }
        self.b.begin(lyon::math::point(x, y));
        self.open = true;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.b.line_to(lyon::math::point(x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.b
            .quadratic_bezier_to(lyon::math::point(x1, y1), lyon::math::point(x, y));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.b.cubic_bezier_to(
            lyon::math::point(x1, y1),
            lyon::math::point(x2, y2),
            lyon::math::point(x, y),
        );
    }
    fn close(&mut self) {
        self.b.end(true);
        self.open = false;
    }
}

/// SVG path data for a glyph placed at `origin` (baseline) with `font_size`, y down.
pub fn glyph_svg_path(gid: u16, origin: V2, font_size: f64) -> String {
    struct Svg {
        d: String,
        o: V2,
        s: f64,
    }
    impl Svg {
        fn p(&self, x: f32, y: f32) -> String {
            let q = self.o + v2(x as f64 * self.s, -(y as f64) * self.s);
            format!("{:.2} {:.2}", q.x, q.y)
        }
    }
    impl ttf_parser::OutlineBuilder for Svg {
        fn move_to(&mut self, x: f32, y: f32) {
            let p = self.p(x, y);
            self.d += &format!("M{p}");
        }
        fn line_to(&mut self, x: f32, y: f32) {
            let p = self.p(x, y);
            self.d += &format!("L{p}");
        }
        fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
            let (a, b) = (self.p(x1, y1), self.p(x, y));
            self.d += &format!("Q{a} {b}");
        }
        fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
            let (a, b, c) = (self.p(x1, y1), self.p(x2, y2), self.p(x, y));
            self.d += &format!("C{a} {b} {c}");
        }
        fn close(&mut self) {
            self.d += "Z";
        }
    }
    let mut s = Svg {
        d: String::new(),
        o: origin,
        s: font_size / units_per_em(),
    };
    face().outline_glyph(ttf_parser::GlyphId(gid), &mut s);
    s.d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_basic_and_newlines() {
        let l = layout("Hello\nWorld!", 20.0, None, TextAlign::Left);
        assert_eq!(l.lines.len(), 2);
        assert!(l.width > 40.0 && l.width < 120.0, "{}", l.width);
        assert!((l.height - 2.0 * line_height(20.0)).abs() < 1e-9);
        assert_eq!(l.glyphs.len(), 11);
        assert_eq!(l.caret(0).0, 0.0);
        assert_eq!(l.caret(6).1, line_height(20.0));
        let empty = layout("", 20.0, None, TextAlign::Left);
        assert_eq!(empty.lines.len(), 1);
        assert_eq!(empty.caret(0), (0.0, 0.0, line_height(20.0)));
    }

    #[test]
    fn wrapping_respects_width() {
        let l = layout(
            "the quick brown fox jumps over the lazy dog",
            16.0,
            Some(80.0),
            TextAlign::Left,
        );
        assert!(l.lines.len() >= 3);
        for line in &l.lines {
            assert!(
                line.width <= 80.0 + 1e-6 || line.end - line.start == 1,
                "{line:?}"
            );
        }
    }

    #[test]
    fn hit_and_caret_agree() {
        let l = layout("abc def", 30.0, None, TextAlign::Left);
        for i in 0..=7 {
            let (x, top, h) = l.caret(i);
            assert_eq!(l.hit(v2(x, top + h * 0.5)), i);
        }
    }

    #[test]
    fn glyphs_tessellate() {
        let (gid, _) = advance('A', 10.0);
        let m = glyph_mesh(gid);
        assert!(!m.indices.is_empty());
        assert!(glyph_svg_path(gid, V2::ZERO, 10.0).starts_with('M'));
    }
}
