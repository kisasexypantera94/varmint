use zerocopy::FromBytes;

pub const ARM64_IMAGE_MAGIC: u32 = 0x644d5241;

/// https://docs.kernel.org/arch/arm64/booting.html#call-the-kernel-image
///
/// u32 code0;                    /* Executable code */
/// u32 code1;                    /* Executable code */
/// u64 text_offset;              /* Image load offset, little endian */
/// u64 image_size;               /* Effective Image size, little endian */
/// u64 flags;                    /* kernel flags, little endian */
/// u64 res2      = 0;            /* reserved */
/// u64 res3      = 0;            /* reserved */
/// u64 res4      = 0;            /* reserved */
/// u32 magic     = 0x644d5241;   /* Magic number, little endian, "ARM\x64" */
/// u32 res5;                     /* reserved (used for PE COFF offset) */

#[derive(Debug, FromBytes)]
#[repr(C, packed)]
pub struct ImageHeader {
    code0: u32,
    code1: u32,
    text_offset: u64,
    image_size: u64,
    flags: u64,
    res2: u64,
    res3: u64,
    res4: u64,
    magic: u32,
    res5: u32,
}

pub fn parse_image_header(image: &[u8]) -> Option<ImageHeader> {
    let (header, _rest) = ImageHeader::read_from_prefix(image).ok()?;
    let magic = header.magic;
    assert_eq!(magic, ARM64_IMAGE_MAGIC, "bad ARM64 Image magic");
    Some(header)
}
