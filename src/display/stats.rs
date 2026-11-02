use font8x8::{BASIC_FONTS, UnicodeFonts};
use serde::Serialize;
use std::{
    fs::File,
    io::{self, BufWriter, Write},
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
    average_host_present_duration_ms: f64,
    angle_copies: u64,
    angle_copy_source_rebuilds: u64,
    angle_copy_target_rebuilds: u64,
    average_angle_copy_duration_ms: f64,
    max_angle_copy_duration_ms: f64,
}

/// Producer-side native texture → IOSurface copies, accumulated until the
/// presenter samples them.
#[derive(Clone, Copy, Default)]
pub struct AngleCopyStats {
    pub copies: u64,
    pub source_rebuilds: u64,
    pub target_rebuilds: u64,
    pub duration: Duration,
    pub max_duration: Duration,
}

impl AngleCopyStats {
    pub fn record_copy(&mut self, duration: Duration) {
        self.copies = self.copies.saturating_add(1);
        self.duration = self.duration.saturating_add(duration);
        self.max_duration = self.max_duration.max(duration);
    }

    pub fn merge(&mut self, other: Self) {
        self.copies = self.copies.saturating_add(other.copies);
        self.source_rebuilds = self.source_rebuilds.saturating_add(other.source_rebuilds);
        self.target_rebuilds = self.target_rebuilds.saturating_add(other.target_rebuilds);
        self.duration = self.duration.saturating_add(other.duration);
        self.max_duration = self.max_duration.max(other.max_duration);
    }
}

#[derive(Default)]
struct Measurements {
    display_updates: u64,
    host_presents: u64,
    coalesced_display_updates: u64,
    host_present_duration: Duration,
    angle_copy: AngleCopyStats,
}

impl Measurements {
    fn record_display_updates(&mut self, count: u64) {
        self.display_updates = self.display_updates.saturating_add(count);
    }

    fn record_present(&mut self, presented: bool, display_updates: u64, duration: Duration) {
        if !presented {
            return;
        }

        self.host_presents = self.host_presents.saturating_add(1);
        self.coalesced_display_updates = self
            .coalesced_display_updates
            .saturating_add(display_updates.saturating_sub(1));
        self.host_present_duration = self.host_present_duration.saturating_add(duration);
    }

    fn take_sample(&mut self, elapsed: Duration) -> Snapshot {
        let elapsed_seconds = elapsed.as_secs_f64();
        let snapshot = Snapshot {
            sample_duration_seconds: elapsed_seconds,
            display_update_fps: self.display_updates as f64 / elapsed_seconds,
            host_present_fps: self.host_presents as f64 / elapsed_seconds,
            coalesced_display_updates: self.coalesced_display_updates,
            average_host_present_duration_ms: if self.host_presents == 0 {
                0.0
            } else {
                self.host_present_duration.as_secs_f64() * 1_000.0 / self.host_presents as f64
            },
            angle_copies: self.angle_copy.copies,
            angle_copy_source_rebuilds: self.angle_copy.source_rebuilds,
            angle_copy_target_rebuilds: self.angle_copy.target_rebuilds,
            average_angle_copy_duration_ms: if self.angle_copy.copies == 0 {
                0.0
            } else {
                self.angle_copy.duration.as_secs_f64() * 1_000.0 / self.angle_copy.copies as f64
            },
            max_angle_copy_duration_ms: self.angle_copy.max_duration.as_secs_f64() * 1_000.0,
        };
        *self = Self::default();
        snapshot
    }
}

fn write_sample(output: &mut impl Write, snapshot: &Snapshot) -> io::Result<()> {
    serde_json::to_writer(&mut *output, snapshot).map_err(io::Error::other)?;
    output.write_all(b"\n")
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
        let output = std::env::var_os("VARMINT_METRICS_PATH").and_then(|path| {
            File::create(&path)
                .map(BufWriter::new)
                .map_err(|error| {
                    eprintln!("display metrics: failed to create {}: {error}", path.to_string_lossy());
                })
                .ok()
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

    pub fn record_angle_copies(&mut self, stats: AngleCopyStats) {
        self.measurements.angle_copy.merge(stats);
    }

    pub fn record_present(&mut self, presented: bool, display_updates: u64, duration: Duration) {
        self.measurements.record_present(presented, display_updates, duration);
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
        let output_error = self
            .output
            .as_mut()
            .and_then(|output| write_sample(output, &snapshot).and_then(|()| output.flush()).err());
        if let Some(error) = output_error {
            eprintln!("display metrics: stopped writing: {error}");
            self.output = None;
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
        || ["HOST FPS -".to_owned(), "PRES -".to_owned()],
        |snapshot| {
            [
                format!("HOST FPS {:.1}", snapshot.host_present_fps),
                format!("PRES {:.2} MS", snapshot.average_host_present_duration_ms),
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
    use super::{AngleCopyStats, Measurements, write_sample};
    use std::time::Duration;

    #[test]
    fn sample_reports_angle_copies() {
        let mut first = AngleCopyStats {
            source_rebuilds: 2,
            target_rebuilds: 1,
            ..Default::default()
        };
        first.record_copy(Duration::from_millis(1));
        first.record_copy(Duration::from_millis(3));
        let mut second = AngleCopyStats {
            source_rebuilds: 1,
            ..Default::default()
        };
        second.record_copy(Duration::from_millis(2));

        let mut measurements = Measurements::default();
        measurements.angle_copy.merge(first);
        measurements.angle_copy.merge(second);

        let sample = measurements.take_sample(Duration::from_secs(1));
        assert_eq!(sample.angle_copies, 3);
        assert_eq!(sample.angle_copy_source_rebuilds, 3);
        assert_eq!(sample.angle_copy_target_rebuilds, 1);
        assert_eq!(sample.average_angle_copy_duration_ms, 2.0);
        assert_eq!(sample.max_angle_copy_duration_ms, 3.0);
    }

    #[test]
    fn sample_reports_rates_coalescing_and_duration() {
        let mut measurements = Measurements::default();
        measurements.record_display_updates(6);
        measurements.record_present(true, 4, Duration::from_millis(2));
        measurements.record_present(true, 2, Duration::from_millis(4));

        let sample = measurements.take_sample(Duration::from_secs(2));
        assert_eq!(sample.sample_duration_seconds, 2.0);
        assert_eq!(sample.display_update_fps, 3.0);
        assert_eq!(sample.host_present_fps, 1.0);
        assert_eq!(sample.coalesced_display_updates, 4);
        assert_eq!(sample.average_host_present_duration_ms, 3.0);
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
        assert_eq!(sample.average_host_present_duration_ms, 0.0);
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
        assert_eq!(sample.average_host_present_duration_ms, 0.0);
    }

    #[test]
    fn sample_serializes_with_metric_names() {
        let mut measurements = Measurements::default();
        let sample = measurements.take_sample(Duration::from_secs(1));
        let mut output = Vec::new();
        write_sample(&mut output, &sample).unwrap();

        assert_eq!(output.last(), Some(&b'\n'));
        let value: serde_json::Value = serde_json::from_slice(&output).unwrap();

        assert!(value.get("sample_duration_seconds").is_some());
        assert!(value.get("display_update_fps").is_some());
        assert!(value.get("host_present_fps").is_some());
        assert!(value.get("coalesced_display_updates").is_some());
        assert!(value.get("average_host_present_duration_ms").is_some());
        assert!(value.get("angle_copies").is_some());
        assert!(value.get("angle_copy_source_rebuilds").is_some());
        assert!(value.get("angle_copy_target_rebuilds").is_some());
        assert!(value.get("average_angle_copy_duration_ms").is_some());
        assert!(value.get("max_angle_copy_duration_ms").is_some());
    }
}
