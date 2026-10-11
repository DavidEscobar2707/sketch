//! Hand-drawn rendering (rough.js port). Filled in with milestone 4.

use super::draw::{DrawOp, node_draw};
use crate::geom::Rect;
use crate::model::Node;
use crate::model::rough::Rough;

pub fn node_ops(n: &Node, wr: Rect, opacity: f64, _r: &Rough) -> Vec<DrawOp> {
    let mut plain = n.clone();
    plain.style.rough = None;
    node_draw(&plain, wr, opacity).ops
}
