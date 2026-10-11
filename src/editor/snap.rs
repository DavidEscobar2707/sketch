//! Alignment snapping between bounding boxes.

use crate::geom::Rect;

/// Snap distance in screen pixels.
pub const SNAP_PX: f64 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Guide {
    /// Vertical line at world x spanning y0..y1.
    V { x: f64, y0: f64, y1: f64 },
    /// Horizontal line at world y spanning x0..x1.
    H { y: f64, x0: f64, x1: f64 },
}

fn anchors_x(r: &Rect) -> [f64; 3] {
    [r.x, r.x + r.w * 0.5, r.right()]
}
fn anchors_y(r: &Rect) -> [f64; 3] {
    [r.y, r.y + r.h * 0.5, r.bottom()]
}

/// Best correction that brings any of `moving` onto any of `targets` within `threshold`.
fn best_offset(
    moving: &[f64],
    targets: impl Iterator<Item = f64> + Clone,
    threshold: f64,
) -> Option<f64> {
    let mut best: Option<f64> = None;
    for m in moving {
        for t in targets.clone() {
            let d = t - m;
            if d.abs() <= threshold && best.is_none_or(|b| d.abs() < b.abs()) {
                best = Some(d);
            }
        }
    }
    best
}

/// Snap a moving rectangle. `use_x/use_y` select which anchors of `moving` may snap per axis
/// (left, center, right / top, middle, bottom). Returns the correction and the guides.
pub fn snap_rect(
    moving: &Rect,
    use_x: [bool; 3],
    use_y: [bool; 3],
    candidates: &[Rect],
    threshold: f64,
) -> (f64, f64, Vec<Guide>) {
    let mx: Vec<f64> = anchors_x(moving)
        .into_iter()
        .zip(use_x)
        .filter(|(_, u)| *u)
        .map(|(v, _)| v)
        .collect();
    let my: Vec<f64> = anchors_y(moving)
        .into_iter()
        .zip(use_y)
        .filter(|(_, u)| *u)
        .map(|(v, _)| v)
        .collect();
    let dx = best_offset(&mx, candidates.iter().flat_map(anchors_x), threshold).unwrap_or(0.0);
    let dy = best_offset(&my, candidates.iter().flat_map(anchors_y), threshold).unwrap_or(0.0);
    let snapped = Rect::new(moving.x + dx, moving.y + dy, moving.w, moving.h);
    let mut guides = Vec::new();
    let eps = 1e-6;
    if dx != 0.0
        || mx.iter().any(|m| {
            candidates
                .iter()
                .flat_map(anchors_x)
                .any(|t| (t - m).abs() < eps)
        })
    {
        for (v, used) in anchors_x(&snapped).into_iter().zip(use_x) {
            if !used {
                continue;
            }
            for c in candidates
                .iter()
                .filter(|c| anchors_x(c).iter().any(|t| (t - v).abs() < eps))
            {
                let u = snapped.union(c);
                guides.push(Guide::V {
                    x: v,
                    y0: u.y,
                    y1: u.bottom(),
                });
            }
        }
    }
    if dy != 0.0
        || my.iter().any(|m| {
            candidates
                .iter()
                .flat_map(anchors_y)
                .any(|t| (t - m).abs() < eps)
        })
    {
        for (v, used) in anchors_y(&snapped).into_iter().zip(use_y) {
            if !used {
                continue;
            }
            for c in candidates
                .iter()
                .filter(|c| anchors_y(c).iter().any(|t| (t - v).abs() < eps))
            {
                let u = snapped.union(c);
                guides.push(Guide::H {
                    y: v,
                    x0: u.x,
                    x1: u.right(),
                });
            }
        }
    }
    (dx, dy, guides)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_left_edge_and_centers() {
        let others = [Rect::new(100.0, 0.0, 50.0, 50.0)];
        let m = Rect::new(103.0, 200.0, 10.0, 10.0);
        let (dx, dy, g) = snap_rect(&m, [true; 3], [true; 3], &others, 6.0);
        assert_eq!((dx, dy), (-3.0, 0.0));
        assert!(
            matches!(g[0], Guide::V { x, y0, y1 } if x == 100.0 && y0 == 0.0 && y1 == 210.0),
            "{g:?}"
        );

        let m = Rect::new(114.0, 200.0, 20.0, 20.0); // center 124 -> 125
        let (dx, _, _) = snap_rect(&m, [true; 3], [true; 3], &others, 6.0);
        assert_eq!(dx, 1.0);
    }

    #[test]
    fn respects_threshold_and_axis_mask() {
        let others = [Rect::new(100.0, 100.0, 50.0, 50.0)];
        let m = Rect::new(110.0, 0.0, 10.0, 10.0);
        let (dx, dy, g) = snap_rect(&m, [true; 3], [true; 3], &others, 2.0);
        assert_eq!((dx, dy), (0.0, 0.0));
        assert!(g.is_empty());
        // Only the right edge may snap (resizing from the right).
        let m = Rect::new(96.0, 0.0, 52.0, 10.0); // left 96 (near 100), right 148 (near 150)
        let (dx, _, _) = snap_rect(&m, [false, false, true], [false; 3], &others, 6.0);
        assert_eq!(dx, 2.0);
    }
}
