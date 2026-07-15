use std::{
    collections::HashMap,
    ffi::{CStr, c_char, c_int, c_void},
    os::raw::c_uint,
    ptr::NonNull,
    sync::OnceLock,
};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Iovec {
    pub iov_base: *mut c_void,
    pub iov_len: usize,
}

pub struct ResourceCreate3D {
    pub target: u32,
    pub format: u32,
    pub bind: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub array_size: u32,
    pub last_level: u32,
    pub nr_samples: u32,
    pub flags: u32,
}

pub struct Transfer3D {
    pub x: u32,
    pub y: u32,
    pub z: u32,
    pub w: u32,
    pub h: u32,
    pub d: u32,
    pub level: u32,
    pub stride: u32,
    pub layer_stride: u32,
    pub offset: u64,
}

pub struct ResourceCreateBlob {
    pub blob_mem: u32,
    pub blob_flags: u32,
    pub blob_id: u64,
    pub size: u64,
}

#[derive(Debug, Copy, Clone)]
pub struct NativeTexture {
    handle: NonNull<c_void>,
    width: u32,
    height: u32,
}

impl NativeTexture {
    pub fn as_ptr(self) -> *mut c_void {
        self.handle.as_ptr()
    }

    pub fn size(self) -> (u32, u32) {
        (self.width, self.height)
    }
}

#[derive(Clone, Copy)]
pub struct VirglFence {
    pub fence_id: u64,
    pub ctx_id: u32,
    pub ring_idx: Option<u32>,
}

#[repr(C)]
struct VirglBox {
    x: u32,
    y: u32,
    z: u32,
    w: u32,
    h: u32,
    d: u32,
}

#[repr(C)]
struct ResourceCreateArgs {
    handle: u32,
    target: u32,
    format: u32,
    bind: u32,
    width: u32,
    height: u32,
    depth: u32,
    array_size: u32,
    last_level: u32,
    nr_samples: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct VirglRendererResourceInfo {
    handle: u32,
    virgl_format: u32,
    width: u32,
    height: u32,
    depth: u32,
    flags: u32,
    tex_id: u32,
    stride: u32,
    drm_fourcc: c_int,
    fd: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
union VirglRendererNativeHandleUnion {
    d3d_tex2d: *mut c_void,
    native_handle: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VirglRendererResourceInfoExt {
    version: c_int,
    base: VirglRendererResourceInfo,
    has_dmabuf_export: bool,
    planes: c_int,
    modifiers: u64,
    handle: VirglRendererNativeHandleUnion,
    native_type: c_int,
}

const VIRGL_RENDERER_RESOURCE_INFO_EXT_VERSION: c_int = 1;
const VIRGL_NATIVE_HANDLE_METAL_TEXTURE: c_int = 2;

#[repr(C)]
struct ResourceCreateBlobArgs {
    res_handle: u32,
    ctx_id: u32,
    blob_mem: u32,
    blob_flags: u32,
    blob_id: u64,
    size: u64,
    iovecs: *const Iovec,
    num_iovs: u32,
}

#[repr(C)]
struct VirglRendererGlCtxParam {
    version: c_int,
    shared: bool,
    major_ver: c_int,
    minor_ver: c_int,
    compat_ctx: c_int,
}

#[repr(C)]
struct VirglRendererCallbacks {
    version: c_int,
    write_fence: Option<extern "C" fn(*mut c_void, u32)>,

    create_gl_context: Option<extern "C" fn(*mut c_void, c_int, *mut VirglRendererGlCtxParam) -> *mut c_void>,
    destroy_gl_context: Option<extern "C" fn(*mut c_void, *mut c_void)>,
    make_current: Option<extern "C" fn(*mut c_void, c_int, *mut c_void) -> c_int>,

    get_drm_fd: Option<extern "C" fn(*mut c_void) -> c_int>,

    write_context_fence: Option<extern "C" fn(*mut c_void, u32, u32, u64)>,

    get_server_fd: Option<extern "C" fn(*mut c_void, u32) -> c_int>,

    get_egl_display: Option<extern "C" fn(*mut c_void) -> *mut c_void>,
}

pub const CAPSET_VIRGL: u32 = 1;
pub const CAPSET_VIRGL2: u32 = 2;
pub const CAPSET_VENUS: u32 = 4;

const VIRGL_RENDERER_THREAD_SYNC: c_int = 1 << 1;
const VIRGL_RENDERER_USE_SURFACELESS: c_int = 1 << 3;
const VIRGL_RENDERER_USE_GLES: c_int = 1 << 4;
const VIRGL_RENDERER_USE_EXTERNAL_BLOB: c_int = 1 << 5;
const VIRGL_RENDERER_VENUS: c_int = 1 << 6;
const VIRGL_RENDERER_NATIVE_SHARE_TEXTURE: c_int = 1 << 12;
const VIRGL_RENDERER_ASYNC_FENCE_CB: c_int = 1 << 8;

const BASE_INIT_FLAGS: c_int = VIRGL_RENDERER_THREAD_SYNC
    | VIRGL_RENDERER_USE_SURFACELESS
    | VIRGL_RENDERER_USE_GLES
    | VIRGL_RENDERER_USE_EXTERNAL_BLOB
    | VIRGL_RENDERER_VENUS
    | VIRGL_RENDERER_ASYNC_FENCE_CB
    | VIRGL_RENDERER_NATIVE_SHARE_TEXTURE;

#[link(name = "virglrenderer")]
unsafe extern "C" {
    fn virgl_set_log_callback(
        cb: Option<extern "C" fn(c_int, *const c_char, *mut c_void)>,
        user_data: *mut c_void,
        free_user_data_cb: Option<extern "C" fn(*mut c_void)>,
    );

    fn virgl_renderer_init(cookie: *mut c_void, flags: c_int, cb: *mut VirglRendererCallbacks) -> c_int;
    fn virgl_renderer_context_poll(ctx_id: u32);
    fn virgl_renderer_cleanup(cookie: *mut c_void);

    fn virgl_renderer_get_cap_set(set: u32, max_ver: *mut u32, max_size: *mut u32);
    fn virgl_renderer_fill_caps(set: u32, version: u32, caps: *mut c_void);

    fn virgl_renderer_context_create_with_flags(ctx_id: u32, ctx_flags: u32, nlen: u32, name: *const c_char) -> c_int;
    fn virgl_renderer_context_destroy(handle: u32);

    fn virgl_renderer_ctx_attach_resource(ctx_id: c_int, res_handle: c_int);
    fn virgl_renderer_ctx_detach_resource(ctx_id: c_int, res_handle: c_int);

    fn virgl_renderer_submit_cmd(buffer: *mut c_void, ctx_id: c_int, ndw: c_int) -> c_int;

    fn virgl_renderer_create_fence(fence_id: c_int, ctx_id: u32) -> c_int;
    fn virgl_renderer_context_create_fence(ctx_id: u32, flags: u32, ring_idx: u32, fence_id: u64) -> c_int;

    fn virgl_renderer_resource_create(args: *mut ResourceCreateArgs, iov: *mut Iovec, num_iovs: u32) -> c_int;

    fn virgl_renderer_resource_create_blob(args: *const ResourceCreateBlobArgs) -> c_int;
    fn virgl_renderer_resource_unref(res_handle: u32);

    fn virgl_renderer_resource_attach_iov(res_handle: c_int, iov: *mut Iovec, num_iovs: c_int) -> c_int;
    fn virgl_renderer_resource_detach_iov(res_handle: c_int, iov: *mut *mut Iovec, num_iovs: *mut c_int);

    fn virgl_renderer_transfer_write_iov(
        handle: u32,
        ctx_id: u32,
        level: c_int,
        stride: u32,
        layer_stride: u32,
        r#box: *mut VirglBox,
        offset: u64,
        iovec: *mut Iovec,
        iovec_cnt: c_uint,
    ) -> c_int;

    fn virgl_renderer_transfer_read_iov(
        handle: u32,
        ctx_id: u32,
        level: u32,
        stride: u32,
        layer_stride: u32,
        r#box: *mut VirglBox,
        offset: u64,
        iov: *mut Iovec,
        iovec_cnt: c_int,
    ) -> c_int;

    fn virgl_renderer_resource_map(res_handle: u32, map: *mut *mut c_void, out_size: *mut u64) -> c_int;

    fn virgl_renderer_resource_unmap(res_handle: u32) -> c_int;

    fn virgl_renderer_resource_get_map_info(res_handle: u32, map_info: *mut u32) -> c_int;

    fn virgl_renderer_resource_get_info_ext(res_handle: c_int, info: *mut VirglRendererResourceInfoExt) -> c_int;
}

extern "C" fn virgl_log_trampoline(level: c_int, message: *const c_char, _user_data: *mut c_void) {
    if message.is_null() {
        return;
    }

    let msg = unsafe { CStr::from_ptr(message) }.to_string_lossy();
    eprintln!("virglrenderer[{level}]: {msg}");
}

// ---- ANGLE/EGL caller winsys ----

type EglDisplay = *mut c_void;
type EglContext = *mut c_void;
type EglSurface = *mut c_void;
type EglConfig = *mut c_void;
type EglBoolean = c_uint;
type EglInt = c_int;
type EglEnum = c_uint;

const EGL_FALSE: EglBoolean = 0;
const EGL_NONE: EglInt = 0x3038;
const EGL_OPENGL_ES_API: EglEnum = 0x30A0;
const EGL_DEFAULT_DISPLAY: *mut c_void = std::ptr::null_mut();

const EGL_PLATFORM_ANGLE_ANGLE: EglEnum = 0x3202;
const EGL_PLATFORM_ANGLE_TYPE_ANGLE: EglInt = 0x3203;
const EGL_PLATFORM_ANGLE_TYPE_METAL_ANGLE: EglInt = 0x3489;

const EGL_SURFACE_TYPE: EglInt = 0x3033;
const EGL_PBUFFER_BIT: EglInt = 0x0001;
const EGL_RENDERABLE_TYPE: EglInt = 0x3040;
const EGL_OPENGL_ES2_BIT: EglInt = 0x0004;
const EGL_RED_SIZE: EglInt = 0x3024;
const EGL_GREEN_SIZE: EglInt = 0x3023;
const EGL_BLUE_SIZE: EglInt = 0x3022;
const EGL_ALPHA_SIZE: EglInt = 0x3021;
const EGL_WIDTH: EglInt = 0x3057;
const EGL_HEIGHT: EglInt = 0x3056;
const EGL_CONTEXT_CLIENT_VERSION: EglInt = 0x3098;

type EglGetPlatformDisplayExt = unsafe extern "C" fn(EglEnum, *mut c_void, *const EglInt) -> EglDisplay;

#[link(name = "EGL")]
unsafe extern "C" {
    fn eglGetProcAddress(name: *const c_char) -> *const c_void;
    fn eglGetError() -> EglInt;
    fn eglInitialize(dpy: EglDisplay, major: *mut EglInt, minor: *mut EglInt) -> EglBoolean;
    fn eglBindAPI(api: EglEnum) -> EglBoolean;
    fn eglChooseConfig(
        dpy: EglDisplay,
        attribs: *const EglInt,
        configs: *mut EglConfig,
        config_size: EglInt,
        num_config: *mut EglInt,
    ) -> EglBoolean;
    fn eglCreatePbufferSurface(dpy: EglDisplay, config: EglConfig, attribs: *const EglInt) -> EglSurface;
    fn eglCreateContext(
        dpy: EglDisplay,
        config: EglConfig,
        share_context: EglContext,
        attribs: *const EglInt,
    ) -> EglContext;
    fn eglDestroyContext(dpy: EglDisplay, ctx: EglContext) -> EglBoolean;
    fn eglMakeCurrent(dpy: EglDisplay, draw: EglSurface, read: EglSurface, ctx: EglContext) -> EglBoolean;
}

#[derive(Clone, Copy)]
struct AngleGlobals {
    display: EglDisplay,
    config: EglConfig,
    surface: EglSurface,
    share_context: EglContext,
}

unsafe impl Send for AngleGlobals {}
unsafe impl Sync for AngleGlobals {}

static ANGLE_GLOBAL: OnceLock<Result<AngleGlobals, EglInt>> = OnceLock::new();

fn angle_globals() -> Option<AngleGlobals> {
    ANGLE_GLOBAL.get().and_then(|r| r.ok())
}

fn angle_display() -> EglDisplay {
    angle_globals().map_or(std::ptr::null_mut(), |g| g.display)
}

fn angle_config() -> EglConfig {
    angle_globals().map_or(std::ptr::null_mut(), |g| g.config)
}

fn angle_surface() -> EglSurface {
    angle_globals().map_or(std::ptr::null_mut(), |g| g.surface)
}

fn angle_share_context() -> EglContext {
    angle_globals().map_or(std::ptr::null_mut(), |g| g.share_context)
}

fn angle_init_once() -> Result<(), EglInt> {
    match ANGLE_GLOBAL.get_or_init(|| angle_init_impl()) {
        Ok(_) => Ok(()),
        Err(e) => Err(*e),
    }
}

fn angle_init_impl() -> Result<AngleGlobals, EglInt> {
    let proc_name = b"eglGetPlatformDisplayEXT\0";
    let proc = unsafe { eglGetProcAddress(proc_name.as_ptr() as *const c_char) };
    if proc.is_null() {
        eprintln!("ANGLE: eglGetPlatformDisplayEXT not found");
        return Err(-1);
    }

    let get_platform_display: EglGetPlatformDisplayExt = unsafe { std::mem::transmute(proc) };

    let display_attribs = [
        EGL_PLATFORM_ANGLE_TYPE_ANGLE,
        EGL_PLATFORM_ANGLE_TYPE_METAL_ANGLE,
        EGL_NONE,
    ];

    let dpy = unsafe { get_platform_display(EGL_PLATFORM_ANGLE_ANGLE, EGL_DEFAULT_DISPLAY, display_attribs.as_ptr()) };
    if dpy.is_null() {
        let err = unsafe { eglGetError() };
        eprintln!("ANGLE: eglGetPlatformDisplayEXT failed error={:#x}", err);
        return Err(err);
    }

    let mut major = 0;
    let mut minor = 0;
    if unsafe { eglInitialize(dpy, &mut major, &mut minor) } == EGL_FALSE {
        let err = unsafe { eglGetError() };
        eprintln!("ANGLE: eglInitialize failed error={:#x}", err);
        return Err(err);
    }

    if unsafe { eglBindAPI(EGL_OPENGL_ES_API) } == EGL_FALSE {
        let err = unsafe { eglGetError() };
        eprintln!("ANGLE: eglBindAPI failed error={:#x}", err);
        return Err(err);
    }

    let cfg_attribs = [
        EGL_SURFACE_TYPE,
        EGL_PBUFFER_BIT,
        EGL_RENDERABLE_TYPE,
        EGL_OPENGL_ES2_BIT,
        EGL_RED_SIZE,
        8,
        EGL_GREEN_SIZE,
        8,
        EGL_BLUE_SIZE,
        8,
        EGL_ALPHA_SIZE,
        8,
        EGL_NONE,
    ];

    let mut cfg: EglConfig = std::ptr::null_mut();
    let mut ncfg = 0;
    if unsafe { eglChooseConfig(dpy, cfg_attribs.as_ptr(), &mut cfg, 1, &mut ncfg) } == EGL_FALSE
        || ncfg < 1
        || cfg.is_null()
    {
        let err = unsafe { eglGetError() };
        eprintln!("ANGLE: eglChooseConfig failed error={:#x}", err);
        return Err(err);
    }

    let surf_attribs = [EGL_WIDTH, 1, EGL_HEIGHT, 1, EGL_NONE];
    let surf = unsafe { eglCreatePbufferSurface(dpy, cfg, surf_attribs.as_ptr()) };
    if surf.is_null() {
        let err = unsafe { eglGetError() };
        eprintln!("ANGLE: eglCreatePbufferSurface failed error={:#x}", err);
        return Err(err);
    }

    let ctx_attribs = [EGL_CONTEXT_CLIENT_VERSION, 3, EGL_NONE];
    let share = unsafe { eglCreateContext(dpy, cfg, std::ptr::null_mut(), ctx_attribs.as_ptr()) };
    if share.is_null() {
        let err = unsafe { eglGetError() };
        eprintln!("ANGLE: eglCreateContext(share) failed error={:#x}", err);
        return Err(err);
    }

    eprintln!(
        "ANGLE: initialized EGL {}.{} backend=metal display={:?} surface={:?} share_ctx={:?}",
        major, minor, dpy, surf, share
    );

    Ok(AngleGlobals {
        display: dpy,
        config: cfg,
        surface: surf,
        share_context: share,
    })
}

extern "C" fn create_gl_context_trampoline(
    _cookie: *mut c_void,
    scanout_idx: c_int,
    param: *mut VirglRendererGlCtxParam,
) -> *mut c_void {
    if let Err(err) = angle_init_once() {
        eprintln!("ANGLE: init failed in create_gl_context err={:#x}", err);
        return std::ptr::null_mut();
    }

    let (major, requested_shared) = unsafe {
        if param.is_null() {
            (3, true)
        } else {
            let p = &*param;
            let major = if p.major_ver > 0 { p.major_ver.min(3) } else { 3 };
            (major, p.shared)
        }
    };

    let dpy = angle_display();
    let cfg = angle_config();

    // shared=true is mandatory: an isolated context breaks
    // transfer/sync with "Wait sync failed: illegal fence object".
    let share = angle_share_context();
    let shared = true;

    let ctx_attribs = [EGL_CONTEXT_CLIENT_VERSION, major, EGL_NONE];

    let ctx = unsafe { eglCreateContext(dpy, cfg, share, ctx_attribs.as_ptr()) };
    if ctx.is_null() {
        let err = unsafe { eglGetError() };
        eprintln!(
            "ANGLE: create_gl_context failed scanout_idx={} major={} shared={} requested_shared={} error={:#x}",
            scanout_idx, major, shared, requested_shared, err
        );
    } else {
        eprintln!(
            "ANGLE: create_gl_context scanout_idx={} major={} shared={} requested_shared={} ctx={:?}",
            scanout_idx, major, shared, requested_shared, ctx
        );
    }

    ctx
}

extern "C" fn destroy_gl_context_trampoline(_cookie: *mut c_void, ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }

    let dpy = angle_display();
    let ret = unsafe { eglDestroyContext(dpy, ctx as EglContext) };
    if ret == EGL_FALSE {
        let err = unsafe { eglGetError() };
        eprintln!("ANGLE: destroy_gl_context {:?} failed error={:#x}", ctx, err);
    } else {
        eprintln!("ANGLE: destroy_gl_context {:?}", ctx);
    }
}

extern "C" fn make_current_trampoline(_cookie: *mut c_void, scanout_idx: c_int, ctx: *mut c_void) -> c_int {
    let dpy = angle_display();
    if dpy.is_null() {
        eprintln!("ANGLE: make_current with uninitialised display");
        return -1;
    }

    let ok = if ctx.is_null() {
        unsafe { eglMakeCurrent(dpy, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) }
    } else {
        unsafe { eglMakeCurrent(dpy, angle_surface(), angle_surface(), ctx as EglContext) }
    };

    if ok == EGL_FALSE {
        let err = unsafe { eglGetError() };
        eprintln!(
            "ANGLE: make_current failed scanout_idx={} ctx={:?} error={:#x}",
            scanout_idx, ctx, err
        );
        -1
    } else {
        0
    }
}

extern "C" fn get_egl_display_trampoline(_cookie: *mut c_void) -> *mut c_void {
    angle_display()
}

#[derive(Debug)]
pub struct VirglError(pub c_int);

pub type VirglResult<T> = Result<T, VirglError>;

#[inline]
fn check(ret: c_int) -> VirglResult<()> {
    if ret == 0 { Ok(()) } else { Err(VirglError(ret)) }
}

#[derive(Debug, Clone, Copy)]
pub struct CapsetInfo {
    pub id: u32,
    pub version: u32,
    pub size: u32,
}

type FenceCb = Box<dyn FnMut(VirglFence) + Send>;

extern "C" fn write_fence_trampoline(cookie: *mut c_void, fence_id: u32) {
    let cb = unsafe { &mut *(cookie as *mut FenceCb) };
    cb(VirglFence {
        fence_id: fence_id as u64,
        ctx_id: 0,
        ring_idx: None,
    });
}

extern "C" fn write_context_fence_trampoline(cookie: *mut c_void, ctx_id: u32, ring_idx: u32, fence_id: u64) {
    let cb = unsafe { &mut *(cookie as *mut FenceCb) };
    cb(VirglFence {
        fence_id,
        ctx_id,
        ring_idx: Some(ring_idx),
    });
}

pub struct VirglRenderer {
    cookie: *mut c_void,
    _cb_storage: Box<FenceCb>,
    _callbacks: Box<VirglRendererCallbacks>,
    capsets: Vec<CapsetInfo>,
    poller_contexts: Vec<u32>,
    backing_iovs: HashMap<u32, Box<[Iovec]>>,
}

impl VirglRenderer {
    pub fn new(fence_handler: impl FnMut(VirglFence) + Send + 'static) -> VirglResult<Self> {
        let mut cb_storage: Box<FenceCb> = Box::new(Box::new(fence_handler));
        let cookie = (&mut *cb_storage) as *mut FenceCb as *mut c_void;

        let mut callbacks = Box::new(VirglRendererCallbacks {
            version: 4,
            write_fence: Some(write_fence_trampoline),

            create_gl_context: Some(create_gl_context_trampoline),
            destroy_gl_context: Some(destroy_gl_context_trampoline),
            make_current: Some(make_current_trampoline),

            get_drm_fd: None,

            write_context_fence: Some(write_context_fence_trampoline),

            get_server_fd: None,

            get_egl_display: Some(get_egl_display_trampoline),
        });

        unsafe {
            virgl_set_log_callback(Some(virgl_log_trampoline), std::ptr::null_mut(), None);
        }

        eprintln!(
            "virgl_ffi: callbacks version={} create_gl={} destroy_gl={} make_current={} write_ctx_fence={} get_egl_display={}",
            callbacks.version,
            callbacks.create_gl_context.is_some(),
            callbacks.destroy_gl_context.is_some(),
            callbacks.make_current.is_some(),
            callbacks.write_context_fence.is_some(),
            callbacks.get_egl_display.is_some(),
        );

        let _ = angle_init_once();

        let init_flags = BASE_INIT_FLAGS;
        eprintln!("virgl_ffi: init_flags={:#x}", init_flags);

        let ret = unsafe { virgl_renderer_init(cookie, init_flags, callbacks.as_mut() as *mut _) };
        check(ret)?;

        let mut capsets = Vec::new();

        for id in [CAPSET_VENUS, CAPSET_VIRGL, CAPSET_VIRGL2] {
            let mut version = 0u32;
            let mut size = 0u32;
            unsafe { virgl_renderer_get_cap_set(id, &mut version, &mut size) };

            if size != 0 {
                eprintln!("virgl_ffi: capset id={} version={} size={}", id, version, size);
                capsets.push(CapsetInfo { id, version, size });
            } else {
                eprintln!(
                    "virgl_ffi: capset id={} unavailable version={} size={}",
                    id, version, size
                );
            }
        }

        if capsets.is_empty() {
            return Err(VirglError(-1));
        }

        let poller_contexts = Vec::<u32>::new();

        Ok(VirglRenderer {
            cookie,
            _cb_storage: cb_storage,
            _callbacks: callbacks,
            capsets,
            poller_contexts,
            backing_iovs: HashMap::new(),
        })
    }

    pub fn get_num_capsets(&self) -> u32 {
        self.capsets.len() as u32
    }

    pub fn get_capset_info(&self, capset_index: u32) -> VirglResult<(u32, u32, u32)> {
        let Some(info) = self.capsets.get(capset_index as usize) else {
            return Err(VirglError(-1));
        };
        Ok((info.id, info.version, info.size))
    }

    pub fn get_capset(&self, capset_id: u32, version: u32) -> VirglResult<Vec<u8>> {
        let mut max_ver = 0u32;
        let mut max_size = 0u32;
        unsafe { virgl_renderer_get_cap_set(capset_id, &mut max_ver, &mut max_size) };

        if max_size == 0 {
            return Err(VirglError(-1));
        }

        let version = if max_ver == 0 {
            // UTM Venus reports version=0 size!=0. Version 0 is the valid
            // version to pass to fill_caps in that case.
            0
        } else if version == 0 {
            max_ver
        } else {
            version.min(max_ver)
        };
        let mut caps = vec![0u8; max_size as usize];

        unsafe { virgl_renderer_fill_caps(capset_id, version, caps.as_mut_ptr() as *mut c_void) };

        Ok(caps)
    }

    pub fn create_ctx(&mut self, ctx_id: u32, context_init: u32, name: Option<&str>) -> VirglResult<()> {
        let context_init = if context_init == 0 {
            let selected = if self.capsets.iter().any(|c| c.id == CAPSET_VIRGL2) {
                CAPSET_VIRGL2
            } else if self.capsets.iter().any(|c| c.id == CAPSET_VIRGL) {
                CAPSET_VIRGL
            } else if self.capsets.iter().any(|c| c.id == CAPSET_VENUS) {
                CAPSET_VENUS
            } else {
                return Err(VirglError(-1));
            };

            eprintln!("virgl_ffi: ctx_id={} context_init=0 -> capset {}", ctx_id, selected);

            selected
        } else {
            context_init
        };

        let name_bytes = name.unwrap_or("").as_bytes();
        let ret = unsafe {
            virgl_renderer_context_create_with_flags(
                ctx_id,
                context_init,
                name_bytes.len() as u32,
                name_bytes.as_ptr() as *const c_char,
            )
        };

        if ret == 0 {
            if !self.poller_contexts.contains(&ctx_id) {
                self.poller_contexts.push(ctx_id);
            }
        }

        check(ret)
    }

    pub fn destroy_ctx(&mut self, ctx_id: u32) -> VirglResult<()> {
        self.poller_contexts.retain(|&id| id != ctx_id);

        unsafe { virgl_renderer_context_destroy(ctx_id) };
        Ok(())
    }

    pub fn ctx_attach_resource(&self, ctx_id: u32, resource_id: u32) -> VirglResult<()> {
        unsafe { virgl_renderer_ctx_attach_resource(ctx_id as c_int, resource_id as c_int) };
        Ok(())
    }

    pub fn ctx_detach_resource(&self, ctx_id: u32, resource_id: u32) -> VirglResult<()> {
        unsafe { virgl_renderer_ctx_detach_resource(ctx_id as c_int, resource_id as c_int) };
        Ok(())
    }

    pub fn submit_command(&self, ctx_id: u32, buf: &mut [u8]) -> VirglResult<()> {
        if buf.is_empty() {
            return Ok(());
        }

        if buf.len() % 4 != 0 {
            return Err(VirglError(-22));
        }

        let ndw = (buf.len() / 4) as c_int;
        let buf_ptr = buf.as_mut_ptr();

        let ret = unsafe { virgl_renderer_submit_cmd(buf_ptr as *mut c_void, ctx_id as c_int, ndw) };

        check(ret)
    }

    pub fn create_fence(&self, fence: VirglFence) -> VirglResult<()> {
        let ret = if let Some(ring_idx) = fence.ring_idx {
            unsafe { virgl_renderer_context_create_fence(fence.ctx_id, 0, ring_idx, fence.fence_id) }
        } else {
            // assume no-ring fence ids fit in 32-bit
            unsafe { virgl_renderer_create_fence(fence.fence_id as c_int, fence.ctx_id) }
        };

        check(ret)
    }

    pub fn resource_create_3d(&self, resource_id: u32, info: ResourceCreate3D) -> VirglResult<()> {
        let mut args = ResourceCreateArgs {
            handle: resource_id,
            target: info.target,
            format: info.format,
            bind: info.bind,
            width: info.width,
            height: info.height,
            depth: info.depth,
            array_size: info.array_size,
            last_level: info.last_level,
            nr_samples: info.nr_samples,
            flags: info.flags,
        };

        let ret = unsafe { virgl_renderer_resource_create(&mut args, std::ptr::null_mut(), 0) };
        check(ret)
    }

    pub fn resource_unref(&mut self, resource_id: u32) {
        unsafe { virgl_renderer_resource_unref(resource_id) };
        self.backing_iovs.remove(&resource_id);
    }

    pub fn resource_create_blob(
        &mut self,
        ctx_id: u32,
        resource_id: u32,
        blob: ResourceCreateBlob,
        iovecs: Option<Vec<Iovec>>,
    ) -> VirglResult<()> {
        let iovecs = iovecs.unwrap_or_default().into_boxed_slice();

        let (iov_ptr, num_iovs) = if iovecs.is_empty() {
            (std::ptr::null(), 0u32)
        } else {
            (iovecs.as_ptr(), iovecs.len() as u32)
        };

        let args = ResourceCreateBlobArgs {
            res_handle: resource_id,
            ctx_id,
            blob_mem: blob.blob_mem,
            blob_flags: blob.blob_flags,
            blob_id: blob.blob_id,
            size: blob.size,
            iovecs: iov_ptr,
            num_iovs,
        };

        let ret = unsafe { virgl_renderer_resource_create_blob(&args) };
        check(ret)?;

        if !iovecs.is_empty() {
            self.backing_iovs.insert(resource_id, iovecs);
        }

        Ok(())
    }

    pub fn attach_backing(&mut self, resource_id: u32, iovecs: Vec<Iovec>) -> VirglResult<()> {
        let mut iovecs = iovecs.into_boxed_slice();

        let (ptr, cnt) = if iovecs.is_empty() {
            (std::ptr::null_mut(), 0)
        } else {
            (iovecs.as_mut_ptr(), iovecs.len() as c_int)
        };

        let ret = unsafe { virgl_renderer_resource_attach_iov(resource_id as c_int, ptr, cnt) };
        check(ret)?;

        self.backing_iovs.insert(resource_id, iovecs);
        Ok(())
    }

    pub fn detach_backing(&mut self, resource_id: u32) {
        let mut iov = std::ptr::null_mut();
        let mut num_iovs = 0;

        unsafe {
            virgl_renderer_resource_detach_iov(resource_id as c_int, &mut iov, &mut num_iovs);
        }

        self.backing_iovs.remove(&resource_id);
    }

    pub fn transfer_write(&self, ctx_id: u32, resource_id: u32, t: Transfer3D) -> VirglResult<()> {
        let mut b = VirglBox {
            x: t.x,
            y: t.y,
            z: t.z,
            w: t.w,
            h: t.h,
            d: t.d,
        };

        let ret = unsafe {
            virgl_renderer_transfer_write_iov(
                resource_id,
                ctx_id,
                t.level as c_int,
                t.stride,
                t.layer_stride,
                &mut b,
                t.offset,
                std::ptr::null_mut(),
                0,
            )
        };
        check(ret)
    }

    pub fn native_texture(&self, resource_id: u32) -> Option<NativeTexture> {
        let mut info: VirglRendererResourceInfoExt = unsafe { std::mem::zeroed() };
        info.version = VIRGL_RENDERER_RESOURCE_INFO_EXT_VERSION;

        let ret = unsafe { virgl_renderer_resource_get_info_ext(resource_id as c_int, &mut info) };
        if ret != 0 || info.native_type != VIRGL_NATIVE_HANDLE_METAL_TEXTURE {
            return None;
        }

        Some(NativeTexture {
            handle: NonNull::new(unsafe { info.handle.native_handle })?,
            width: info.base.width,
            height: info.base.height,
        })
    }

    pub fn transfer_read(&self, ctx_id: u32, resource_id: u32, t: Transfer3D) -> VirglResult<()> {
        let mut b = VirglBox {
            x: t.x,
            y: t.y,
            z: t.z,
            w: t.w,
            h: t.h,
            d: t.d,
        };

        let ret = unsafe {
            virgl_renderer_transfer_read_iov(
                resource_id,
                ctx_id,
                t.level,
                t.stride,
                t.layer_stride,
                &mut b,
                t.offset,
                std::ptr::null_mut(),
                0,
            )
        };
        check(ret)
    }

    pub fn map_info(&self, resource_id: u32) -> VirglResult<u32> {
        let mut info = 0u32;
        let ret = unsafe { virgl_renderer_resource_get_map_info(resource_id, &mut info) };

        check(ret).map(|_| info)
    }

    pub fn map_ptr(&self, resource_id: u32) -> VirglResult<u64> {
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let mut size = 0u64;

        let ret = unsafe { virgl_renderer_resource_map(resource_id, &mut ptr, &mut size) };
        check(ret)?;

        if ptr.is_null() {
            return Err(VirglError(-14));
        }

        let _ = size;
        Ok(ptr as u64)
    }

    pub fn unmap(&self, resource_id: u32) -> VirglResult<()> {
        let ret = unsafe { virgl_renderer_resource_unmap(resource_id) };

        check(ret)
    }

    pub fn poll_ctxs(&self) {
        for &ctx_id in self.poller_contexts.iter() {
            unsafe { virgl_renderer_context_poll(ctx_id) };
        }
    }
}

impl Drop for VirglRenderer {
    fn drop(&mut self) {
        unsafe { virgl_renderer_cleanup(self.cookie) };
    }
}
