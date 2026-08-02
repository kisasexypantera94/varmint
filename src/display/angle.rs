use super::iosurface::IOSurfaceRef;
use crate::virtio::virgl_ffi::NativeTexture;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr,
};

type EGLBoolean = i32;
type EGLint = i32;
type EGLenum = u32;
type EGLDisplay = *mut c_void;
type EGLSurface = *mut c_void;
type EGLContext = *mut c_void;
type EGLConfig = *mut c_void;
type EGLClientBuffer = *mut c_void;
type EGLImageKHR = *mut c_void;

const RTLD_NOW: i32 = 2;

const EGL_FALSE: EGLBoolean = 0;
const EGL_NONE: EGLint = 0x3038;
const EGL_EXTENSIONS: EGLint = 0x3055;
const EGL_WIDTH: EGLint = 0x3057;
const EGL_HEIGHT: EGLint = 0x3056;
const EGL_SURFACE_TYPE: EGLint = 0x3033;
const EGL_PBUFFER_BIT: EGLint = 0x0001;
const EGL_RENDERABLE_TYPE: EGLint = 0x3040;
const EGL_OPENGL_ES2_BIT: EGLint = 0x0004;
const EGL_RED_SIZE: EGLint = 0x3024;
const EGL_GREEN_SIZE: EGLint = 0x3023;
const EGL_BLUE_SIZE: EGLint = 0x3022;
const EGL_ALPHA_SIZE: EGLint = 0x3021;
const EGL_TEXTURE_TARGET: EGLint = 0x3081;
const EGL_TEXTURE_2D: EGLint = 0x305F;
const EGL_TEXTURE_FORMAT: EGLint = 0x3080;
const EGL_TEXTURE_RGBA: EGLint = 0x305E;
const EGL_BACK_BUFFER: EGLint = 0x3084;
const EGL_DRAW: EGLint = 0x3059;
const EGL_READ: EGLint = 0x305A;
const EGL_CONTEXT_CLIENT_VERSION: EGLint = 0x3098;
const EGL_OPENGL_ES_API: EGLenum = 0x30A0;

const EGL_PLATFORM_ANGLE_ANGLE: EGLenum = 0x3202;
const EGL_PLATFORM_ANGLE_TYPE_ANGLE: EGLint = 0x3203;
const EGL_PLATFORM_ANGLE_TYPE_METAL_ANGLE: EGLint = 0x3489;

const EGL_IOSURFACE_ANGLE: EGLenum = 0x3454;
const EGL_IOSURFACE_PLANE_ANGLE: EGLint = 0x345A;
const EGL_TEXTURE_TYPE_ANGLE: EGLint = 0x345C;
const EGL_TEXTURE_INTERNAL_FORMAT_ANGLE: EGLint = 0x345D;
const EGL_METAL_TEXTURE_ANGLE: EGLenum = 0x34A7;

const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
const GL_NEAREST: i32 = 0x2600;
const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
const GL_COLOR_BUFFER_BIT: u32 = 0x00004000;
const GL_BGRA_EXT: u32 = 0x80E1;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_VERTEX_SHADER: u32 = 0x8B31;
const GL_FRAGMENT_SHADER: u32 = 0x8B30;
const GL_COMPILE_STATUS: u32 = 0x8B81;
const GL_LINK_STATUS: u32 = 0x8B82;
const GL_TEXTURE0: u32 = 0x84C0;
const GL_FLOAT: u32 = 0x1406;
const GL_FALSE_U8: u8 = 0;
const GL_TRIANGLE_STRIP: u32 = 0x0005;

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

type EglGetProcAddress = unsafe extern "C" fn(*const c_char) -> *const c_void;
type EglGetDisplay = unsafe extern "C" fn(*mut c_void) -> EGLDisplay;
type EglGetPlatformDisplayExt = unsafe extern "C" fn(EGLenum, *mut c_void, *const EGLint) -> EGLDisplay;
type EglInitialize = unsafe extern "C" fn(EGLDisplay, *mut EGLint, *mut EGLint) -> EGLBoolean;
type EglBindApi = unsafe extern "C" fn(EGLenum) -> EGLBoolean;
type EglChooseConfig =
    unsafe extern "C" fn(EGLDisplay, *const EGLint, *mut EGLConfig, EGLint, *mut EGLint) -> EGLBoolean;
type EglCreateContext = unsafe extern "C" fn(EGLDisplay, EGLConfig, EGLContext, *const EGLint) -> EGLContext;
type EglCreatePbufferSurface = unsafe extern "C" fn(EGLDisplay, EGLConfig, *const EGLint) -> EGLSurface;
type EglCreatePbufferFromClientBuffer =
    unsafe extern "C" fn(EGLDisplay, EGLenum, EGLClientBuffer, EGLConfig, *const EGLint) -> EGLSurface;
type EglMakeCurrent = unsafe extern "C" fn(EGLDisplay, EGLSurface, EGLSurface, EGLContext) -> EGLBoolean;
type EglGetCurrentDisplay = unsafe extern "C" fn() -> EGLDisplay;
type EglGetCurrentContext = unsafe extern "C" fn() -> EGLContext;
type EglGetCurrentSurface = unsafe extern "C" fn(EGLint) -> EGLSurface;
type EglBindTexImage = unsafe extern "C" fn(EGLDisplay, EGLSurface, EGLint) -> EGLBoolean;
type EglReleaseTexImage = unsafe extern "C" fn(EGLDisplay, EGLSurface, EGLint) -> EGLBoolean;
type EglDestroySurface = unsafe extern "C" fn(EGLDisplay, EGLSurface) -> EGLBoolean;
type EglDestroyContext = unsafe extern "C" fn(EGLDisplay, EGLContext) -> EGLBoolean;
type EglQueryString = unsafe extern "C" fn(EGLDisplay, EGLint) -> *const c_char;
type EglGetError = unsafe extern "C" fn() -> EGLint;
type EglCreateImageKHR =
    unsafe extern "C" fn(EGLDisplay, EGLContext, EGLenum, EGLClientBuffer, *const EGLint) -> EGLImageKHR;
type EglDestroyImageKHR = unsafe extern "C" fn(EGLDisplay, EGLImageKHR) -> EGLBoolean;

type GlGenTextures = unsafe extern "C" fn(i32, *mut u32);
type GlBindTexture = unsafe extern "C" fn(u32, u32);
type GlTexParameteri = unsafe extern "C" fn(u32, u32, i32);
type GlGenFramebuffers = unsafe extern "C" fn(i32, *mut u32);
type GlBindFramebuffer = unsafe extern "C" fn(u32, u32);
type GlFramebufferTexture2D = unsafe extern "C" fn(u32, u32, u32, u32, i32);
type GlCheckFramebufferStatus = unsafe extern "C" fn(u32) -> u32;
type GlClearColor = unsafe extern "C" fn(f32, f32, f32, f32);
type GlClear = unsafe extern "C" fn(u32);
type GlFinish = unsafe extern "C" fn();
type GlFlush = unsafe extern "C" fn();
type GlDeleteTextures = unsafe extern "C" fn(i32, *const u32);
type GlDeleteFramebuffers = unsafe extern "C" fn(i32, *const u32);

type GlCreateShader = unsafe extern "C" fn(u32) -> u32;
type GlShaderSource = unsafe extern "C" fn(u32, i32, *const *const c_char, *const i32);
type GlCompileShader = unsafe extern "C" fn(u32);
type GlGetShaderiv = unsafe extern "C" fn(u32, u32, *mut i32);
type GlDeleteShader = unsafe extern "C" fn(u32);
type GlCreateProgram = unsafe extern "C" fn() -> u32;
type GlAttachShader = unsafe extern "C" fn(u32, u32);
type GlBindAttribLocation = unsafe extern "C" fn(u32, u32, *const c_char);
type GlLinkProgram = unsafe extern "C" fn(u32);
type GlGetProgramiv = unsafe extern "C" fn(u32, u32, *mut i32);
type GlDeleteProgram = unsafe extern "C" fn(u32);
type GlUseProgram = unsafe extern "C" fn(u32);
type GlGetUniformLocation = unsafe extern "C" fn(u32, *const c_char) -> i32;
type GlUniform1i = unsafe extern "C" fn(i32, i32);
type GlViewport = unsafe extern "C" fn(i32, i32, i32, i32);
type GlActiveTexture = unsafe extern "C" fn(u32);
type GlEnableVertexAttribArray = unsafe extern "C" fn(u32);
type GlVertexAttribPointer = unsafe extern "C" fn(u32, i32, u32, u8, i32, *const c_void);
type GlDrawArrays = unsafe extern "C" fn(u32, i32, i32);
type GlEGLImageTargetTexture2DOES = unsafe extern "C" fn(u32, EGLImageKHR);

fn open_library(candidates: &[&str]) -> Option<*mut c_void> {
    for name in candidates {
        let c_name = CString::new(*name).ok()?;
        let handle = unsafe { dlopen(c_name.as_ptr(), RTLD_NOW) };
        if !handle.is_null() {
            return Some(handle);
        }
    }
    None
}

unsafe fn symbol<T: Copy>(handle: *mut c_void, name: &str) -> Option<T> {
    let c_name = CString::new(name).ok()?;
    let ptr = unsafe { dlsym(handle, c_name.as_ptr()) };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute_copy(&ptr) })
    }
}

unsafe fn proc_symbol<T: Copy>(egl_get_proc_address: EglGetProcAddress, name: &str) -> Option<T> {
    let c_name = CString::new(name).ok()?;
    let ptr = unsafe { egl_get_proc_address(c_name.as_ptr()) };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute_copy(&ptr) })
    }
}

unsafe fn load_egl_display(egl_get_proc_address: EglGetProcAddress, egl_get_display: EglGetDisplay) -> EGLDisplay {
    let egl_get_platform_display_ext: Option<EglGetPlatformDisplayExt> =
        unsafe { proc_symbol(egl_get_proc_address, "eglGetPlatformDisplayEXT") };

    if let Some(get_platform_display) = egl_get_platform_display_ext {
        let attrs = [
            EGL_PLATFORM_ANGLE_TYPE_ANGLE,
            EGL_PLATFORM_ANGLE_TYPE_METAL_ANGLE,
            EGL_NONE,
        ];
        let dpy = unsafe { get_platform_display(EGL_PLATFORM_ANGLE_ANGLE, ptr::null_mut(), attrs.as_ptr()) };
        if !dpy.is_null() {
            return dpy;
        }
    }

    unsafe { egl_get_display(ptr::null_mut()) }
}

unsafe fn choose_config(
    display: EGLDisplay,
    egl_choose_config: EglChooseConfig,
    egl_get_error: EglGetError,
) -> Option<EGLConfig> {
    let config_attribs = [
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

    let mut config: EGLConfig = ptr::null_mut();
    let mut num_configs = 0;
    if unsafe { egl_choose_config(display, config_attribs.as_ptr(), &mut config, 1, &mut num_configs) } == EGL_FALSE
        || num_configs == 0
        || config.is_null()
    {
        eprintln!("angle_egl: eglChooseConfig failed err=0x{:x}", unsafe {
            egl_get_error()
        });
        None
    } else {
        Some(config)
    }
}

struct CachedMetalSource {
    handle: *mut c_void,
    width: u32,
    height: u32,
    image: EGLImageKHR,
    texture: u32,
}

struct CachedIosurfaceTarget {
    surface: IOSurfaceRef,
    width: u32,
    height: u32,
    pbuffer: EGLSurface,
    texture: u32,
    fbo: u32,
}

struct AngleCopySession {
    display: EGLDisplay,
    config: EGLConfig,
    context: EGLContext,
    dummy_surface: EGLSurface,

    egl_make_current: EglMakeCurrent,
    egl_get_current_display: EglGetCurrentDisplay,
    egl_get_current_context: EglGetCurrentContext,
    egl_get_current_surface: EglGetCurrentSurface,
    egl_create_pbuffer_from_client_buffer: EglCreatePbufferFromClientBuffer,
    egl_bind_tex_image: EglBindTexImage,
    egl_release_tex_image: EglReleaseTexImage,
    egl_destroy_surface: EglDestroySurface,
    egl_destroy_context: EglDestroyContext,
    egl_create_image_khr: EglCreateImageKHR,
    egl_destroy_image_khr: EglDestroyImageKHR,
    egl_get_error: EglGetError,

    gl_gen_textures: GlGenTextures,
    gl_bind_texture: GlBindTexture,
    gl_tex_parameteri: GlTexParameteri,
    gl_delete_textures: GlDeleteTextures,
    gl_gen_framebuffers: GlGenFramebuffers,
    gl_bind_framebuffer: GlBindFramebuffer,
    gl_framebuffer_texture_2d: GlFramebufferTexture2D,
    gl_check_framebuffer_status: GlCheckFramebufferStatus,
    gl_delete_framebuffers: GlDeleteFramebuffers,
    gl_finish: GlFinish,
    gl_flush: GlFlush,

    gl_use_program: GlUseProgram,
    gl_uniform1i: GlUniform1i,
    gl_viewport: GlViewport,
    gl_active_texture: GlActiveTexture,
    gl_enable_vertex_attrib_array: GlEnableVertexAttribArray,
    gl_vertex_attrib_pointer: GlVertexAttribPointer,
    gl_draw_arrays: GlDrawArrays,
    gl_egl_image_target_texture_2d_oes: GlEGLImageTargetTexture2DOES,
    gl_delete_program: GlDeleteProgram,

    program: u32,
    sampler_location: i32,

    source_cache: Option<CachedMetalSource>,
    target_cache: Option<CachedIosurfaceTarget>,
}

unsafe impl Send for AngleCopySession {}

impl AngleCopySession {
    fn new() -> Option<Self> {
        let egl_lib = open_library(&["@rpath/libEGL.dylib", "libEGL.dylib"])?;
        let gles_lib = open_library(&["@rpath/libGLESv2.dylib", "libGLESv2.dylib"])?;

        let egl_get_proc_address: EglGetProcAddress = unsafe { symbol(egl_lib, "eglGetProcAddress")? };
        let egl_get_display: EglGetDisplay = unsafe { symbol(egl_lib, "eglGetDisplay")? };
        let egl_initialize: EglInitialize = unsafe { symbol(egl_lib, "eglInitialize")? };
        let egl_bind_api: EglBindApi = unsafe { symbol(egl_lib, "eglBindAPI")? };
        let egl_choose_config: EglChooseConfig = unsafe { symbol(egl_lib, "eglChooseConfig")? };
        let egl_create_context: EglCreateContext = unsafe { symbol(egl_lib, "eglCreateContext")? };
        let egl_create_pbuffer_surface: EglCreatePbufferSurface =
            unsafe { symbol(egl_lib, "eglCreatePbufferSurface")? };
        let egl_make_current: EglMakeCurrent = unsafe { symbol(egl_lib, "eglMakeCurrent")? };
        let egl_get_current_display: EglGetCurrentDisplay = unsafe { symbol(egl_lib, "eglGetCurrentDisplay")? };
        let egl_get_current_context: EglGetCurrentContext = unsafe { symbol(egl_lib, "eglGetCurrentContext")? };
        let egl_get_current_surface: EglGetCurrentSurface = unsafe { symbol(egl_lib, "eglGetCurrentSurface")? };
        let egl_create_pbuffer_from_client_buffer: EglCreatePbufferFromClientBuffer =
            unsafe { symbol(egl_lib, "eglCreatePbufferFromClientBuffer")? };
        let egl_bind_tex_image: EglBindTexImage = unsafe { symbol(egl_lib, "eglBindTexImage")? };
        let egl_release_tex_image: EglReleaseTexImage = unsafe { symbol(egl_lib, "eglReleaseTexImage")? };
        let egl_destroy_surface: EglDestroySurface = unsafe { symbol(egl_lib, "eglDestroySurface")? };
        let egl_destroy_context: EglDestroyContext = unsafe { symbol(egl_lib, "eglDestroyContext")? };
        let egl_query_string: EglQueryString = unsafe { symbol(egl_lib, "eglQueryString")? };
        let egl_get_error: EglGetError = unsafe { symbol(egl_lib, "eglGetError")? };

        let Some(egl_create_image_khr): Option<EglCreateImageKHR> =
            (unsafe { proc_symbol(egl_get_proc_address, "eglCreateImageKHR") })
        else {
            eprintln!("angle_egl_copy: missing eglCreateImageKHR");
            return None;
        };

        let Some(egl_destroy_image_khr): Option<EglDestroyImageKHR> =
            (unsafe { proc_symbol(egl_get_proc_address, "eglDestroyImageKHR") })
        else {
            eprintln!("angle_egl_copy: missing eglDestroyImageKHR");
            return None;
        };

        let gl_gen_textures: GlGenTextures = unsafe { symbol(gles_lib, "glGenTextures")? };
        let gl_bind_texture: GlBindTexture = unsafe { symbol(gles_lib, "glBindTexture")? };
        let gl_tex_parameteri: GlTexParameteri = unsafe { symbol(gles_lib, "glTexParameteri")? };
        let gl_delete_textures: GlDeleteTextures = unsafe { symbol(gles_lib, "glDeleteTextures")? };
        let gl_gen_framebuffers: GlGenFramebuffers = unsafe { symbol(gles_lib, "glGenFramebuffers")? };
        let gl_bind_framebuffer: GlBindFramebuffer = unsafe { symbol(gles_lib, "glBindFramebuffer")? };
        let gl_framebuffer_texture_2d: GlFramebufferTexture2D = unsafe { symbol(gles_lib, "glFramebufferTexture2D")? };
        let gl_check_framebuffer_status: GlCheckFramebufferStatus =
            unsafe { symbol(gles_lib, "glCheckFramebufferStatus")? };
        let gl_delete_framebuffers: GlDeleteFramebuffers = unsafe { symbol(gles_lib, "glDeleteFramebuffers")? };
        let gl_finish: GlFinish = unsafe { symbol(gles_lib, "glFinish")? };
        let gl_flush: GlFlush = unsafe { symbol(gles_lib, "glFlush")? };

        let gl_create_shader: GlCreateShader = unsafe { symbol(gles_lib, "glCreateShader")? };
        let gl_shader_source: GlShaderSource = unsafe { symbol(gles_lib, "glShaderSource")? };
        let gl_compile_shader: GlCompileShader = unsafe { symbol(gles_lib, "glCompileShader")? };
        let gl_get_shaderiv: GlGetShaderiv = unsafe { symbol(gles_lib, "glGetShaderiv")? };
        let gl_delete_shader: GlDeleteShader = unsafe { symbol(gles_lib, "glDeleteShader")? };
        let gl_create_program: GlCreateProgram = unsafe { symbol(gles_lib, "glCreateProgram")? };
        let gl_attach_shader: GlAttachShader = unsafe { symbol(gles_lib, "glAttachShader")? };
        let gl_bind_attrib_location: GlBindAttribLocation = unsafe { symbol(gles_lib, "glBindAttribLocation")? };
        let gl_link_program: GlLinkProgram = unsafe { symbol(gles_lib, "glLinkProgram")? };
        let gl_get_programiv: GlGetProgramiv = unsafe { symbol(gles_lib, "glGetProgramiv")? };
        let gl_delete_program: GlDeleteProgram = unsafe { symbol(gles_lib, "glDeleteProgram")? };
        let gl_use_program: GlUseProgram = unsafe { symbol(gles_lib, "glUseProgram")? };
        let gl_get_uniform_location: GlGetUniformLocation = unsafe { symbol(gles_lib, "glGetUniformLocation")? };
        let gl_uniform1i: GlUniform1i = unsafe { symbol(gles_lib, "glUniform1i")? };
        let gl_viewport: GlViewport = unsafe { symbol(gles_lib, "glViewport")? };
        let gl_active_texture: GlActiveTexture = unsafe { symbol(gles_lib, "glActiveTexture")? };
        let gl_enable_vertex_attrib_array: GlEnableVertexAttribArray =
            unsafe { symbol(gles_lib, "glEnableVertexAttribArray")? };
        let gl_vertex_attrib_pointer: GlVertexAttribPointer = unsafe { symbol(gles_lib, "glVertexAttribPointer")? };
        let gl_draw_arrays: GlDrawArrays = unsafe { symbol(gles_lib, "glDrawArrays")? };

        let gl_egl_image_target_texture_2d_oes: GlEGLImageTargetTexture2DOES =
            if let Some(f) = unsafe { proc_symbol(egl_get_proc_address, "glEGLImageTargetTexture2DOES") } {
                f
            } else if let Some(f) = unsafe { symbol(gles_lib, "glEGLImageTargetTexture2DOES") } {
                f
            } else {
                eprintln!("angle_egl_copy: missing glEGLImageTargetTexture2DOES");
                return None;
            };

        let display = unsafe { load_egl_display(egl_get_proc_address, egl_get_display) };
        if display.is_null() {
            eprintln!("angle_egl_copy: failed to get EGLDisplay");
            return None;
        }

        let mut major = 0;
        let mut minor = 0;
        if unsafe { egl_initialize(display, &mut major, &mut minor) } == EGL_FALSE {
            eprintln!("angle_egl_copy: eglInitialize failed err=0x{:x}", unsafe {
                egl_get_error()
            });
            return None;
        }

        let ext_ptr = unsafe { egl_query_string(display, EGL_EXTENSIONS) };
        let extensions = if ext_ptr.is_null() {
            ""
        } else {
            unsafe { CStr::from_ptr(ext_ptr).to_str().unwrap_or("") }
        };

        if !extensions.contains("EGL_ANGLE_iosurface_client_buffer") {
            eprintln!("angle_egl_copy: missing EGL_ANGLE_iosurface_client_buffer");
            return None;
        }

        if !extensions.contains("EGL_ANGLE_metal_texture_client_buffer") {
            eprintln!("angle_egl_copy: missing EGL_ANGLE_metal_texture_client_buffer");
            return None;
        }

        if unsafe { egl_bind_api(EGL_OPENGL_ES_API) } == EGL_FALSE {
            eprintln!("angle_egl_copy: eglBindAPI failed err=0x{:x}", unsafe {
                egl_get_error()
            });
            return None;
        }

        let Some(config) = (unsafe { choose_config(display, egl_choose_config, egl_get_error) }) else {
            return None;
        };

        let context_attribs = [EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE];
        let context = unsafe { egl_create_context(display, config, ptr::null_mut(), context_attribs.as_ptr()) };
        if context.is_null() {
            eprintln!("angle_egl_copy: eglCreateContext failed err=0x{:x}", unsafe {
                egl_get_error()
            });
            return None;
        }

        let dummy_attribs = [EGL_WIDTH, 1, EGL_HEIGHT, 1, EGL_NONE];
        let dummy_surface = unsafe { egl_create_pbuffer_surface(display, config, dummy_attribs.as_ptr()) };
        if dummy_surface.is_null() {
            eprintln!(
                "angle_egl_copy: eglCreatePbufferSurface(dummy) failed err=0x{:x}",
                unsafe { egl_get_error() }
            );
            return None;
        }

        if unsafe { egl_make_current(display, dummy_surface, dummy_surface, context) } == EGL_FALSE {
            eprintln!("angle_egl_copy: eglMakeCurrent failed err=0x{:x}", unsafe {
                egl_get_error()
            });
            return None;
        }

        let Some(program) = (unsafe {
            compile_copy_program(
                gl_create_shader,
                gl_shader_source,
                gl_compile_shader,
                gl_get_shaderiv,
                gl_delete_shader,
                gl_create_program,
                gl_attach_shader,
                gl_bind_attrib_location,
                gl_link_program,
                gl_get_programiv,
                gl_delete_program,
            )
        }) else {
            eprintln!("angle_egl_copy: failed to compile/link copy shader program");
            return None;
        };

        let sampler_name = CString::new("u_tex").unwrap();
        let sampler_location = unsafe { gl_get_uniform_location(program, sampler_name.as_ptr()) };
        if sampler_location < 0 {
            eprintln!("angle_egl_copy: missing u_tex uniform");
            unsafe { gl_delete_program(program) };
            return None;
        }

        eprintln!("angle_egl_copy: initialized persistent ANGLE IOSurface copy session (EGL {major}.{minor})");

        Some(Self {
            display,
            config,
            context,
            dummy_surface,

            egl_make_current,
            egl_get_current_display,
            egl_get_current_context,
            egl_get_current_surface,
            egl_create_pbuffer_from_client_buffer,
            egl_bind_tex_image,
            egl_release_tex_image,
            egl_destroy_surface,
            egl_destroy_context,
            egl_create_image_khr,
            egl_destroy_image_khr,
            egl_get_error,

            gl_gen_textures,
            gl_bind_texture,
            gl_tex_parameteri,
            gl_delete_textures,
            gl_gen_framebuffers,
            gl_bind_framebuffer,
            gl_framebuffer_texture_2d,
            gl_check_framebuffer_status,
            gl_delete_framebuffers,
            gl_finish,
            gl_flush,

            gl_use_program,
            gl_uniform1i,
            gl_viewport,
            gl_active_texture,
            gl_enable_vertex_attrib_array,
            gl_vertex_attrib_pointer,
            gl_draw_arrays,
            gl_egl_image_target_texture_2d_oes,
            gl_delete_program,

            program,
            sampler_location,

            source_cache: None,
            target_cache: None,
        })
    }

    unsafe fn destroy_source_cache(&mut self) {
        if let Some(cache) = self.source_cache.take() {
            unsafe {
                (self.gl_delete_textures)(1, &cache.texture);
                (self.egl_destroy_image_khr)(self.display, cache.image);
            }
        }
    }

    unsafe fn destroy_target_cache(&mut self) {
        if let Some(cache) = self.target_cache.take() {
            unsafe {
                (self.gl_delete_framebuffers)(1, &cache.fbo);
                (self.gl_delete_textures)(1, &cache.texture);
                (self.egl_destroy_surface)(self.display, cache.pbuffer);
            }
        }
    }

    unsafe fn ensure_source_cache(&mut self, source_texture: *mut c_void, width: u32, height: u32) -> Option<u32> {
        if let Some(cache) = self.source_cache.as_ref() {
            if cache.handle == source_texture && cache.width == width && cache.height == height {
                return Some(cache.texture);
            }
        }

        unsafe {
            self.destroy_source_cache();
        }

        let image_attribs_bgra = [EGL_TEXTURE_INTERNAL_FORMAT_ANGLE, GL_BGRA_EXT as EGLint, EGL_NONE];
        let image_attribs_default = [EGL_NONE];

        let mut image = unsafe {
            (self.egl_create_image_khr)(
                self.display,
                ptr::null_mut(),
                EGL_METAL_TEXTURE_ANGLE,
                source_texture as EGLClientBuffer,
                image_attribs_bgra.as_ptr(),
            )
        };

        if image.is_null() {
            image = unsafe {
                (self.egl_create_image_khr)(
                    self.display,
                    ptr::null_mut(),
                    EGL_METAL_TEXTURE_ANGLE,
                    source_texture as EGLClientBuffer,
                    image_attribs_default.as_ptr(),
                )
            };
        }

        if image.is_null() {
            eprintln!(
                "angle_egl_copy: eglCreateImageKHR(MTLTexture) failed err=0x{:x}",
                unsafe { (self.egl_get_error)() }
            );
            return None;
        }

        let mut texture = 0;
        unsafe {
            (self.gl_gen_textures)(1, &mut texture);
            (self.gl_bind_texture)(GL_TEXTURE_2D, texture);
            (self.gl_tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
            (self.gl_tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
            (self.gl_egl_image_target_texture_2d_oes)(GL_TEXTURE_2D, image);
        }

        self.source_cache = Some(CachedMetalSource {
            handle: source_texture,
            width,
            height,
            image,
            texture,
        });

        Some(texture)
    }

    unsafe fn ensure_target_cache(
        &mut self,
        target_surface: IOSurfaceRef,
        width: u32,
        height: u32,
    ) -> Option<(u32, u32, EGLSurface)> {
        if let Some(cache) = self.target_cache.as_ref() {
            if cache.surface == target_surface && cache.width == width && cache.height == height {
                return Some((cache.texture, cache.fbo, cache.pbuffer));
            }
        }

        unsafe {
            self.destroy_target_cache();
        }

        let iosurface_attribs = [
            EGL_WIDTH,
            width as EGLint,
            EGL_HEIGHT,
            height as EGLint,
            EGL_IOSURFACE_PLANE_ANGLE,
            0,
            EGL_TEXTURE_TARGET,
            EGL_TEXTURE_2D,
            EGL_TEXTURE_INTERNAL_FORMAT_ANGLE,
            GL_BGRA_EXT as EGLint,
            EGL_TEXTURE_FORMAT,
            EGL_TEXTURE_RGBA,
            EGL_TEXTURE_TYPE_ANGLE,
            GL_UNSIGNED_BYTE as EGLint,
            EGL_NONE,
        ];

        let pbuffer = unsafe {
            (self.egl_create_pbuffer_from_client_buffer)(
                self.display,
                EGL_IOSURFACE_ANGLE,
                target_surface as EGLClientBuffer,
                self.config,
                iosurface_attribs.as_ptr(),
            )
        };

        if pbuffer.is_null() {
            eprintln!(
                "angle_egl_copy: eglCreatePbufferFromClientBuffer(IOSurface) failed err=0x{:x}",
                unsafe { (self.egl_get_error)() }
            );
            return None;
        }

        let mut texture = 0;
        unsafe {
            (self.gl_gen_textures)(1, &mut texture);
            (self.gl_bind_texture)(GL_TEXTURE_2D, texture);
            (self.gl_tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
            (self.gl_tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        }

        if unsafe { (self.egl_bind_tex_image)(self.display, pbuffer, EGL_BACK_BUFFER) } == EGL_FALSE {
            eprintln!(
                "angle_egl_copy: eglBindTexImage(IOSurface/cache) failed err=0x{:x}",
                unsafe { (self.egl_get_error)() }
            );
            unsafe {
                (self.gl_delete_textures)(1, &texture);
                (self.egl_destroy_surface)(self.display, pbuffer);
            }
            return None;
        }

        let mut fbo = 0;
        unsafe {
            (self.gl_gen_framebuffers)(1, &mut fbo);
            (self.gl_bind_framebuffer)(GL_FRAMEBUFFER, fbo);
            (self.gl_framebuffer_texture_2d)(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, texture, 0);
        }

        let status = unsafe { (self.gl_check_framebuffer_status)(GL_FRAMEBUFFER) };
        unsafe {
            (self.egl_release_tex_image)(self.display, pbuffer, EGL_BACK_BUFFER);
        }

        if status != GL_FRAMEBUFFER_COMPLETE {
            eprintln!("angle_egl_copy: target IOSurface FBO incomplete status=0x{status:x}");
            unsafe {
                (self.gl_delete_framebuffers)(1, &fbo);
                (self.gl_delete_textures)(1, &texture);
                (self.egl_destroy_surface)(self.display, pbuffer);
            }
            return None;
        }

        self.target_cache = Some(CachedIosurfaceTarget {
            surface: target_surface,
            width,
            height,
            pbuffer,
            texture,
            fbo,
        });

        Some((texture, fbo, pbuffer))
    }

    unsafe fn copy_metal_texture_to_iosurface(
        &mut self,
        source_texture: *mut c_void,
        target_surface: IOSurfaceRef,
        source_size: (u32, u32),
        source_rect: (u32, u32, u32, u32),
    ) -> bool {
        let (source_width, source_height) = source_size;
        let (source_x, source_y, width, height) = source_rect;

        if source_texture.is_null()
            || target_surface.is_null()
            || source_width == 0
            || source_height == 0
            || width == 0
            || height == 0
            || source_x.checked_add(width).is_none_or(|end| end > source_width)
            || source_y.checked_add(height).is_none_or(|end| end > source_height)
        {
            return false;
        }

        if unsafe { (self.egl_make_current)(self.display, self.dummy_surface, self.dummy_surface, self.context) }
            == EGL_FALSE
        {
            eprintln!("angle_egl_copy: eglMakeCurrent(copy) failed err=0x{:x}", unsafe {
                (self.egl_get_error)()
            });
            return false;
        }

        let Some(source_tex) = (unsafe { self.ensure_source_cache(source_texture, source_width, source_height) })
        else {
            return false;
        };

        let Some((target_tex, fbo, target_pbuffer)) =
            (unsafe { self.ensure_target_cache(target_surface, width, height) })
        else {
            return false;
        };

        unsafe {
            (self.gl_bind_texture)(GL_TEXTURE_2D, target_tex);
        }

        if unsafe { (self.egl_bind_tex_image)(self.display, target_pbuffer, EGL_BACK_BUFFER) } == EGL_FALSE {
            eprintln!(
                "angle_egl_copy: eglBindTexImage(IOSurface/frame) failed err=0x{:x}",
                unsafe { (self.egl_get_error)() }
            );
            return false;
        }

        let u0 = source_x as f32 / source_width as f32;
        let v0 = source_y as f32 / source_height as f32;
        let u1 = (source_x + width) as f32 / source_width as f32;
        let v1 = (source_y + height) as f32 / source_height as f32;

        // V-only flip. U is normal.
        let vertices: [f32; 16] = [
            -1.0, -1.0, u0, v0, 1.0, -1.0, u1, v0, -1.0, 1.0, u0, v1, 1.0, 1.0, u1, v1,
        ];

        unsafe {
            (self.gl_bind_framebuffer)(GL_FRAMEBUFFER, fbo);
            (self.gl_viewport)(0, 0, width as i32, height as i32);
            (self.gl_use_program)(self.program);
            (self.gl_active_texture)(GL_TEXTURE0);
            (self.gl_bind_texture)(GL_TEXTURE_2D, source_tex);
            (self.gl_uniform1i)(self.sampler_location, 0);

            (self.gl_enable_vertex_attrib_array)(0);
            (self.gl_enable_vertex_attrib_array)(1);
            (self.gl_vertex_attrib_pointer)(
                0,
                2,
                GL_FLOAT,
                GL_FALSE_U8,
                4 * std::mem::size_of::<f32>() as i32,
                vertices.as_ptr() as *const c_void,
            );
            (self.gl_vertex_attrib_pointer)(
                1,
                2,
                GL_FLOAT,
                GL_FALSE_U8,
                4 * std::mem::size_of::<f32>() as i32,
                vertices.as_ptr().add(2) as *const c_void,
            );

            (self.gl_draw_arrays)(GL_TRIANGLE_STRIP, 0, 4);

            (self.egl_release_tex_image)(self.display, target_pbuffer, EGL_BACK_BUFFER);

            (self.gl_flush)();
        }

        true
    }
}

impl Drop for AngleCopySession {
    fn drop(&mut self) {
        let old_display = unsafe { (self.egl_get_current_display)() };
        let old_draw = unsafe { (self.egl_get_current_surface)(EGL_DRAW) };
        let old_read = unsafe { (self.egl_get_current_surface)(EGL_READ) };
        let old_context = unsafe { (self.egl_get_current_context)() };

        let made_current = unsafe {
            (self.egl_make_current)(self.display, self.dummy_surface, self.dummy_surface, self.context) != EGL_FALSE
        };

        if made_current {
            unsafe {
                self.destroy_source_cache();
                self.destroy_target_cache();
                (self.gl_delete_program)(self.program);
                (self.egl_make_current)(self.display, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            }
        } else {
            eprintln!("angle_egl_copy: failed to make copy context current during teardown");
        }

        unsafe {
            (self.egl_destroy_surface)(self.display, self.dummy_surface);
            (self.egl_destroy_context)(self.display, self.context);
        }

        if !old_display.is_null() && old_context != self.context {
            let restore_ok = unsafe { (self.egl_make_current)(old_display, old_draw, old_read, old_context) };
            if restore_ok == EGL_FALSE {
                eprintln!("angle_egl_copy: failed to restore previous EGL context during teardown");
            }
        }
    }
}

unsafe fn compile_shader(
    gl_create_shader: GlCreateShader,
    gl_shader_source: GlShaderSource,
    gl_compile_shader: GlCompileShader,
    gl_get_shaderiv: GlGetShaderiv,
    gl_delete_shader: GlDeleteShader,
    shader_type: u32,
    source: &str,
) -> Option<u32> {
    let shader = unsafe { gl_create_shader(shader_type) };
    if shader == 0 {
        return None;
    }

    let c_source = CString::new(source).ok()?;
    let ptr = c_source.as_ptr();

    unsafe {
        gl_shader_source(shader, 1, &ptr, ptr::null());
        gl_compile_shader(shader);
    }

    let mut ok = 0;
    unsafe {
        gl_get_shaderiv(shader, GL_COMPILE_STATUS, &mut ok);
    }

    if ok == 0 {
        unsafe {
            gl_delete_shader(shader);
        }
        None
    } else {
        Some(shader)
    }
}

unsafe fn compile_copy_program(
    gl_create_shader: GlCreateShader,
    gl_shader_source: GlShaderSource,
    gl_compile_shader: GlCompileShader,
    gl_get_shaderiv: GlGetShaderiv,
    gl_delete_shader: GlDeleteShader,
    gl_create_program: GlCreateProgram,
    gl_attach_shader: GlAttachShader,
    gl_bind_attrib_location: GlBindAttribLocation,
    gl_link_program: GlLinkProgram,
    gl_get_programiv: GlGetProgramiv,
    gl_delete_program: GlDeleteProgram,
) -> Option<u32> {
    let vs = unsafe {
        compile_shader(
            gl_create_shader,
            gl_shader_source,
            gl_compile_shader,
            gl_get_shaderiv,
            gl_delete_shader,
            GL_VERTEX_SHADER,
            r#"attribute vec2 a_pos;
attribute vec2 a_uv;
varying vec2 v_uv;
void main() {
    gl_Position = vec4(a_pos, 0.0, 1.0);
    v_uv = a_uv;
}
"#,
        )?
    };

    let fs = unsafe {
        compile_shader(
            gl_create_shader,
            gl_shader_source,
            gl_compile_shader,
            gl_get_shaderiv,
            gl_delete_shader,
            GL_FRAGMENT_SHADER,
            r#"precision mediump float;
uniform sampler2D u_tex;
varying vec2 v_uv;
void main() {
    gl_FragColor = texture2D(u_tex, v_uv);
}
"#,
        )?
    };

    let program = unsafe { gl_create_program() };
    if program == 0 {
        unsafe {
            gl_delete_shader(vs);
            gl_delete_shader(fs);
        }
        return None;
    }

    let a_pos = CString::new("a_pos").unwrap();
    let a_uv = CString::new("a_uv").unwrap();

    unsafe {
        gl_attach_shader(program, vs);
        gl_attach_shader(program, fs);
        gl_bind_attrib_location(program, 0, a_pos.as_ptr());
        gl_bind_attrib_location(program, 1, a_uv.as_ptr());
        gl_link_program(program);
    }

    let mut ok = 0;
    unsafe {
        gl_get_programiv(program, GL_LINK_STATUS, &mut ok);
        gl_delete_shader(vs);
        gl_delete_shader(fs);
    }

    if ok == 0 {
        unsafe {
            gl_delete_program(program);
        }
        None
    } else {
        Some(program)
    }
}

pub struct AngleCopy {
    session: Option<AngleCopySession>,
}

impl AngleCopy {
    pub fn new() -> Self {
        let session = AngleCopySession::new();

        if session.is_none() {
            panic!("angle_egl_copy: failed to initialize copy session");
        }

        Self { session }
    }

    pub fn copy_native_texture_to_iosurface(
        &mut self,
        source_texture: NativeTexture,
        target_surface: IOSurfaceRef,
        source_rect: (u32, u32, u32, u32),
    ) -> bool {
        let Some(sess) = self.session.as_mut() else {
            return false;
        };

        let old_display = unsafe { (sess.egl_get_current_display)() };
        let old_draw = unsafe { (sess.egl_get_current_surface)(EGL_DRAW) };
        let old_read = unsafe { (sess.egl_get_current_surface)(EGL_READ) };
        let old_context = unsafe { (sess.egl_get_current_context)() };

        let ok = unsafe {
            sess.copy_metal_texture_to_iosurface(
                source_texture.as_ptr(),
                target_surface,
                source_texture.size(),
                source_rect,
            )
        };

        let restore_ok = unsafe {
            if old_display.is_null() {
                (sess.egl_make_current)(sess.display, ptr::null_mut(), ptr::null_mut(), ptr::null_mut())
            } else {
                (sess.egl_make_current)(old_display, old_draw, old_read, old_context)
            }
        };

        if restore_ok == EGL_FALSE {
            eprintln!("angle_egl_copy: failed to restore previous EGL context");
        }

        ok
    }
}
