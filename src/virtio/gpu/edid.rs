use zerocopy::{Immutable, IntoBytes};

const BLOCK_SIZE: usize = 128;
const EDID_SIZE: usize = BLOCK_SIZE * 2;
const DESCRIPTOR_SIZE: usize = 18;
const DISPLAYID_TIMING_SIZE: usize = 20;

const MANUFACTURER: [u8; 3] = *b"VRM";
const PRODUCT_CODE: u16 = 1;
const SERIAL_NUMBER: u32 = 1;
const MANUFACTURE_WEEK: u8 = 1;
const MANUFACTURE_YEAR: u16 = 2026;
const MONITOR_NAME: &[u8] = b"Varmint VM";

const DIGITAL_INPUT: u8 = 1 << 7;
const GAMMA_2_2: u8 = 120;
const PREFERRED_TIMING_PRESENT: u8 = 1 << 1;

const MIN_VERTICAL_HZ: u8 = 48;
const MAX_VERTICAL_HZ: u8 = 120;
const MIN_HORIZONTAL_KHZ: u8 = 30;
const MAX_HORIZONTAL_KHZ: u8 = 255;
const MAX_PIXEL_CLOCK_MHZ: u16 = 1200;

const HORIZONTAL_BLANKING: u16 = 160;
const HORIZONTAL_SYNC_OFFSET: u16 = 48;
const HORIZONTAL_SYNC_WIDTH: u16 = 32;
const VERTICAL_BLANKING: u16 = 35;
const VERTICAL_SYNC_OFFSET: u16 = 3;
const VERTICAL_SYNC_WIDTH: u16 = 6;

const MONITOR_NAME_TAG: u8 = 0xfc;
const RANGE_LIMITS_TAG: u8 = 0xfd;
const DIGITAL_SEPARATE_SYNC: u8 = 0x1e;

const DISPLAYID_EXTENSION_TAG: u8 = 0x70;
const DISPLAYID_VERSION: u8 = 0x13;
const DISPLAYID_PRODUCT_TYPE_STANDALONE: u8 = 0x01;
const DISPLAYID_DETAILED_TIMING_TYPE_I: u8 = 0x03;
const DISPLAYID_TIMING_PREFERRED: u8 = 1 << 7;
const DISPLAYID_TIMING_ASPECT_UNDEFINED: u8 = 0x08;

#[repr(C)]
#[derive(IntoBytes, Immutable)]
struct BaseBlock {
    header: [u8; 8],
    manufacturer_id: [u8; 2],
    product_code: [u8; 2],
    serial_number: [u8; 4],
    manufacture_week: u8,
    manufacture_year_since_1990: u8,
    version: u8,
    revision: u8,
    video_input: u8,
    horizontal_size_cm: u8,
    vertical_size_cm: u8,
    gamma: u8,
    features: u8,
    chromaticity: [u8; 10],
    established_timings: [u8; 3],
    standard_timings: [u8; 16],
    descriptors: [[u8; DESCRIPTOR_SIZE]; 4],
    extension_count: u8,
    checksum: u8,
}

#[repr(C)]
#[derive(IntoBytes, Immutable)]
struct DetailedTimingDescriptor {
    pixel_clock_10khz: [u8; 2],
    horizontal_active_low: u8,
    horizontal_blanking_low: u8,
    horizontal_active_blanking_high: u8,
    vertical_active_low: u8,
    vertical_blanking_low: u8,
    vertical_active_blanking_high: u8,
    horizontal_sync_offset_low: u8,
    horizontal_sync_width_low: u8,
    vertical_sync_offset_width_low: u8,
    sync_high_bits: u8,
    horizontal_image_size_low: u8,
    vertical_image_size_low: u8,
    image_size_high: u8,
    horizontal_border: u8,
    vertical_border: u8,
    flags: u8,
}

#[derive(Clone, Copy)]
struct DisplayMode {
    width: u16,
    height: u16,
    refresh_hz: u16,
}

#[derive(Clone, Copy)]
struct PhysicalSizeMm {
    width: u16,
    height: u16,
}

#[derive(Clone, Copy)]
struct Timing {
    horizontal_active: u16,
    vertical_active: u16,
    horizontal_blanking: u16,
    vertical_blanking: u16,
    horizontal_sync_offset: u16,
    horizontal_sync_width: u16,
    vertical_sync_offset: u16,
    vertical_sync_width: u16,
    pixel_clock_10khz: u32,
    flags: u8,
}

impl Timing {
    fn descriptor(self, physical_size: PhysicalSizeMm) -> Option<[u8; DESCRIPTOR_SIZE]> {
        let descriptor = DetailedTimingDescriptor {
            pixel_clock_10khz: u16::try_from(self.pixel_clock_10khz).ok()?.to_le_bytes(),
            horizontal_active_low: low_byte(self.horizontal_active),
            horizontal_blanking_low: low_byte(self.horizontal_blanking),
            horizontal_active_blanking_high: high_nibbles(self.horizontal_active, self.horizontal_blanking),
            vertical_active_low: low_byte(self.vertical_active),
            vertical_blanking_low: low_byte(self.vertical_blanking),
            vertical_active_blanking_high: high_nibbles(self.vertical_active, self.vertical_blanking),
            horizontal_sync_offset_low: low_byte(self.horizontal_sync_offset),
            horizontal_sync_width_low: low_byte(self.horizontal_sync_width),
            vertical_sync_offset_width_low: low_nibbles(self.vertical_sync_offset, self.vertical_sync_width),
            sync_high_bits: sync_high_bits(
                self.horizontal_sync_offset,
                self.horizontal_sync_width,
                self.vertical_sync_offset,
                self.vertical_sync_width,
            ),
            horizontal_image_size_low: low_byte(physical_size.width),
            vertical_image_size_low: low_byte(physical_size.height),
            image_size_high: high_nibbles(physical_size.width, physical_size.height),
            horizontal_border: 0,
            vertical_border: 0,
            flags: self.flags,
        };

        Some(copy_bytes(descriptor.as_bytes()))
    }

    fn displayid_descriptor(self, preferred: bool) -> Option<[u8; DISPLAYID_TIMING_SIZE]> {
        let mut descriptor = [0u8; DISPLAYID_TIMING_SIZE];
        write_u24(&mut descriptor[0..3], self.pixel_clock_10khz.checked_sub(1)?)?;
        descriptor[3] = if preferred { DISPLAYID_TIMING_PREFERRED } else { 0 } | DISPLAYID_TIMING_ASPECT_UNDEFINED;
        write_u16_minus_one(&mut descriptor[4..6], self.horizontal_active)?;
        write_u16_minus_one(&mut descriptor[6..8], self.horizontal_blanking)?;
        write_u16_minus_one(&mut descriptor[8..10], self.horizontal_sync_offset)?;
        write_u16_minus_one(&mut descriptor[10..12], self.horizontal_sync_width)?;
        write_u16_minus_one(&mut descriptor[12..14], self.vertical_active)?;
        write_u16_minus_one(&mut descriptor[14..16], self.vertical_blanking)?;
        write_u16_minus_one(&mut descriptor[16..18], self.vertical_sync_offset)?;
        write_u16_minus_one(&mut descriptor[18..20], self.vertical_sync_width)?;
        Some(descriptor)
    }
}

impl DisplayMode {
    fn new(width: u32, height: u32, refresh_hz: u16) -> Option<Self> {
        if width == 0 || height == 0 || width > u32::from(u16::MAX) || height > u32::from(u16::MAX) {
            return None;
        }

        Some(Self {
            width: width as u16,
            height: height as u16,
            refresh_hz,
        })
    }

    fn doubled(self) -> Option<Self> {
        Self::new(
            u32::from(self.width).checked_mul(2)?,
            u32::from(self.height).checked_mul(2)?,
            self.refresh_hz,
        )
    }

    fn with_refresh(self, refresh_hz: u16) -> Self {
        Self { refresh_hz, ..self }
    }

    fn physical_size(self) -> PhysicalSizeMm {
        PhysicalSizeMm {
            width: (self.width / 5).clamp(1, 0x0fff),
            height: (self.height / 5).clamp(1, 0x0fff),
        }
    }

    fn timing(self) -> Option<Timing> {
        let horizontal_total = u64::from(self.width).checked_add(u64::from(HORIZONTAL_BLANKING))?;
        let vertical_total = u64::from(self.height).checked_add(u64::from(VERTICAL_BLANKING))?;
        let pixels_per_second = horizontal_total
            .checked_mul(vertical_total)?
            .checked_mul(u64::from(self.refresh_hz))?;

        Some(Timing {
            horizontal_active: self.width,
            vertical_active: self.height,
            horizontal_blanking: HORIZONTAL_BLANKING,
            vertical_blanking: VERTICAL_BLANKING,
            horizontal_sync_offset: HORIZONTAL_SYNC_OFFSET,
            horizontal_sync_width: HORIZONTAL_SYNC_WIDTH,
            vertical_sync_offset: VERTICAL_SYNC_OFFSET,
            vertical_sync_width: VERTICAL_SYNC_WIDTH,
            pixel_clock_10khz: u32::try_from((pixels_per_second + 5_000) / 10_000).ok()?,
            flags: DIGITAL_SEPARATE_SYNC,
        })
    }
}

pub fn build(width: u32, height: u32, _compatibility_mode: Option<(u32, u32)>) -> Option<[u8; EDID_SIZE]> {
    let half_retina_120 = DisplayMode::new(width, height, 120)?;
    let half_retina_60 = half_retina_120.with_refresh(60);
    let retina_120 = half_retina_120.doubled()?;
    let retina_60 = retina_120.with_refresh(60);
    let physical_size = half_retina_120.physical_size();

    let preferred_timing = half_retina_120.timing()?;
    let descriptors = [
        preferred_timing.descriptor(physical_size)?,
        half_retina_60.timing()?.descriptor(physical_size)?,
        text_descriptor(MONITOR_NAME_TAG, MONITOR_NAME),
        range_limits_descriptor(),
    ];

    let mut base = BaseBlock {
        header: [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00],
        manufacturer_id: manufacturer_id(MANUFACTURER),
        product_code: PRODUCT_CODE.to_le_bytes(),
        serial_number: SERIAL_NUMBER.to_le_bytes(),
        manufacture_week: MANUFACTURE_WEEK,
        manufacture_year_since_1990: (MANUFACTURE_YEAR - 1990) as u8,
        version: 1,
        revision: 4,
        video_input: DIGITAL_INPUT,
        horizontal_size_cm: millimeters_to_centimeters(physical_size.width),
        vertical_size_cm: millimeters_to_centimeters(physical_size.height),
        gamma: GAMMA_2_2,
        features: PREFERRED_TIMING_PRESENT,
        chromaticity: [0; 10],
        established_timings: [0; 3],
        standard_timings: [0x01; 16],
        descriptors,
        extension_count: 1,
        checksum: 0,
    };
    base.checksum = checksum(base.as_bytes());

    // Half Retina modes are already advertised by the two base-block DTDs.
    // Keep the DisplayID extension for the Retina-only modes so DRM does not
    // expose duplicate resolutions and refresh rates.
    let extension = displayid_extension(&[(retina_120, false), (retina_60, false)])?;

    let mut edid = [0u8; EDID_SIZE];
    edid[..BLOCK_SIZE].copy_from_slice(base.as_bytes());
    edid[BLOCK_SIZE..].copy_from_slice(&extension);
    Some(edid)
}

fn displayid_extension(modes: &[(DisplayMode, bool)]) -> Option<[u8; BLOCK_SIZE]> {
    let payload_len = modes.len().checked_mul(DISPLAYID_TIMING_SIZE)?;
    let block_len = 3usize.checked_add(payload_len)?;
    let displayid_payload_len = 2usize.checked_add(block_len)?;
    if displayid_payload_len > BLOCK_SIZE - 5 || payload_len > usize::from(u8::MAX) {
        return None;
    }

    let mut extension = [0u8; BLOCK_SIZE];
    extension[0] = DISPLAYID_EXTENSION_TAG;
    extension[1] = DISPLAYID_VERSION;
    extension[2] = u8::try_from(displayid_payload_len).ok()?;
    extension[3] = DISPLAYID_PRODUCT_TYPE_STANDALONE;
    extension[4] = 0;
    extension[5] = DISPLAYID_DETAILED_TIMING_TYPE_I;
    // Type I detailed timing data block revision 1.
    extension[6] = 1;
    extension[7] = u8::try_from(payload_len).ok()?;

    let mut offset = 8;
    for &(mode, preferred) in modes {
        let descriptor = mode.timing()?.displayid_descriptor(preferred)?;
        extension[offset..offset + DISPLAYID_TIMING_SIZE].copy_from_slice(&descriptor);
        offset += DISPLAYID_TIMING_SIZE;
    }

    // DisplayID has its own checksum immediately after the payload. It covers
    // the DisplayID revision byte through the end of the payload, excluding
    // the outer EDID extension tag at byte 0.
    let displayid_checksum_offset = 5usize.checked_add(displayid_payload_len)?;
    extension[displayid_checksum_offset] = checksum(&extension[1..displayid_checksum_offset]);

    // The EDID extension block also has the usual checksum in its last byte.
    extension[127] = checksum(&extension[..127]);
    Some(extension)
}

fn manufacturer_id(name: [u8; 3]) -> [u8; 2] {
    let letter = |value: u8| u16::from(value - b'@');
    let encoded = (letter(name[0]) << 10) | (letter(name[1]) << 5) | letter(name[2]);
    encoded.to_be_bytes()
}

fn text_descriptor(tag: u8, text: &[u8]) -> [u8; DESCRIPTOR_SIZE] {
    let mut descriptor = [0u8; DESCRIPTOR_SIZE];
    descriptor[3] = tag;

    let payload = &mut descriptor[5..];
    payload.fill(b' ');
    let text_len = text.len().min(payload.len().saturating_sub(1));
    payload[..text_len].copy_from_slice(&text[..text_len]);
    payload[text_len] = b'\n';

    descriptor
}

fn range_limits_descriptor() -> [u8; DESCRIPTOR_SIZE] {
    let mut descriptor = [0u8; DESCRIPTOR_SIZE];
    descriptor[3] = RANGE_LIMITS_TAG;
    descriptor[5] = MIN_VERTICAL_HZ;
    descriptor[6] = MAX_VERTICAL_HZ;
    descriptor[7] = MIN_HORIZONTAL_KHZ;
    descriptor[8] = MAX_HORIZONTAL_KHZ;
    descriptor[9] = (MAX_PIXEL_CLOCK_MHZ / 10).min(u16::from(u8::MAX)) as u8;
    descriptor
}

fn write_u24(destination: &mut [u8], value: u32) -> Option<()> {
    if destination.len() != 3 || value > 0x00ff_ffff {
        return None;
    }
    destination.copy_from_slice(&value.to_le_bytes()[..3]);
    Some(())
}

fn write_u16_minus_one(destination: &mut [u8], value: u16) -> Option<()> {
    let encoded = value.checked_sub(1)?.to_le_bytes();
    destination.copy_from_slice(&encoded);
    Some(())
}

fn low_byte(value: u16) -> u8 {
    value as u8
}

fn high_nibbles(first: u16, second: u16) -> u8 {
    (((first >> 8) & 0x0f) << 4 | ((second >> 8) & 0x0f)) as u8
}

fn low_nibbles(first: u16, second: u16) -> u8 {
    (((first & 0x0f) << 4) | (second & 0x0f)) as u8
}

fn sync_high_bits(horizontal_offset: u16, horizontal_width: u16, vertical_offset: u16, vertical_width: u16) -> u8 {
    (((horizontal_offset >> 8) & 0x03) << 6
        | ((horizontal_width >> 8) & 0x03) << 4
        | ((vertical_offset >> 4) & 0x03) << 2
        | ((vertical_width >> 4) & 0x03)) as u8
}

fn millimeters_to_centimeters(value: u16) -> u8 {
    (value / 10).min(u16::from(u8::MAX)) as u8
}

fn checksum(bytes: &[u8]) -> u8 {
    0u8.wrapping_sub(bytes.iter().copied().fold(0u8, u8::wrapping_add))
}

fn copy_bytes<const N: usize>(source: &[u8]) -> [u8; N] {
    let mut bytes = [0u8; N];
    bytes.copy_from_slice(source);
    bytes
}
