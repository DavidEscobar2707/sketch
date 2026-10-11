//! Scene nodes.

use crate::geom::{Rect, V2};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub u64);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// sRGB color with straight alpha. Serialized as `#rrggbbaa`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const INK: Color = Color::rgb(28, 28, 30);
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }
    pub fn from_hex(s: &str) -> Option<Color> {
        let s = s.strip_prefix('#')?;
        let p = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
        match s.len() {
            6 => Some(Color::rgb(p(0)?, p(2)?, p(4)?)),
            8 => Some(Color::rgba(p(0)?, p(2)?, p(4)?, p(6)?)),
            _ => None,
        }
    }
    /// Normalized RGBA with the alpha multiplied by `opacity`.
    pub fn to_f32(self, opacity: f64) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            (self.a as f64 / 255.0 * opacity) as f32,
        ]
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::from_hex(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid color {s:?}")))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: f64,
    pub corner_radius: f64,
    pub opacity: f64,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            fill: None,
            stroke: None,
            stroke_width: 1.0,
            corner_radius: 0.0,
            opacity: 1.0,
        }
    }
}

/// One sampled input point of a freehand stroke, in node-local coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StrokePoint {
    pub x: f64,
    pub y: f64,
    /// 0..=1. Real pen pressure when available, otherwise simulated from velocity.
    pub p: f64,
}

impl StrokePoint {
    pub fn pos(&self) -> V2 {
        V2 {
            x: self.x,
            y: self.y,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum NodeKind {
    /// Artboard-like container; children are positioned relative to it.
    Frame {
        #[serde(default = "yes")]
        clip: bool,
    },
    Rect,
    Ellipse,
    /// Straight line between two node-local points.
    Line {
        a: V2,
        b: V2,
    },
    Text {
        text: String,
        font_size: f64,
        /// When true the box grows to fit the text; otherwise it wraps at `rect.w`.
        #[serde(default = "yes")]
        auto_width: bool,
        #[serde(default)]
        align: TextAlign,
    },
    /// Freehand ink. Points are node-local; the outline is computed on render.
    Stroke {
        points: Vec<StrokePoint>,
        /// Nominal diameter at pressure 0.5.
        width: f64,
        /// How much pressure modulates width (0 = uniform width).
        #[serde(default = "default_thinning")]
        thinning: f64,
    },
}

fn yes() -> bool {
    true
}
fn default_thinning() -> f64 {
    0.6
}

impl NodeKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            NodeKind::Frame { .. } => "Frame",
            NodeKind::Rect => "Rectangle",
            NodeKind::Ellipse => "Ellipse",
            NodeKind::Line { .. } => "Line",
            NodeKind::Text { .. } => "Text",
            NodeKind::Stroke { .. } => "Drawing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    #[serde(flatten)]
    pub kind: NodeKind,
    /// Bounds relative to the parent frame's origin (or the world for root nodes).
    pub rect: Rect,
    #[serde(default)]
    pub style: Style,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<NodeId>,
    /// Paint order, bottom to top.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<NodeId>,
}

impl Node {
    pub fn new(id: NodeId, kind: NodeKind, rect: Rect) -> Node {
        let style = match &kind {
            NodeKind::Frame { .. } => Style {
                fill: Some(Color::WHITE),
                ..Style::default()
            },
            NodeKind::Rect => Style {
                fill: Some(Color::rgb(0xd9, 0xd9, 0xd9)),
                ..Style::default()
            },
            NodeKind::Ellipse => Style {
                fill: Some(Color::rgb(0xd9, 0xd9, 0xd9)),
                ..Style::default()
            },
            NodeKind::Line { .. } => Style {
                stroke: Some(Color::INK),
                stroke_width: 2.0,
                ..Style::default()
            },
            NodeKind::Text { .. } => Style {
                fill: Some(Color::INK),
                ..Style::default()
            },
            NodeKind::Stroke { .. } => Style {
                fill: Some(Color::INK),
                ..Style::default()
            },
        };
        Node {
            id,
            name: kind.type_name().to_string(),
            kind,
            rect,
            style,
            visible: true,
            locked: false,
            parent: None,
            children: Vec::new(),
        }
    }

    pub fn is_frame(&self) -> bool {
        matches!(self.kind, NodeKind::Frame { .. })
    }

    /// Change the local bounds, scaling intrinsic geometry (ink, lines) to match.
    pub fn set_rect_scaling(&mut self, new: Rect) {
        let old = self.rect;
        let sx = if old.w.abs() > 1e-9 {
            new.w / old.w
        } else {
            1.0
        };
        let sy = if old.h.abs() > 1e-9 {
            new.h / old.h
        } else {
            1.0
        };
        match &mut self.kind {
            NodeKind::Stroke { points, .. } => {
                for p in points.iter_mut() {
                    p.x *= sx;
                    p.y *= sy;
                }
            }
            NodeKind::Line { a, b } => {
                a.x *= sx;
                a.y *= sy;
                b.x *= sx;
                b.y *= sy;
            }
            NodeKind::Text { auto_width, .. } if (new.w - old.w).abs() > 1e-9 => {
                *auto_width = false;
            }
            _ => {}
        }
        self.rect = new;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_hex_roundtrip() {
        for c in [Color::rgb(1, 2, 255), Color::rgba(0x12, 0x34, 0x56, 0x78)] {
            assert_eq!(Color::from_hex(&c.to_hex()), Some(c));
        }
        assert_eq!(Color::from_hex("#abc"), None);
        assert_eq!(Color::from_hex("ffffff"), None);
    }

    #[test]
    fn scaling_rect_scales_ink() {
        let mut n = Node::new(
            NodeId(1),
            NodeKind::Stroke {
                points: vec![StrokePoint {
                    x: 10.0,
                    y: 20.0,
                    p: 0.5,
                }],
                width: 4.0,
                thinning: 0.5,
            },
            Rect::new(0.0, 0.0, 100.0, 100.0),
        );
        n.set_rect_scaling(Rect::new(5.0, 5.0, 200.0, 50.0));
        let NodeKind::Stroke { points, .. } = &n.kind else {
            unreachable!()
        };
        assert_eq!((points[0].x, points[0].y), (20.0, 10.0));
    }

    #[test]
    fn node_json_shape() {
        let n = Node::new(
            NodeId(7),
            NodeKind::Frame { clip: true },
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"type\":\"frame\""), "{s}");
        assert!(s.contains("\"fill\":\"#ffffff\""), "{s}");
        let back: Node = serde_json::from_str(&s).unwrap();
        assert_eq!(back, n);
    }
}
