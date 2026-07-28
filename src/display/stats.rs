use font8x8::{BASIC_FONTS, UnicodeFonts};
use std::time::{Duration, Instant};

const UPDATE_INTERVAL: Duration = Duration::from_secs(1);

pub const WIDTH: u32 = 304;
pub const HEIGHT: u32 = 56;
pub const MARGIN: usize = 8;

const GLYPH_SCALE: usize = 2;
const GLYPH_SIZE: usize = 8;
const GLYPH_ADVANCE: usize = 9 * GLYPH_SCALE;
const LINE_ADVANCE: usize = 10 * GLYPH_SCALE;

const BACKGROUND: u32 = 0xff181818;
const BORDER: u32 = 0xff505050;
const TEXT: u32 = 0xfff2f2f2;

struct Snapshot {
    fps: f64,
    display_ms: f64,
}

pub struct StatsHud {
    visible: bool,
    window_started: Option<Instant>,
    presented: u64,
    display_time: Duration,
    pending_pixels: Option<Vec<u32>>,
}

impl StatsHud {
    pub fn new() -> Self {
        Self {
            visible: false,
            window_started: None,
            presented: 0,
            display_time: Duration::ZERO,
            pending_pixels: None,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn set_visible(&mut self, visible: bool) {
        if self.visible == visible {
            return;
        }

        self.visible = visible;
        self.reset_window();
        self.pending_pixels = visible.then(|| render(None));
    }

    pub fn record_frame(&mut self, presented: bool, display_time: Duration) {
        if !self.visible || !presented {
            return;
        }

        if self.window_started.is_none() {
            self.window_started = Some(Instant::now());
            return;
        }

        self.presented += 1;
        self.display_time += display_time;
    }

    pub fn refresh(&mut self) {
        if !self.visible {
            return;
        }

        let Some(started) = self.window_started else {
            return;
        };
        let elapsed = started.elapsed();
        if elapsed < UPDATE_INTERVAL {
            return;
        }

        let snapshot = Snapshot {
            fps: self.presented as f64 / elapsed.as_secs_f64(),
            display_ms: if self.presented == 0 {
                0.0
            } else {
                self.display_time.as_secs_f64() * 1_000.0 / self.presented as f64
            },
        };
        self.pending_pixels = Some(render(Some(&snapshot)));
        self.window_started = Some(Instant::now());
        self.presented = 0;
        self.display_time = Duration::ZERO;
    }

    pub fn take_pixels(&mut self) -> Option<Vec<u32>> {
        self.pending_pixels.take()
    }

    fn reset_window(&mut self) {
        self.window_started = None;
        self.presented = 0;
        self.display_time = Duration::ZERO;
    }
}

fn render(snapshot: Option<&Snapshot>) -> Vec<u32> {
    let width = WIDTH as usize;
    let height = HEIGHT as usize;
    let mut pixels = vec![BACKGROUND; width * height];

    for x in 0..width {
        pixels[x] = BORDER;
        pixels[(height - 1) * width + x] = BORDER;
    }
    for y in 0..height {
        pixels[y * width] = BORDER;
        pixels[y * width + width - 1] = BORDER;
    }

    let lines = snapshot.map_or_else(
        || ["FPS     -".to_owned(), "DISPLAY -".to_owned()],
        |snapshot| {
            [
                format!("FPS     {:.1}", snapshot.fps),
                format!("DISPLAY {:.2} MS", snapshot.display_ms),
            ]
        },
    );

    let mut y = MARGIN;
    for line in lines {
        draw_text_line(&mut pixels, y, &line);
        y += LINE_ADVANCE;
    }

    pixels
}

fn draw_text_line(pixels: &mut [u32], y: usize, text: &str) {
    let width = WIDTH as usize;
    let height = HEIGHT as usize;
    let mut x = MARGIN;

    for character in text.chars() {
        let Some(glyph) = BASIC_FONTS.get(character) else {
            continue;
        };
        if x + GLYPH_SIZE * GLYPH_SCALE >= width - MARGIN {
            break;
        }

        for (row, bits) in glyph.into_iter().enumerate() {
            for column in 0..GLYPH_SIZE {
                if bits & (1 << column) == 0 {
                    continue;
                }
                for scale_y in 0..GLYPH_SCALE {
                    for scale_x in 0..GLYPH_SCALE {
                        let px = x + column * GLYPH_SCALE + scale_x;
                        let py = y + row * GLYPH_SCALE + scale_y;
                        if px < width && py < height {
                            pixels[py * width + px] = TEXT;
                        }
                    }
                }
            }
        }

        x += GLYPH_ADVANCE;
    }
}
