use super::{
    AngleCopy, EGL_CONTEXT_CLIENT_VERSION, EGL_DRAW, EGL_FALSE, EGL_HEIGHT, EGL_NONE, EGL_READ, EGL_WIDTH, EGLContext,
    EGLDisplay, EGLSurface, EglCreateContext, EglCreatePbufferSurface, EglDestroyContext, EglDestroySurface,
    EglMakeCurrent, open_library, symbol,
};
use crate::{display::iosurface::ScopedIOSurface, virtio::virgl_ffi::NativeTexture};
use objc2::{msg_send, rc::Retained, runtime::ProtocolObject};
use objc2_io_surface::IOSurfaceLockOptions;
use objc2_metal::{
    MTLBlitCommandEncoder, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue, MTLCreateSystemDefaultDevice,
    MTLDevice, MTLOrigin, MTLPixelFormat, MTLRegion, MTLSize, MTLStorageMode, MTLTexture, MTLTextureDescriptor,
    MTLTextureUsage,
};
use serde::Serialize;
use std::{
    ptr::{self, NonNull},
    time::{Duration, Instant},
};

type MetalTexture = ProtocolObject<dyn MTLTexture>;
type MetalCommandBuffer = ProtocolObject<dyn MTLCommandBuffer>;

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

// Experimental native Metal equivalent of AngleCopy for BGRA8 sources only:
// a blit cannot convert formats, so it is not a general replacement.
struct MetalBlit {
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    target: Retained<MetalTexture>,
}

impl MetalBlit {
    fn new(target: &ScopedIOSurface) -> Self {
        let device = MTLCreateSystemDefaultDevice().expect("no Metal device");
        let desc = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                MTLPixelFormat::BGRA8Unorm,
                target.width() as usize,
                target.height() as usize,
                false,
            )
        };
        desc.setStorageMode(MTLStorageMode::Shared);
        let target = device
            .newTextureWithDescriptor_iosurface_plane(&desc, target.as_objc_ref(), 0)
            .expect("failed to wrap target IOSurface");
        let queue = device.newCommandQueue().expect("failed to create Metal command queue");
        Self { queue, target }
    }

    /// Same rectangle semantics as AngleCopy: the source rect's origin lands at target (0, 0).
    fn copy(&self, source: &MetalTexture, (x, y, width, height): (u32, u32, u32, u32)) -> Retained<MetalCommandBuffer> {
        let command_buffer = self.queue.commandBuffer().unwrap();
        let blit = command_buffer.blitCommandEncoder().unwrap();
        unsafe {
            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                source,
                0,
                0,
                MTLOrigin { x: x as usize, y: y as usize, z: 0 },
                MTLSize { width: width as usize, height: height as usize, depth: 1 },
                &self.target,
                0,
                0,
                MTLOrigin { x: 0, y: 0, z: 0 },
            );
        }
        blit.endEncoding();
        command_buffer.commit();
        command_buffer
    }
}

fn gpu_execution_time(command_buffer: &MetalCommandBuffer) -> Duration {
    // GPUStartTime/GPUEndTime are behind objc2-metal's core-foundation feature.
    let start: f64 = unsafe { msg_send![command_buffer, GPUStartTime] };
    let end: f64 = unsafe { msg_send![command_buffer, GPUEndTime] };
    Duration::from_secs_f64(end - start)
}

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Implementation {
    AngleCopy,
    MetalBlit,
}

enum Copier {
    Angle(AngleCopy),
    Metal {
        blit: MetalBlit,
        pending: Option<Retained<MetalCommandBuffer>>,
    },
}

impl Copier {
    fn copy(&mut self, source: &MetalTexture, target: &ScopedIOSurface, source_rect: (u32, u32, u32, u32)) {
        match self {
            Self::Angle(copy) => {
                assert!(copy.copy_native_texture_to_iosurface(native(source), target.as_ptr(), source_rect))
            }
            Self::Metal { blit, pending } => *pending = Some(blit.copy(source, source_rect)),
        }
    }

    /// Blocks until the last copy has completed; returns its GPU execution
    /// time where the implementation exposes one.
    fn finish(&mut self) -> Option<Duration> {
        match self {
            Self::Angle(copy) => {
                finish(copy);
                None
            }
            Self::Metal { pending, .. } => {
                let command_buffer = pending.take().unwrap();
                command_buffer.waitUntilCompleted();
                Some(gpu_execution_time(&command_buffer))
            }
        }
    }
}

// The experimental blit must reproduce AngleCopy's BGRA8 output exactly,
// including crops at the benchmark's sizes.
#[test]
#[ignore]
fn metal_blit_matches_angle_copy_for_bgra8() {
    for (texture_size, source_rect) in [
        ((64, 48), (8, 4, 40, 32)),
        ((1920, 1200), (0, 0, 1920, 1200)),
        ((2560, 1600), (320, 200, 1920, 1200)),
    ] {
        let texture = source_texture(MTLPixelFormat::BGRA8Unorm, texture_size.0, texture_size.1);
        upload_pattern(&texture, true);
        let (x0, y0, width, height) = source_rect;

        let angle_target = ScopedIOSurface::new_bgra(width, height).unwrap();
        let mut angle = Copier::Angle(AngleCopy::new());
        angle.copy(&texture, &angle_target, source_rect);
        angle.finish();

        let metal_target = ScopedIOSurface::new_bgra(width, height).unwrap();
        let mut metal = Copier::Metal {
            blit: MetalBlit::new(&metal_target),
            pending: None,
        };
        metal.copy(&texture, &metal_target, source_rect);
        metal.finish();

        let expected: Vec<_> = (0..height)
            .flat_map(|y| (0..width).map(move |x| pattern(x0 + x, y0 + y)))
            .collect();
        assert!(
            read_bgra_surface(&angle_target) == expected,
            "angle copy {source_rect:?}"
        );
        assert!(
            read_bgra_surface(&metal_target) == expected,
            "metal blit {source_rect:?}"
        );
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
    /// Wall clock from the copy call until the implementation's wait for that
    /// copy returns; includes submission, scheduling and synchronization.
    completion_latency_ms: Distribution,
    gpu_execution_ms: Option<Distribution>,
    copies: u64,
    source_rebuilds: u64,
    target_rebuilds: u64,
}

#[derive(Serialize)]
struct CaseResult {
    implementation: Implementation,
    name: &'static str,
    texture_size: (u32, u32),
    source_rect: (u32, u32, u32, u32),
    textures: usize,
    renderer_context: bool,
    copy_cpu_median_ms: f64,
    copy_cpu_p95_ms: f64,
    copy_cpu_mean_ms: f64,
    completion_latency_median_ms: f64,
    gpu_execution_median_ms: Option<f64>,
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

fn run_case(case: &BenchCase, implementation: Implementation, copies: usize) -> RunResult {
    let (texture_width, texture_height) = case.texture_size;
    let textures: Vec<_> = (0..case.textures)
        .map(|_| source_texture(MTLPixelFormat::BGRA8Unorm, texture_width, texture_height))
        .collect();
    let target = ScopedIOSurface::new_bgra(case.source_rect.2, case.source_rect.3).unwrap();
    let mut copier = match implementation {
        Implementation::AngleCopy => Copier::Angle(AngleCopy::new()),
        Implementation::MetalBlit => Copier::Metal {
            blit: MetalBlit::new(&target),
            pending: None,
        },
    };
    let _renderer = match &copier {
        Copier::Angle(copy) if case.renderer_context => Some(RendererContext::make_current(copy)),
        _ => None,
    };

    let copy_frame = |copier: &mut Copier, frame: usize| {
        let texture = &textures[frame % textures.len()];
        let started = Instant::now();
        copier.copy(texture, &target, case.source_rect);
        started.elapsed()
    };

    for frame in 0..WARMUP_COPIES {
        copy_frame(&mut copier, frame);
        copier.finish();
    }
    if let Copier::Angle(copy) = &mut copier {
        copy.take_stats();
    }

    // Guest frames arrive far slower than this copy completes, so drain the GPU
    // between copies to keep queue backpressure out of the duration measurement.
    let mut cpu = Vec::with_capacity(copies);
    let mut gpu = Vec::with_capacity(copies);
    for frame in WARMUP_COPIES..WARMUP_COPIES + copies {
        cpu.push(copy_frame(&mut copier, frame));
        gpu.extend(copier.finish());
    }
    let stats = match &mut copier {
        Copier::Angle(copy) => copy.take_stats(),
        Copier::Metal { .. } => Default::default(),
    };

    let mut completion = Vec::with_capacity(copies);
    for frame in 0..copies {
        let started = Instant::now();
        copy_frame(&mut copier, frame);
        copier.finish();
        completion.push(started.elapsed());
    }

    RunResult {
        copy_cpu_ms: Distribution::new(&cpu),
        completion_latency_ms: Distribution::new(&completion),
        gpu_execution_ms: (!gpu.is_empty()).then(|| Distribution::new(&gpu)),
        copies: copies as u64,
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
    let cases: Vec<_> = [Implementation::AngleCopy, Implementation::MetalBlit]
        .into_iter()
        .flat_map(|implementation| {
            BENCH_CASES
                .iter()
                .filter(|case| case.name.contains(&filter))
                .filter(move |case| implementation == Implementation::AngleCopy || case.renderer_context)
                .map(move |case| (implementation, case))
        })
        .collect();

    let started = Instant::now();
    let mut results: Vec<Vec<RunResult>> = cases.iter().map(|_| Vec::new()).collect();
    // Interleave cases so slow drift (thermals, background load) spreads across all of them.
    for _ in 0..runs {
        for ((implementation, case), results) in cases.iter().zip(&mut results) {
            results.push(run_case(case, *implementation, copies));
        }
    }

    let cases = cases
        .into_iter()
        .zip(results)
        .map(|((implementation, case), runs)| {
            let total_copies = runs.iter().map(|r| r.copies).sum::<u64>() as f64;
            CaseResult {
                implementation,
                name: case.name,
                texture_size: case.texture_size,
                source_rect: case.source_rect,
                textures: case.textures,
                renderer_context: case.renderer_context,
                copy_cpu_median_ms: median(runs.iter().map(|r| r.copy_cpu_ms.median).collect()),
                copy_cpu_p95_ms: median(runs.iter().map(|r| r.copy_cpu_ms.p95).collect()),
                copy_cpu_mean_ms: median(runs.iter().map(|r| r.copy_cpu_ms.mean).collect()),
                completion_latency_median_ms: median(runs.iter().map(|r| r.completion_latency_ms.median).collect()),
                gpu_execution_median_ms: runs
                    .iter()
                    .map(|r| r.gpu_execution_ms.as_ref().map(|d| d.median))
                    .collect::<Option<Vec<_>>>()
                    .map(median),
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
