use objc2_core_foundation::{CFDictionary, CFNumber, CFNumberType, CFRetained, CFString};
use objc2_io_surface::{
    IOSurfaceRef as ObjcIOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceBytesPerRow, kIOSurfaceHeight,
    kIOSurfacePixelFormat, kIOSurfaceWidth,
};
use std::ffi::c_void;

pub type IOSurfaceRef = *mut c_void;

const IOSURFACE_PIXEL_FORMAT_BGRA: i32 = 0x4247_5241; // 'BGRA'

fn cf_i32(value: i32) -> Option<CFRetained<CFNumber>> {
    unsafe { CFNumber::new(None, CFNumberType::SInt32Type, &value as *const i32 as *const c_void) }
}

fn bgra_properties(width: u32, height: u32) -> Option<CFRetained<CFDictionary<CFString, CFNumber>>> {
    let width_i32 = i32::try_from(width).ok()?;
    let height_i32 = i32::try_from(height).ok()?;
    let bytes_per_row_i32 = i32::try_from(width.checked_mul(4)?).ok()?;

    let width = cf_i32(width_i32)?;
    let height = cf_i32(height_i32)?;
    let pixel_format = cf_i32(IOSURFACE_PIXEL_FORMAT_BGRA)?;
    let bytes_per_element = cf_i32(4)?;
    let bytes_per_row = cf_i32(bytes_per_row_i32)?;

    let keys: [&CFString; 5] = unsafe {
        [
            kIOSurfaceWidth,
            kIOSurfaceHeight,
            kIOSurfacePixelFormat,
            kIOSurfaceBytesPerElement,
            kIOSurfaceBytesPerRow,
        ]
    };

    let values: [&CFNumber; 5] = [
        width.as_ref(),
        height.as_ref(),
        pixel_format.as_ref(),
        bytes_per_element.as_ref(),
        bytes_per_row.as_ref(),
    ];

    Some(CFDictionary::from_slices(&keys, &values))
}

pub struct ScopedIOSurface {
    surface: CFRetained<ObjcIOSurfaceRef>,
    id: u32,
    width: u32,
    height: u32,
}

impl ScopedIOSurface {
    pub fn lookup(id: u32, width: u32, height: u32) -> Option<Self> {
        let surface = ObjcIOSurfaceRef::lookup(id)?;

        Some(Self {
            surface,
            width,
            height,
            id,
        })
    }

    pub fn new_bgra(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }

        let properties = bgra_properties(width, height)?;

        let surface = unsafe { ObjcIOSurfaceRef::new(properties.as_ref())? };
        let id = surface.id();

        Some(Self {
            surface,
            id,
            width,
            height,
        })
    }

    pub fn as_ptr(&self) -> IOSurfaceRef {
        self.as_objc_ref() as *const ObjcIOSurfaceRef as IOSurfaceRef
    }

    pub fn as_objc_ref(&self) -> &ObjcIOSurfaceRef {
        self.surface.as_ref()
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
