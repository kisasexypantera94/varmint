use std::{ffi::c_void, ptr};

type CFIndex = isize;
type CFAllocatorRef = *const c_void;
type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFNumberRef = *const c_void;
type CFMutableDictionaryRef = *mut c_void;
type CFDictionaryRef = *const c_void;

pub type IOSurfaceRef = *mut c_void;

#[repr(C)]
struct CFDictionaryKeyCallBacks {
    _private: [usize; 6],
}

#[repr(C)]
struct CFDictionaryValueCallBacks {
    _private: [usize; 5],
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFTypeDictionaryKeyCallBacks: CFDictionaryKeyCallBacks;
    static kCFTypeDictionaryValueCallBacks: CFDictionaryValueCallBacks;

    fn CFDictionaryCreateMutable(
        allocator: CFAllocatorRef,
        capacity: CFIndex,
        key_callbacks: *const CFDictionaryKeyCallBacks,
        value_callbacks: *const CFDictionaryValueCallBacks,
    ) -> CFMutableDictionaryRef;

    fn CFDictionarySetValue(dict: CFMutableDictionaryRef, key: *const c_void, value: *const c_void);

    fn CFNumberCreate(
        allocator: CFAllocatorRef,
        the_type: i32,
        value_ptr: *const c_void,
    ) -> CFNumberRef;

    fn CFRetain(value: CFTypeRef) -> CFTypeRef;
    fn CFRelease(value: CFTypeRef);
}

#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
    static kIOSurfaceWidth: CFStringRef;
    static kIOSurfaceHeight: CFStringRef;
    static kIOSurfacePixelFormat: CFStringRef;
    static kIOSurfaceBytesPerElement: CFStringRef;
    static kIOSurfaceBytesPerRow: CFStringRef;

    fn IOSurfaceCreate(properties: CFDictionaryRef) -> IOSurfaceRef;
    fn IOSurfaceLookup(cs_id: u32) -> IOSurfaceRef;
    fn IOSurfaceGetID(surface: IOSurfaceRef) -> u32;
}

const K_CF_NUMBER_SINT32_TYPE: i32 = 3;
const IOSURFACE_PIXEL_FORMAT_BGRA: i32 = 0x4247_5241; // 'BGRA'

fn add_i32(dict: CFMutableDictionaryRef, key: CFStringRef, value: i32) -> bool {
    let number = unsafe {
        CFNumberCreate(
            ptr::null(),
            K_CF_NUMBER_SINT32_TYPE,
            &value as *const i32 as *const c_void,
        )
    };

    if number.is_null() {
        return false;
    }

    unsafe {
        CFDictionarySetValue(dict, key as *const c_void, number as *const c_void);
        CFRelease(number as CFTypeRef);
    }

    true
}

pub struct ScopedIOSurface {
    ptr: IOSurfaceRef,
    id: u32,
    width: u32,
    height: u32,
    owned: bool,
}

impl ScopedIOSurface {
    pub fn lookup(id: u32, width: u32, height: u32) -> Option<Self> {
        let ptr = unsafe { IOSurfaceLookup(id) };
        if ptr.is_null() {
            None
        } else {
            unsafe { CFRetain(ptr as CFTypeRef) };
            Some(Self {
                ptr,
                width,
                height,
                id,
                owned: true,
            })
        }
    }

    pub fn new_bgra(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }

        let dict = unsafe {
            CFDictionaryCreateMutable(
                ptr::null(),
                0,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            )
        };

        if dict.is_null() {
            return None;
        }

        let ok = add_i32(dict, unsafe { kIOSurfaceWidth }, width as i32)
            && add_i32(dict, unsafe { kIOSurfaceHeight }, height as i32)
            && add_i32(
                dict,
                unsafe { kIOSurfacePixelFormat },
                IOSURFACE_PIXEL_FORMAT_BGRA,
            )
            && add_i32(dict, unsafe { kIOSurfaceBytesPerElement }, 4)
            && add_i32(
                dict,
                unsafe { kIOSurfaceBytesPerRow },
                width.saturating_mul(4) as i32,
            );

        if !ok {
            unsafe {
                CFRelease(dict as CFTypeRef);
            }
            return None;
        }

        let ptr = unsafe { IOSurfaceCreate(dict as CFDictionaryRef) };

        unsafe {
            CFRelease(dict as CFTypeRef);
        }

        if ptr.is_null() {
            return None;
        }

        let id = unsafe { IOSurfaceGetID(ptr) };

        Some(Self {
            ptr,
            id,
            width,
            height,
            owned: true,
        })
    }

    pub fn as_ptr(&self) -> IOSurfaceRef {
        self.ptr
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

impl Drop for ScopedIOSurface {
    fn drop(&mut self) {
        if self.owned && !self.ptr.is_null() {
            unsafe {
                CFRelease(self.ptr as CFTypeRef);
            }
            self.ptr = ptr::null_mut();
        }
    }
}
