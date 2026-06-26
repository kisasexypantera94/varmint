use crate::iosurface::ScopedIOSurface;
use core_graphics_types::geometry::CGSize;
use foreign_types::ForeignType;
use metal::{
    CAMetalLayer, CommandQueue, Device, MTLOrigin, MTLPixelFormat, MTLRegion, MTLSize,
    MTLStorageMode, MTLTextureType, MTLTextureUsage, MetalLayer, Texture, TextureDescriptor,
    TextureRef,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use raw_window_metal::Layer;
use std::ffi::{c_char, c_void};
use winit::window::Window;

struct IOSurfaceBackedTexture {
    surface_id: u32,
    surface: ScopedIOSurface,
    texture: Texture,
}

pub struct Presenter {
    _window: Window,

    layer: MetalLayer,
    device: Device,
    queue: CommandQueue,

    texture: Option<Texture>,
    tex_w: u32,
    tex_h: u32,

    iosurface_texture: Option<IOSurfaceBackedTexture>,

    drawable_w: u32,
    drawable_h: u32,
}

type Sel = *mut c_void;

#[link(name = "objc")]
unsafe extern "C" {
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
}

unsafe fn new_texture_with_iosurface(
    device: &Device,
    desc: &TextureDescriptor,
    surface: crate::iosurface::IOSurfaceRef,
) -> Option<Texture> {
    let sel_name = b"newTextureWithDescriptor:iosurface:plane:\0";
    let sel = unsafe { sel_registerName(sel_name.as_ptr() as *const c_char) };

    let send: unsafe extern "C" fn(
        *mut c_void,
        Sel,
        *const c_void,
        crate::iosurface::IOSurfaceRef,
        usize,
    ) -> *mut metal::MTLTexture = unsafe { std::mem::transmute(objc_msgSend as *const ()) };

    let texture = unsafe {
        send(
            device.as_ptr() as *mut c_void,
            sel,
            desc.as_ptr() as *const c_void,
            surface,
            0,
        )
    };

    if texture.is_null() {
        None
    } else {
        Some(unsafe { Texture::from_ptr(texture) })
    }
}

impl Presenter {
    pub fn new(window: Window, width: u32, height: u32) -> Self {
        let raw = window.window_handle().expect("window handle").as_raw();

        let raw_layer = match raw {
            RawWindowHandle::AppKit(handle) => {
                // SAFETY: winit's AppKit handle is a valid NSView.
                unsafe { Layer::from_ns_view(handle.ns_view) }
            }
            other => panic!("unsupported window handle for Metal presenter: {other:?}"),
        };

        let layer_ptr = raw_layer.into_raw();

        // SAFETY: raw-window-metal returned a retained CAMetalLayer pointer.
        let layer = unsafe { MetalLayer::from_ptr(layer_ptr.as_ptr() as *mut CAMetalLayer) };

        let device = Device::system_default().expect("no Metal device");
        let queue = device.new_command_queue();
        eprintln!("present: IOSurface-backed upload path enabled");
        eprintln!("present: ANGLE MTLTexture -> IOSurface copy path enabled");
        layer.set_device(&device);
        layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        layer.set_framebuffer_only(false);

        // Guest scanout data is effectively XRGB in many paths; do not let
        // WindowServer/CoreAnimation treat guest alpha as real window alpha.
        layer.set_opaque(true);
        layer.remove_all_animations();

        layer.set_presents_with_transaction(false);
        layer.set_display_sync_enabled(false);
        layer.set_drawable_size(CGSize {
            width: width.max(1) as f64,
            height: height.max(1) as f64,
        });

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
        }
    }

    fn set_drawable_size_if_needed(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);

        if self.drawable_w == width && self.drawable_h == height {
            return;
        }

        self.layer.set_drawable_size(CGSize {
            width: width as f64,
            height: height as f64,
        });

        self.drawable_w = width;
        self.drawable_h = height;
    }

    pub fn resize_surface(&mut self, width: u32, height: u32) {
        self.set_drawable_size_if_needed(width, height);
    }

    fn ensure_texture(&mut self, w: u32, h: u32) -> bool {
        if self.tex_w == w && self.tex_h == h && self.texture.is_some() {
            return false;
        }

        let desc = TextureDescriptor::new();
        desc.set_texture_type(MTLTextureType::D2);
        desc.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        desc.set_width(w as u64);
        desc.set_height(h as u64);
        desc.set_depth(1);
        desc.set_mipmap_level_count(1);
        desc.set_storage_mode(MTLStorageMode::Shared);
        desc.set_usage(MTLTextureUsage::ShaderRead);

        let texture = self.device.new_texture(&desc);

        self.texture = Some(texture);
        self.tex_w = w;
        self.tex_h = h;

        true
    }

    fn upload_rect_to_texture(
        texture: &TextureRef,
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

        let bytes: &[u8] =
            unsafe { std::slice::from_raw_parts(pixels.as_ptr() as *const u8, pixels.len() * 4) };

        let row_bytes = pw as usize * 4;
        let start = y as usize * row_bytes + x as usize * 4;
        let last = start + (h as usize - 1) * row_bytes + w as usize * 4;
        if last > bytes.len() {
            return false;
        }

        texture.replace_region(
            MTLRegion::new_2d(x as u64, y as u64, w as u64, h as u64),
            0,
            bytes[start..].as_ptr() as *const c_void,
            row_bytes as u64,
        );

        true
    }

    fn ensure_iosurface_texture_for_id(
        &mut self,
        surface_id: u32,
        width: u32,
        height: u32,
    ) -> bool {
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

        let desc = TextureDescriptor::new();
        desc.set_texture_type(MTLTextureType::D2);
        desc.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        desc.set_width(width as u64);
        desc.set_height(height as u64);
        desc.set_depth(1);
        desc.set_mipmap_level_count(1);
        desc.set_storage_mode(MTLStorageMode::Shared);
        desc.set_usage(MTLTextureUsage::ShaderRead);

        let Some(texture) =
            (unsafe { new_texture_with_iosurface(&self.device, &desc, surface.as_ptr()) })
        else {
            eprintln!(
                "present: failed to create Metal texture for producer IOSurface id={surface_id}"
            );
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
    ) -> bool {
        if let Some(iosurface_id) = iosurface_id {
            self.set_drawable_size_if_needed(pw, ph);

            if self.ensure_iosurface_texture_for_id(iosurface_id, pw, ph) {
                if let Some(backing) = self.iosurface_texture.as_ref() {
                    return self.blit_texture_to_drawable(backing.texture.as_ref(), pw, ph);
                }
            }

            return false;
        }

        self.present_rect(pixels, pw, ph, dirty_x, dirty_y, dirty_w, dirty_h)
    }

    pub fn present_rect(
        &mut self,
        pixels: &[u32],
        pw: u32,
        ph: u32,
        dirty_x: u32,
        dirty_y: u32,
        dirty_w: u32,
        dirty_h: u32,
    ) -> bool {
        if pw == 0 || ph == 0 {
            return false;
        }

        self.set_drawable_size_if_needed(pw, ph);

        let force_full = self.ensure_texture(pw, ph);

        let Some(texture) = self.texture.as_ref() else {
            return false;
        };

        if !Self::upload_rect_to_texture(
            texture.as_ref(),
            pixels,
            pw,
            ph,
            dirty_x,
            dirty_y,
            dirty_w,
            dirty_h,
            force_full,
        ) {
            return false;
        }

        self.blit_texture_to_drawable(texture.as_ref(), pw, ph)
    }

    fn blit_texture_to_drawable(&self, src: &TextureRef, width: u32, height: u32) -> bool {
        let Some(drawable) = self.layer.next_drawable() else {
            return false;
        };

        let dst = drawable.texture();

        let copy_w = (width as u64).min(src.width()).min(dst.width());
        let copy_h = (height as u64).min(src.height()).min(dst.height());
        if copy_w == 0 || copy_h == 0 {
            return false;
        }

        let command_buffer = self.queue.new_command_buffer();
        let blit = command_buffer.new_blit_command_encoder();

        blit.copy_from_texture(
            src,
            0,
            0,
            MTLOrigin { x: 0, y: 0, z: 0 },
            MTLSize {
                width: copy_w,
                height: copy_h,
                depth: 1,
            },
            dst,
            0,
            0,
            MTLOrigin { x: 0, y: 0, z: 0 },
        );

        blit.end_encoding();
        command_buffer.present_drawable(drawable);

        command_buffer.commit();

        true
    }
}
