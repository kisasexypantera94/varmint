use font8x8::{BASIC_FONTS, UnicodeFonts};
use serde::Serialize;
use std::{
    fs::File,
    io::{BufWriter, Write},
    time::{Duration, Instant},
};

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

#[derive(Serialize)]
struct Snapshot {
    sample_duration_seconds: f64,
    display_update_fps: f64,
    host_present_fps: f64,
    coalesced_display_updates: u64,
    average_host_present_cpu_time_ms: f64,
}

#[derive(Default)]
struct Measurements {
    display_updates: u64,
    host_presents: u64,
    coalesced_display_updates: u64,
    host_present_cpu_time: Duration,
}

impl Measurements {
    fn record_display_updates(&mut self, count: u64) {
        self.display_updates = self.display_updates.saturating_add(count);
    }

    fn record_present(&mut self, presented: bool, display_updates: u64, cpu_time: Duration) {
        if !presented {
            return;
        }

        self.host_presents = self.host_presents.saturating_add(1);
        self.coalesced_display_updates = self
            .coalesced_display_updates
            .saturating_add(display_updates.saturating_sub(1));
        self.host_present_cpu_time = self.host_present_cpu_time.saturating_add(cpu_time);
    }

    fn take_sample(&mut self, elapsed: Duration) -> Snapshot {
        let elapsed_seconds = elapsed.as_secs_f64();
        let snapshot = Snapshot {
            sample_duration_seconds: elapsed_seconds,
            display_update_fps: self.display_updates as f64 / elapsed_seconds,
            host_present_fps: self.host_presents as f64 / elapsed_seconds,
            coalesced_display_updates: self.coalesced_display_updates,
            average_host_present_cpu_time_ms: if self.host_presents == 0 {
                0.0
            } else {
                self.host_present_cpu_time.as_secs_f64() * 1_000.0 / self.host_presents as f64
            },
        };
        *self = Self::default();
        snapshot
    }
}

fn write_sample(output: &mut impl Write, snapshot: &Snapshot) {
    serde_json::to_writer(&mut *output, snapshot).expect("failed to write display metrics");
    output.write_all(b"\n").expect("failed to write display metrics");
}

pub struct DisplayStats {
    visible: bool,
    window_started: Instant,
    measurements: Measurements,
    output: Option<BufWriter<File>>,
    pending_pixels: Option<Vec<u32>>,
}

impl DisplayStats {
    pub fn new() -> Self {
        let output = std::env::var_os("VARMINT_METRICS_PATH").map(|path| {
            let file = File::create(&path)
                .unwrap_or_else(|error| panic!("failed to create metrics file {}: {error}", path.to_string_lossy()));
            BufWriter::new(file)
        });

        Self {
            visible: false,
            window_started: Instant::now(),
            measurements: Measurements::default(),
            output,
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
        self.pending_pixels = visible.then(|| render(None));
    }

    pub fn record_display_updates(&mut self, count: u64) {
        self.measurements.record_display_updates(count);
    }

    pub fn record_present(&mut self, presented: bool, display_updates: u64, cpu_time: Duration) {
        self.measurements.record_present(presented, display_updates, cpu_time);
    }

    pub fn refresh(&mut self) {
        let elapsed = self.window_started.elapsed();
        if elapsed < UPDATE_INTERVAL {
            return;
        }

        let snapshot = self.measurements.take_sample(elapsed);
        if self.visible {
            self.pending_pixels = Some(render(Some(&snapshot)));
        }
        if let Some(output) = self.output.as_mut() {
            write_sample(output, &snapshot);
            output.flush().expect("failed to flush display metrics");
        }
        self.window_started = Instant::now();
    }

    pub fn take_pixels(&mut self) -> Option<Vec<u32>> {
        self.pending_pixels.take()
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
        || ["HOST FPS -".to_owned(), "PRES CPU -".to_owned()],
        |snapshot| {
            [
                format!("HOST FPS {:.1}", snapshot.host_present_fps),
                format!("PRES CPU {:.2} MS", snapshot.average_host_present_cpu_time_ms),
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

#[cfg(test)]
mod tests {
    use super::{Measurements, write_sample};
    use std::time::Duration;

    #[test]
    fn sample_reports_rates_coalescing_and_cpu_time() {
        let mut measurements = Measurements::default();
        measurements.record_display_updates(6);
        measurements.record_present(true, 4, Duration::from_millis(2));
        measurements.record_present(true, 2, Duration::from_millis(4));

        let sample = measurements.take_sample(Duration::from_secs(2));
        assert_eq!(sample.sample_duration_seconds, 2.0);
        assert_eq!(sample.display_update_fps, 3.0);
        assert_eq!(sample.host_present_fps, 1.0);
        assert_eq!(sample.coalesced_display_updates, 4);
        assert_eq!(sample.average_host_present_cpu_time_ms, 3.0);
    }

    #[test]
    fn failed_presents_do_not_affect_present_metrics() {
        let mut measurements = Measurements::default();
        measurements.record_display_updates(1);
        measurements.record_present(false, 1, Duration::from_millis(5));

        let sample = measurements.take_sample(Duration::from_secs(1));
        assert_eq!(sample.display_update_fps, 1.0);
        assert_eq!(sample.host_present_fps, 0.0);
        assert_eq!(sample.coalesced_display_updates, 0);
        assert_eq!(sample.average_host_present_cpu_time_ms, 0.0);
    }

    #[test]
    fn sampling_resets_measurements() {
        let mut measurements = Measurements::default();
        measurements.record_display_updates(3);
        measurements.record_present(true, 3, Duration::from_millis(1));
        measurements.take_sample(Duration::from_secs(1));

        let sample = measurements.take_sample(Duration::from_secs(1));
        assert_eq!(sample.display_update_fps, 0.0);
        assert_eq!(sample.host_present_fps, 0.0);
        assert_eq!(sample.coalesced_display_updates, 0);
        assert_eq!(sample.average_host_present_cpu_time_ms, 0.0);
    }

    #[test]
    fn sample_serializes_with_metric_names() {
        let mut measurements = Measurements::default();
        let sample = measurements.take_sample(Duration::from_secs(1));
        let mut output = Vec::new();
        write_sample(&mut output, &sample);

        assert_eq!(output.last(), Some(&b'\n'));
        let value: serde_json::Value = serde_json::from_slice(&output).unwrap();

        assert!(value.get("sample_duration_seconds").is_some());
        assert!(value.get("display_update_fps").is_some());
        assert!(value.get("host_present_fps").is_some());
        assert!(value.get("coalesced_display_updates").is_some());
        assert!(value.get("average_host_present_cpu_time_ms").is_some());
    }
}
