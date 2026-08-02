mod angle;
mod iosurface;
mod presenter;
mod scanout;
mod stats;

pub use presenter::{PresentMode, Presenter};
pub use scanout::ScanoutPublisher;
use zerocopy::IntoBytes;

#[derive(Clone, Copy)]
pub enum DisplayEvent {
    Changed,
}

#[derive(Clone, Copy)]
struct Rect {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

pub struct DisplayUpdate {
    pub sequence: u64,
    pub width: usize,
    pub height: usize,
    pub dirty_rect: (usize, usize, usize, usize),
    pub iosurface_id: Option<u32>,
}

pub struct DisplayBuffer {
    width: usize,
    height: usize,
    pixels: Vec<u32>,
    dirty_rect: Option<(usize, usize, usize, usize)>,
    iosurface_id: Option<u32>,
    sequence: u64,
}

impl DisplayBuffer {
    pub fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            pixels: Vec::new(),
            dirty_rect: None,
            iosurface_id: None,
            sequence: 0,
        }
    }

    fn resize(&mut self, width: usize, height: usize) {
        if self.width == width && self.height == height {
            return;
        }

        self.width = width;
        self.height = height;
        self.pixels.resize(width * height, 0);
        self.dirty_rect = Some((0, 0, width, height));
        self.iosurface_id = None;
        self.sequence = self.sequence.wrapping_add(1);
    }

    fn publish_pixels(&mut self, data: &[u8], stride: usize, source: Rect, damage: Rect) -> bool {
        let Some((x, y, width, height)) = self.clip_damage(source, damage) else {
            return false;
        };

        let copy_src_x0 = source.x + x;
        let copy_src_y0 = source.y + y;
        let copy_src_x1 = copy_src_x0 + width;
        let copy_src_y1 = copy_src_y0 + height;

        if data.len() < copy_src_y1 * stride {
            return false;
        }

        let dst_width = self.width;
        for (src_y, dst_y) in (copy_src_y0..copy_src_y1).zip(y..y + height) {
            let src_row = src_y * stride;
            let src = &data[src_row + copy_src_x0 * 4..src_row + copy_src_x1 * 4];
            let dst = &mut self.pixels[dst_y * dst_width + x..dst_y * dst_width + x + width];
            dst.as_mut_bytes().copy_from_slice(src);
        }

        self.publish_damage(None, (x, y, width, height));
        true
    }

    fn publish_iosurface(&mut self, iosurface_id: u32, source: Rect, damage: Rect) -> bool {
        let Some(dirty_rect) = self.clip_damage(source, damage) else {
            return false;
        };

        self.publish_damage(Some(iosurface_id), dirty_rect);
        true
    }

    fn clip_damage(&self, source: Rect, damage: Rect) -> Option<(usize, usize, usize, usize)> {
        let damage_x1 = damage.x.saturating_add(damage.width);
        let damage_y1 = damage.y.saturating_add(damage.height);
        let source_x1 = source.x.saturating_add(source.width);
        let source_y1 = source.y.saturating_add(source.height);

        let src_x0 = damage.x.max(source.x);
        let src_y0 = damage.y.max(source.y);
        let src_x1 = damage_x1.min(source_x1);
        let src_y1 = damage_y1.min(source_y1);

        if src_x0 >= src_x1 || src_y0 >= src_y1 {
            return None;
        }

        let x0 = (src_x0 - source.x).min(self.width);
        let y0 = (src_y0 - source.y).min(self.height);
        let x1 = (src_x1 - source.x).min(self.width);
        let y1 = (src_y1 - source.y).min(self.height);

        (x0 < x1 && y0 < y1).then_some((x0, y0, x1 - x0, y1 - y0))
    }

    fn publish_damage(&mut self, iosurface_id: Option<u32>, dirty_rect: (usize, usize, usize, usize)) {
        self.iosurface_id = iosurface_id;
        self.dirty_rect = Some(match self.dirty_rect {
            Some(old) => merge_rects(old, dirty_rect),
            None => dirty_rect,
        });
        self.sequence = self.sequence.wrapping_add(1);
    }

    pub fn take_update(&mut self, last_sequence: u64, front: &mut Vec<u32>) -> Option<DisplayUpdate> {
        if self.sequence == last_sequence {
            return None;
        }

        let full = (0, 0, self.width, self.height);
        let mut dirty_rect = self.dirty_rect.take().unwrap_or(full);

        if front.len() != self.pixels.len() {
            front.resize(self.pixels.len(), 0);
            dirty_rect = full;
        }

        let (x, y, width, height) = dirty_rect;
        let x = x.min(self.width);
        let y = y.min(self.height);
        let width = width.min(self.width.saturating_sub(x));
        let height = height.min(self.height.saturating_sub(y));
        let dirty_rect = (x, y, width, height);

        if self.iosurface_id.is_none() && width != 0 && height != 0 {
            for row in 0..height {
                let offset = (y + row) * self.width + x;
                front[offset..offset + width].copy_from_slice(&self.pixels[offset..offset + width]);
            }
        }

        Some(DisplayUpdate {
            sequence: self.sequence,
            width: self.width,
            height: self.height,
            dirty_rect,
            iosurface_id: self.iosurface_id,
        })
    }
}

fn merge_rects(
    (old_x, old_y, old_width, old_height): (usize, usize, usize, usize),
    (new_x, new_y, new_width, new_height): (usize, usize, usize, usize),
) -> (usize, usize, usize, usize) {
    let x0 = old_x.min(new_x);
    let y0 = old_y.min(new_y);
    let x1 = old_x.saturating_add(old_width).max(new_x.saturating_add(new_width));
    let y1 = old_y.saturating_add(old_height).max(new_y.saturating_add(new_height));
    (x0, y0, x1 - x0, y1 - y0)
}
