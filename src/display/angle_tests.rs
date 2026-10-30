use super::{
    AngleCopy, EGL_CONTEXT_CLIENT_VERSION, EGL_DRAW, EGL_FALSE, EGL_HEIGHT, EGL_NONE, EGL_READ, EGL_WIDTH, EGLContext,
    EGLDisplay, EGLSurface, EglCreateContext, EglCreatePbufferSurface, EglDestroyContext, EglDestroySurface,
    EglMakeCurrent, open_library, symbol,
};
use crate::{display::iosurface::ScopedIOSurface, virtio::virgl_ffi::NativeTexture};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_io_surface::IOSurfaceLockOptions;
use objc2_metal::{
    MTLCreateSystemDefaultDevice, MTLDevice, MTLOrigin, MTLPixelFormat, MTLRegion, MTLSize, MTLStorageMode, MTLTexture,
    MTLTextureDescriptor, MTLTextureUsage,
};
use serde::Serialize;
use std::{
    ptr::{self, NonNull},
    time::{Duration, Instant},
};

type MetalTexture = ProtocolObject<dyn MTLTexture>;

/// Matches virglrenderer's `vrend_resource_metal_init` for scanout resources.
fn source_texture(format: MTLPixelFormat, width: u32, height: u32) -> Retained<MetalTexture> {
    let device = MTLCreateSystemDefaultDevice().expect("no Metal device");
    let desc = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            format,
            width as usize,
            height as usize,
            false,
        )
    };
    desc.setStorageMode(MTLStorageMode::Shared);
    desc.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite | MTLTextureUsage::RenderTarget);
    device
        .newTextureWithDescriptor(&desc)
        .expect("failed to create source texture")
}

fn native(texture: &MetalTexture) -> NativeTexture {
    NativeTexture::new(
        NonNull::from(texture).cast(),
        texture.width() as u32,
        texture.height() as u32,
    )
}

/// Waits for the copy context's GPU work while preserving the caller's current context.
fn finish(copy: &mut AngleCopy) {
    let sess = copy.session.as_mut().unwrap();
    unsafe {
        let display = (sess.egl_get_current_display)();
        let draw = (sess.egl_get_current_surface)(EGL_DRAW);
        let read = (sess.egl_get_current_surface)(EGL_READ);
        let context = (sess.egl_get_current_context)();
        assert_ne!(
            (sess.egl_make_current)(sess.display, sess.dummy_surface, sess.dummy_surface, sess.context),
            EGL_FALSE
        );
        (sess.gl_finish)();
        if display.is_null() {
            (sess.egl_make_current)(sess.display, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
        } else {
            (sess.egl_make_current)(display, draw, read, context);
        }
    }
}

/// Stands in for the virglrenderer context that is current on the GPU thread
/// when a scanout is presented, so the copy pays for the same context switches.
struct RendererContext {
    display: EGLDisplay,
    surface: EGLSurface,
    context: EGLContext,
    make_current: EglMakeCurrent,
    destroy_surface: EglDestroySurface,
    destroy_context: EglDestroyContext,
}

impl RendererContext {
    fn make_current(copy: &AngleCopy) -> Self {
        let sess = copy.session.as_ref().unwrap();
        let egl = open_library(&["@rpath/libEGL.dylib", "libEGL.dylib"]).unwrap();
        let create_context: EglCreateContext = unsafe { symbol(egl, "eglCreateContext") }.unwrap();
        let create_pbuffer: EglCreatePbufferSurface = unsafe { symbol(egl, "eglCreatePbufferSurface") }.unwrap();

        let surface_attribs = [EGL_WIDTH, 1, EGL_HEIGHT, 1, EGL_NONE];
        let context_attribs = [EGL_CONTEXT_CLIENT_VERSION, 3, EGL_NONE];
        let surface = unsafe { create_pbuffer(sess.display, sess.config, surface_attribs.as_ptr()) };
        let context = unsafe { create_context(sess.display, sess.config, ptr::null_mut(), context_attribs.as_ptr()) };
        assert!(!surface.is_null() && !context.is_null());
        assert_ne!(
            unsafe { (sess.egl_make_current)(sess.display, surface, surface, context) },
            EGL_FALSE
        );

        Self {
            display: sess.display,
            surface,
            context,
            make_current: sess.egl_make_current,
            destroy_surface: sess.egl_destroy_surface,
            destroy_context: sess.egl_destroy_context,
        }
    }
}

impl Drop for RendererContext {
    fn drop(&mut self) {
        unsafe {
            (self.make_current)(self.display, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            (self.destroy_surface)(self.display, self.surface);
            (self.destroy_context)(self.display, self.context);
        }
    }
}

fn pattern(x: u32, y: u32) -> [u8; 4] {
    let r = (x * 4) as u8;
    let g = (y * 5) as u8;
    let b = (x ^ (y * 3) ^ 0x5a) as u8;
    [r, g, b, 0xff]
}

fn upload_pattern(texture: &MetalTexture, bgra: bool) {
    let (width, height) = (texture.width() as u32, texture.height() as u32);
    let mut bytes = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let [r, g, b, a] = pattern(x, y);
            bytes.extend_from_slice(&if bgra { [b, g, r, a] } else { [r, g, b, a] });
        }
    }

    let region = MTLRegion {
        origin: MTLOrigin { x: 0, y: 0, z: 0 },
        size: MTLSize {
            width: width as usize,
            height: height as usize,
            depth: 1,
        },
    };
    unsafe {
        texture.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
            region,
            0,
            NonNull::new(bytes.as_mut_ptr().cast()).unwrap(),
            width as usize * 4,
        );
    }
}

/// Returns the target's pixels as [r, g, b, a], row 0 first.
fn read_bgra_surface(surface: &ScopedIOSurface) -> Vec<[u8; 4]> {
    let surface_ref = surface.as_objc_ref();
    unsafe { surface_ref.lock(IOSurfaceLockOptions::ReadOnly, ptr::null_mut()) };
    let base = surface_ref.base_address().as_ptr() as *const u8;
    let stride = surface_ref.bytes_per_row();
    let mut pixels = Vec::new();
    for y in 0..surface.height() as usize {
        for x in 0..surface.width() as usize {
            let [b, g, r, a] = unsafe { *(base.add(y * stride + x * 4) as *const [u8; 4]) };
            pixels.push([r, g, b, a]);
        }
    }
    unsafe { surface_ref.unlock(IOSurfaceLockOptions::ReadOnly, ptr::null_mut()) };
    pixels
}

// Pins the semantics a replacement for the GLES copy has to keep: channel
// order of either 8-bit source layout into the BGRA IOSurface, source
// rectangle cropping, and row 0 of the source rect landing in row 0 of the target.
// Ignored because libEGL only finds libGLESv2 through a framework layout; run
// through scripts/present-bench.sh.
#[test]
#[ignore]
fn copy_converts_channels_crops_and_keeps_orientation() {
    let (texture_width, texture_height) = (64, 48);
    let (x0, y0, width, height) = (8, 4, 40, 32);

    for (format, bgra) in [(MTLPixelFormat::BGRA8Unorm, true), (MTLPixelFormat::RGBA8Unorm, false)] {
        let texture = source_texture(format, texture_width, texture_height);
        upload_pattern(&texture, bgra);
        let target = ScopedIOSurface::new_bgra(width, height).unwrap();

        let mut copy = AngleCopy::new();
        assert!(copy.copy_native_texture_to_iosurface(native(&texture), target.as_ptr(), (x0, y0, width, height)));
        finish(&mut copy);

        let pixels = read_bgra_surface(&target);
        for y in 0..height {
            for x in 0..width {
                assert_eq!(
                    pixels[(y * width + x) as usize],
                    pattern(x0 + x, y0 + y),
                    "format={format:?} target=({x}, {y})"
                );
            }
        }
    }
}

struct BenchCase {
    name: &'static str,
    texture_size: (u32, u32),
    source_rect: (u32, u32, u32, u32),
    textures: usize,
    renderer_context: bool,
}

const fn full(name: &'static str, width: u32, height: u32, textures: usize) -> BenchCase {
    BenchCase {
        name,
        texture_size: (width, height),
        source_rect: (0, 0, width, height),
        textures,
        renderer_context: true,
    }
}

const BENCH_CASES: &[BenchCase] = &[
    full("1920x1200/1tex", 1920, 1200, 1),
    full("1920x1200/2tex", 1920, 1200, 2),
    full("1920x1200/3tex", 1920, 1200, 3),
    full("2560x1600/1tex", 2560, 1600, 1),
    full("2560x1600/2tex", 2560, 1600, 2),
    full("2560x1600/3tex", 2560, 1600, 3),
    full("3456x2168/1tex", 3456, 2168, 1),
    full("3456x2168/2tex", 3456, 2168, 2),
    full("3456x2168/3tex", 3456, 2168, 3),
    BenchCase {
        name: "1920x1200-in-2560x1600/3tex",
        source_rect: (320, 200, 1920, 1200),
        ..full("", 2560, 1600, 3)
    },
    BenchCase {
        renderer_context: false,
        ..full("1920x1200/1tex/no-renderer-context", 1920, 1200, 1)
    },
    BenchCase {
        renderer_context: false,
        ..full("1920x1200/3tex/no-renderer-context", 1920, 1200, 3)
    },
];

const WARMUP_COPIES: usize = 30;

#[derive(Serialize)]
struct Distribution {
    median: f64,
    p95: f64,
    mean: f64,
    max: f64,
}

impl Distribution {
    fn new(samples: &[Duration]) -> Self {
        let mut ms: Vec<f64> = samples.iter().map(|d| d.as_secs_f64() * 1_000.0).collect();
        ms.sort_by(f64::total_cmp);
        let at = |q: f64| ms[((ms.len() - 1) as f64 * q).round() as usize];
        Self {
            median: at(0.5),
            p95: at(0.95),
            mean: ms.iter().sum::<f64>() / ms.len() as f64,
            max: at(1.0),
        }
    }
}

#[derive(Serialize)]
struct RunResult {
    copy_cpu_ms: Distribution,
    copy_to_gpu_complete_ms: Distribution,
    copies: u64,
    source_rebuilds: u64,
    target_rebuilds: u64,
}

#[derive(Serialize)]
struct CaseResult {
    name: &'static str,
    texture_size: (u32, u32),
    source_rect: (u32, u32, u32, u32),
    textures: usize,
    renderer_context: bool,
    copy_cpu_median_ms: f64,
    copy_cpu_p95_ms: f64,
    copy_cpu_mean_ms: f64,
    copy_to_gpu_complete_median_ms: f64,
    source_rebuilds_per_copy: f64,
    target_rebuilds_per_copy: f64,
    runs: Vec<RunResult>,
}

#[derive(Serialize)]
struct BenchReport {
    runs: usize,
    copies_per_run: usize,
    duration_seconds: f64,
    cases: Vec<CaseResult>,
}

fn run_case(case: &BenchCase, copies: usize) -> RunResult {
    let (texture_width, texture_height) = case.texture_size;
    let textures: Vec<_> = (0..case.textures)
        .map(|_| source_texture(MTLPixelFormat::BGRA8Unorm, texture_width, texture_height))
        .collect();
    let target = ScopedIOSurface::new_bgra(case.source_rect.2, case.source_rect.3).unwrap();
    let mut copy = AngleCopy::new();
    let _renderer = case.renderer_context.then(|| RendererContext::make_current(&copy));

    let copy_frame = |copy: &mut AngleCopy, frame: usize| {
        let texture = native(&textures[frame % textures.len()]);
        let started = Instant::now();
        assert!(copy.copy_native_texture_to_iosurface(texture, target.as_ptr(), case.source_rect));
        started.elapsed()
    };

    for frame in 0..WARMUP_COPIES {
        copy_frame(&mut copy, frame);
        finish(&mut copy);
    }
    copy.take_stats();

    // Guest frames arrive far slower than this copy completes, so drain the GPU
    // between copies to keep queue backpressure out of the CPU measurement.
    let mut cpu = Vec::with_capacity(copies);
    for frame in WARMUP_COPIES..WARMUP_COPIES + copies {
        cpu.push(copy_frame(&mut copy, frame));
        finish(&mut copy);
    }
    let stats = copy.take_stats();

    let mut gpu_complete = Vec::with_capacity(copies);
    for frame in 0..copies {
        let started = Instant::now();
        copy_frame(&mut copy, frame);
        finish(&mut copy);
        gpu_complete.push(started.elapsed());
    }

    RunResult {
        copy_cpu_ms: Distribution::new(&cpu),
        copy_to_gpu_complete_ms: Distribution::new(&gpu_complete),
        copies: stats.copies,
        source_rebuilds: stats.source_rebuilds,
        target_rebuilds: stats.target_rebuilds,
    }
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// A/B tool for the native texture → IOSurface snapshot stage, not a pass/fail
/// test. Run through `scripts/present-bench.sh`.
#[test]
#[ignore]
fn bench_angle_copy() {
    let output = std::env::var("VARMINT_PRESENT_BENCH_OUTPUT").expect("VARMINT_PRESENT_BENCH_OUTPUT is not set");
    let runs = env_or("VARMINT_PRESENT_BENCH_RUNS", 5);
    let copies = env_or("VARMINT_PRESENT_BENCH_COPIES", 300);
    let filter = std::env::var("VARMINT_PRESENT_BENCH_CASES").unwrap_or_default();
    let cases: Vec<_> = BENCH_CASES.iter().filter(|case| case.name.contains(&filter)).collect();

    let started = Instant::now();
    let mut results: Vec<Vec<RunResult>> = cases.iter().map(|_| Vec::new()).collect();
    // Interleave cases so slow drift (thermals, background load) spreads across all of them.
    for _ in 0..runs {
        for (case, results) in cases.iter().zip(&mut results) {
            results.push(run_case(case, copies));
        }
    }

    let cases = cases
        .into_iter()
        .zip(results)
        .map(|(case, runs)| {
            let total_copies = runs.iter().map(|r| r.copies).sum::<u64>() as f64;
            CaseResult {
                name: case.name,
                texture_size: case.texture_size,
                source_rect: case.source_rect,
                textures: case.textures,
                renderer_context: case.renderer_context,
                copy_cpu_median_ms: median(runs.iter().map(|r| r.copy_cpu_ms.median).collect()),
                copy_cpu_p95_ms: median(runs.iter().map(|r| r.copy_cpu_ms.p95).collect()),
                copy_cpu_mean_ms: median(runs.iter().map(|r| r.copy_cpu_ms.mean).collect()),
                copy_to_gpu_complete_median_ms: median(runs.iter().map(|r| r.copy_to_gpu_complete_ms.median).collect()),
                source_rebuilds_per_copy: runs.iter().map(|r| r.source_rebuilds).sum::<u64>() as f64 / total_copies,
                target_rebuilds_per_copy: runs.iter().map(|r| r.target_rebuilds).sum::<u64>() as f64 / total_copies,
                runs,
            }
        })
        .collect();

    let report = BenchReport {
        runs,
        copies_per_run: copies,
        duration_seconds: started.elapsed().as_secs_f64(),
        cases,
    };
    let file = std::fs::File::create(&output).unwrap_or_else(|e| panic!("failed to create {output}: {e}"));
    serde_json::to_writer_pretty(file, &report).unwrap();
}
