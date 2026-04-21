use crate::scene::OwnedPolygon;
use gdstk_rs::BoundingBox;

#[derive(Clone, Debug, PartialEq)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub world_box: BoundingBox,
    pub pan_x: f64,
    pub pan_y: f64,
    pub scale: f64,
}

impl Viewport {
    pub fn effective_box(&self) -> BoundingBox {
        let mut world_box = self.world_box;
        if !world_box.min_x.is_finite()
            || !world_box.min_y.is_finite()
            || !world_box.max_x.is_finite()
            || !world_box.max_y.is_finite()
        {
            world_box = BoundingBox {
                min_x: 0.0,
                min_y: 0.0,
                max_x: self.width as f64,
                max_y: self.height as f64,
            };
        }

        let width = (world_box.max_x - world_box.min_x).abs().max(1.0);
        let height = (world_box.max_y - world_box.min_y).abs().max(1.0);
        let scale = if self.scale.is_finite() && self.scale > 0.0 {
            self.scale
        } else {
            1.0
        };
        let center_x = (world_box.min_x + world_box.max_x) * 0.5 + self.pan_x;
        let center_y = (world_box.min_y + world_box.max_y) * 0.5 + self.pan_y;
        let half_w = width / scale * 0.5;
        let half_h = height / scale * 0.5;

        BoundingBox {
            min_x: center_x - half_w,
            min_y: center_y - half_h,
            max_x: center_x + half_w,
            max_y: center_y + half_h,
        }
    }
}

pub fn svg_viewbox(bbox: BoundingBox, width: u32, height: u32) -> String {
    let mut min_x = bbox.min_x;
    let mut min_y = bbox.min_y;
    let mut max_x = bbox.max_x;
    let mut max_y = bbox.max_y;

    if !min_x.is_finite() || !min_y.is_finite() || !max_x.is_finite() || !max_y.is_finite() {
        min_x = 0.0;
        min_y = 0.0;
        max_x = width as f64;
        max_y = height as f64;
    }

    if (max_x - min_x).abs() < f64::EPSILON {
        max_x += 1.0;
    }
    if (max_y - min_y).abs() < f64::EPSILON {
        max_y += 1.0;
    }

    format!("{:.4} {:.4} {:.4} {:.4}", min_x, min_y, max_x - min_x, max_y - min_y)
}

pub fn expanded_bbox(base: BoundingBox, highlights: &[OwnedPolygon]) -> BoundingBox {
    let mut bbox = base;
    for poly in highlights {
        for point in &poly.points {
            if point.x < bbox.min_x {
                bbox.min_x = point.x;
            }
            if point.y < bbox.min_y {
                bbox.min_y = point.y;
            }
            if point.x > bbox.max_x {
                bbox.max_x = point.x;
            }
            if point.y > bbox.max_y {
                bbox.max_y = point.y;
            }
        }
    }
    bbox
}
