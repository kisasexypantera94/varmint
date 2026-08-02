use super::{
    iosurface::ScopedIOSurface,
    stats::{HEIGHT as STATS_HEIGHT, MARGIN as STATS_MARGIN, StatsHud, WIDTH as STATS_WIDTH},
};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_foundation::NSSize;
use objc2_metal::{
    MTLBlitCommandEncoder, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue, MTLCreateSystemDefaultDevice,
    MTLDevice, MTLDrawable, MTLOrigin, MTLPixelFormat, MTLRegion, MTLSize, MTLStorageMode, MTLTexture,
    MTLTextureDescriptor, MTLTextureUsage,
};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use raw_window_metal::Layer;
use std::{ffi::c_void, ptr::NonNull, time::Instant};
use winit::window::Window;

type MetalDevice = ProtocolObject<dyn MTLDevice>;
type MetalCommandQueue = ProtocolObject<dyn MTLCommandQueue>;
type MetalTexture = ProtocolObject<dyn MTLTexture>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PresentMode {
    NewFrame,
    Redraw,
}

struct IOSurfaceBackedTexture {
    surface_id: u32,
    surface: ScopedIOSurface,
    texture: Retained<MetalTexture>,
}

pub struct Presenter {
    _window: Window,

    layer: Retained<CAMetalLayer>,
    device: Retained<MetalDevice>,
    queue: Retained<MetalCommandQueue>,

    texture: Option<Retained<MetalTexture>>,
    tex_w: u32,
    tex_h: u32,

    iosurface_texture: Option<IOSurfaceBackedTexture>,

    drawable_w: u32,
    drawable_h: u32,

    stats: StatsHud,
    stats_texture: Option<Retained<MetalTexture>>,
}

fn bgra_texture_descriptor(width: u32, height: u32) -> Retained<MTLTextureDescriptor> {
    let desc = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            MTLPixelFormat::BGRA8Unorm,
            width.max(1) as usize,
            height.max(1) as usize,
            false,
        )
    };

    desc.setStorageMode(MTLStorageMode::Shared);
    desc.setUsage(MTLTextureUsage::ShaderRead);

    desc
}

impl Presenter {
    pub fn window(&self) -> &Window {
        &self._window
    }

    pub fn new(window: Window, width: u32, height: u32) -> Self {
        let raw = window.window_handle().expect("window handle").as_raw();

        let raw_layer = match raw {
            RawWindowHandle::AppKit(handle) => unsafe { Layer::from_ns_view(handle.ns_view) },
            other => panic!("unsupported window handle for Metal presenter: {other:?}"),
        };

        let layer_ptr = raw_layer.into_raw();

        let layer = unsafe { Retained::<CAMetalLayer>::from_raw(layer_ptr.as_ptr().cast()) }
            .expect("raw-window-metal returned null CAMetalLayer");

        let device = MTLCreateSystemDefaultDevice().expect("no Metal device");
        let queue = device.newCommandQueue().expect("failed to create Metal command queue");

        layer.setDevice(Some(device.as_ref()));
        layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm);

        layer.setFramebufferOnly(false);

        // Guest scanout data is effectively XRGB in many paths; do not let
        // WindowServer/CoreAnimation treat guest alpha as real window alpha.
        layer.setOpaque(true);
        layer.removeAllAnimations();

        layer.setPresentsWithTransaction(false);
        layer.setDisplaySyncEnabled(false);
        layer.setDrawableSize(NSSize::new(width.max(1) as f64, height.max(1) as f64));

        Self {
            _window: window,
            layer,
            device,
            queue,
            texture: None,
            tex_w: 0,
            tex_h: 0,
            iosurface_texture: None,
            drawable_w: width.max(1),
            drawable_h: height.max(1),
            stats: StatsHud::new(),
            stats_texture: None,
        }
    }

    pub fn resize_surface(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);

        if self.drawable_w == width && self.drawable_h == height {
            return;
        }

        self.layer.setDrawableSize(NSSize::new(width as f64, height as f64));

        self.drawable_w = width;
        self.drawable_h = height;
    }

    pub fn set_stats_visible(&mut self, visible: bool) {
        self.stats.set_visible(visible);
        self.sync_stats_texture();
    }

    fn sync_stats_texture(&mut self) {
        let Some(pixels) = self.stats.take_pixels() else {
            return;
        };

        if self.stats_texture.is_none() {
            let desc = bgra_texture_descriptor(STATS_WIDTH, STATS_HEIGHT);
            self.stats_texture = self.device.newTextureWithDescriptor(&desc);
        }

        let Some(texture) = self.stats_texture.as_ref() else {
            return;
        };
        let _ = Self::upload_rect_to_texture(
            texture.as_ref(),
            &pixels,
            STATS_WIDTH,
            STATS_HEIGHT,
            0,
            0,
            STATS_WIDTH,
            STATS_HEIGHT,
            true,
        );
    }

    fn ensure_texture(&mut self, w: u32, h: u32) -> bool {
        if self.tex_w == w && self.tex_h == h && self.texture.is_some() {
            return false;
        }

        let desc = bgra_texture_descriptor(w, h);
        let Some(texture) = self.device.newTextureWithDescriptor(&desc) else {
            self.texture = None;
            self.tex_w = 0;
            self.tex_h = 0;
            return false;
        };

        self.texture = Some(texture);
        self.tex_w = w;
        self.tex_h = h;

        true
    }

    fn upload_rect_to_texture(
        texture: &MetalTexture,
        pixels: &[u32],
        pw: u32,
        ph: u32,
        dirty_x: u32,
        dirty_y: u32,
        dirty_w: u32,
        dirty_h: u32,
        force_full: bool,
    ) -> bool {
        if pw == 0 || ph == 0 || pixels.len() < (pw * ph) as usize {
            return false;
        }

        let (x, y, w, h) = if force_full {
            (0, 0, pw, ph)
        } else {
            let x = dirty_x.min(pw);
            let y = dirty_y.min(ph);
            let w = dirty_w.min(pw.saturating_sub(x));
            let h = dirty_h.min(ph.saturating_sub(y));
            (x, y, w, h)
        };

        if w == 0 || h == 0 {
            return false;
        }

        let bytes: &[u8] = unsafe { std::slice::from_raw_parts(pixels.as_ptr() as *const u8, pixels.len() * 4) };

        let row_bytes = pw as usize * 4;
        let start = y as usize * row_bytes + x as usize * 4;
        let last = start + (h as usize - 1) * row_bytes + w as usize * 4;
        if last > bytes.len() {
            return false;
        }

        let Some(src_bytes) = NonNull::new(bytes[start..].as_ptr() as *mut c_void) else {
            return false;
        };

        let region = MTLRegion {
            origin: MTLOrigin {
                x: x as usize,
                y: y as usize,
                z: 0,
            },
            size: MTLSize {
                width: w as usize,
                height: h as usize,
                depth: 1,
            },
        };

        unsafe {
            texture.replaceRegion_mipmapLevel_withBytes_bytesPerRow(region, 0, src_bytes, row_bytes);
        }

        true
    }

    fn ensure_iosurface_texture_for_id(&mut self, surface_id: u32, width: u32, height: u32) -> bool {
        if let Some(backing) = self.iosurface_texture.as_ref() {
            if backing.surface_id == surface_id
                && backing.surface.width() == width
                && backing.surface.height() == height
            {
                return true;
            }
        }

        let Some(surface) = ScopedIOSurface::lookup(surface_id, width, height) else {
            eprintln!("present: IOSurfaceLookup({surface_id}) failed");
            self.iosurface_texture = None;
            return false;
        };

        let desc = bgra_texture_descriptor(width, height);
        let Some(texture) = self
            .device
            .newTextureWithDescriptor_iosurface_plane(&desc, surface.as_objc_ref(), 0)
        else {
            eprintln!("present: failed to create Metal texture for producer IOSurface id={surface_id}");
            self.iosurface_texture = None;
            return false;
        };

        self.iosurface_texture = Some(IOSurfaceBackedTexture {
            surface_id,
            surface,
            texture,
        });

        true
    }

    pub fn present_iosurface_or_rect(
        &mut self,
        iosurface_id: Option<u32>,
        pixels: &[u32],
        pw: u32,
        ph: u32,
        dirty_x: u32,
        dirty_y: u32,
        dirty_w: u32,
        dirty_h: u32,
        mode: PresentMode,
    ) -> bool {
        let started = (mode == PresentMode::NewFrame && self.stats.is_visible()).then(Instant::now);

        let presented = if let Some(iosurface_id) = iosurface_id {
            self.present_iosurface(iosurface_id, pw, ph, mode)
        } else {
            self.present_rect(pixels, pw, ph, dirty_x, dirty_y, dirty_w, dirty_h, mode)
        };

        if let Some(started) = started {
            self.stats.record_frame(presented, started.elapsed());
        }
        self.stats.refresh();
        self.sync_stats_texture();

        presented
    }

    fn present_iosurface(&mut self, iosurface_id: u32, width: u32, height: u32, mode: PresentMode) -> bool {
        if mode == PresentMode::NewFrame {
            self.resize_surface(width, height);
            if !self.ensure_iosurface_texture_for_id(iosurface_id, width, height) {
                return false;
            }
        }

        let Some(backing) = self.iosurface_texture.as_ref() else {
            return false;
        };
        if backing.surface_id != iosurface_id {
            return false;
        }

        self.blit_texture_to_drawable(backing.texture.as_ref(), width, height)
    }

    fn present_rect(
        &mut self,
        pixels: &[u32],
        width: u32,
        height: u32,
        dirty_x: u32,
        dirty_y: u32,
        dirty_w: u32,
        dirty_h: u32,
        mode: PresentMode,
    ) -> bool {
        if width == 0 || height == 0 {
            return false;
        }

        if mode == PresentMode::Redraw {
            let Some(texture) = self.texture.as_ref() else {
                return false;
            };
            return self.blit_texture_to_drawable(texture.as_ref(), width, height);
        }

        self.resize_surface(width, height);
        let force_full = self.ensure_texture(width, height);
        let Some(texture) = self.texture.as_ref() else {
            return false;
        };

        if !Self::upload_rect_to_texture(
            texture.as_ref(),
            pixels,
            width,
            height,
            dirty_x,
            dirty_y,
            dirty_w,
            dirty_h,
            force_full,
        ) {
            return false;
        }

        self.blit_texture_to_drawable(texture.as_ref(), width, height)
    }

    fn blit_texture_to_drawable(&self, src: &MetalTexture, width: u32, height: u32) -> bool {
        let Some(drawable) = self.layer.nextDrawable() else {
            return false;
        };

        let dst = drawable.texture();

        let copy_w = (width as usize).min(src.width()).min(dst.width());
        let copy_h = (height as usize).min(src.height()).min(dst.height());
        if copy_w == 0 || copy_h == 0 {
            return false;
        }

        let Some(command_buffer) = self.queue.commandBuffer() else {
            return false;
        };
        let Some(blit) = command_buffer.blitCommandEncoder() else {
            return false;
        };

        let source_size = MTLSize {
            width: copy_w,
            height: copy_h,
            depth: 1,
        };

        let zero_origin = MTLOrigin { x: 0, y: 0, z: 0 };

        unsafe {
            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                src,
                0,
                0,
                zero_origin,
                source_size,
                dst.as_ref(),
                0,
                0,
                zero_origin,
            );
        }

        if self.stats.is_visible() {
            if let Some(stats_texture) = self.stats_texture.as_ref() {
                let stats_w = stats_texture.width().min(dst.width());
                let stats_h = stats_texture.height().min(dst.height());
                if stats_w > 0 && stats_h > 0 {
                    let stats_size = MTLSize {
                        width: stats_w,
                        height: stats_h,
                        depth: 1,
                    };
                    let destination_origin = MTLOrigin {
                        x: STATS_MARGIN.min(dst.width().saturating_sub(stats_w)),
                        y: STATS_MARGIN.min(dst.height().saturating_sub(stats_h)),
                        z: 0,
                    };
                    unsafe {
                        blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                            stats_texture.as_ref(),
                            0,
                            0,
                            zero_origin,
                            stats_size,
                            dst.as_ref(),
                            0,
                            0,
                            destination_origin,
                        );
                    }
                }
            }
        }

        blit.endEncoding();

        let drawable_for_present: &ProtocolObject<dyn MTLDrawable> = drawable.as_ref();
        command_buffer.presentDrawable(drawable_for_present);
        command_buffer.commit();

        true
    }
}
