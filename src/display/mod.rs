mod angle;
mod iosurface;
mod presenter;
mod scanout;

pub use presenter::Presenter;
pub use scanout::ScanoutPublisher;

pub struct DisplayBuffer {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
    pub dirty_rect: Option<(usize, usize, usize, usize)>,
    pub iosurface_id: Option<u32>,
    pub seq: u64,
}

impl DisplayBuffer {
    pub fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            pixels: Vec::new(),
            seq: 0,
            dirty_rect: None,
            iosurface_id: None,
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        if self.width == width && self.height == height {
            return;
        }

        self.width = width;
        self.height = height;
        self.pixels.resize(width * height, 0);
        self.dirty_rect = Some((0, 0, width, height));
        self.iosurface_id = None;
        self.seq = self.seq.wrapping_add(1);
    }
}
