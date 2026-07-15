use zerocopy::{Immutable, IntoBytes};

const BLOCK_SIZE: usize = 128;
const DESCRIPTOR_SIZE: usize = 18;

const MANUFACTURER: [u8; 3] = *b"VRM";
const PRODUCT_CODE: u16 = 1;
const SERIAL_NUMBER: u32 = 1;
const MANUFACTURE_WEEK: u8 = 1;
const MANUFACTURE_YEAR: u16 = 2026;
const MONITOR_NAME: &[u8] = b"Varmint";

const DIGITAL_INPUT: u8 = 1 << 7;
const GAMMA_2_2: u8 = 120;
const PREFERRED_TIMING_PRESENT: u8 = 1 << 1;

const MIN_VERTICAL_HZ: u8 = 48;
const MAX_VERTICAL_HZ: u8 = 120;
const MIN_HORIZONTAL_KHZ: u8 = 30;
const MAX_HORIZONTAL_KHZ: u8 = 255;
const MAX_PIXEL_CLOCK_MHZ: u16 = 600;

const HORIZONTAL_BLANKING: u16 = 160;
const HORIZONTAL_SYNC_OFFSET: u16 = 48;
const HORIZONTAL_SYNC_WIDTH: u16 = 32;
const VERTICAL_BLANKING: u16 = 35;
const VERTICAL_SYNC_OFFSET: u16 = 3;
const VERTICAL_SYNC_WIDTH: u16 = 6;

const MONITOR_NAME_TAG: u8 = 0xfc;
const RANGE_LIMITS_TAG: u8 = 0xfd;
const DIGITAL_SEPARATE_SYNC: u8 = 0x1e;
const DIGITAL_SEPARATE_SYNC_POSITIVE_VERTICAL: u8 = 0x1c;

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
struct Timing {
    horizontal_active: u16,
    vertical_active: u16,
    horizontal_blanking: u16,
    vertical_blanking: u16,
    horizontal_sync_offset: u16,
    horizontal_sync_width: u16,
    vertical_sync_offset: u16,
    vertical_sync_width: u16,
    pixel_clock_10khz: u16,
    flags: u8,
}

impl Timing {
    fn descriptor(self, mode: DisplayMode) -> [u8; DESCRIPTOR_SIZE] {
        let descriptor = DetailedTimingDescriptor {
            pixel_clock_10khz: self.pixel_clock_10khz.to_le_bytes(),
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
            horizontal_image_size_low: low_byte(mode.physical_width_mm()),
            vertical_image_size_low: low_byte(mode.physical_height_mm()),
            image_size_high: high_nibbles(mode.physical_width_mm(), mode.physical_height_mm()),
            horizontal_border: 0,
            vertical_border: 0,
            flags: self.flags,
        };

        copy_bytes(descriptor.as_bytes())
    }
}

impl DisplayMode {
    fn new(width: u32, height: u32, refresh_hz: u16) -> Option<Self> {
        if width == 0 || height == 0 || width > 0x0fff || height > 0x0fff {
            return None;
        }

        Some(Self {
            width: width as u16,
            height: height as u16,
            refresh_hz,
        })
    }

    fn with_refresh(self, refresh_hz: u16) -> Self {
        Self { refresh_hz, ..self }
    }

    fn physical_width_mm(self) -> u16 {
        (self.width / 5).clamp(1, 0x0fff)
    }

    fn physical_height_mm(self) -> u16 {
        (self.height / 5).clamp(1, 0x0fff)
    }

    fn fixed_timing(self) -> Option<Timing> {
        let horizontal_total = u64::from(self.width + HORIZONTAL_BLANKING);
        let vertical_total = u64::from(self.height + VERTICAL_BLANKING);
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
            pixel_clock_10khz: u16::try_from((pixels_per_second + 5_000) / 10_000).ok()?,
            flags: DIGITAL_SEPARATE_SYNC,
        })
    }

    fn cvt_timing(self) -> Option<Timing> {
        const HORIZONTAL_GRANULARITY: u32 = 8;
        const MIN_VERTICAL_PORCH: u32 = 3;
        const MIN_VSYNC_BACK_PORCH_US: u64 = 550;
        const CLOCK_STEP_KHZ: u64 = 250;

        let horizontal_active = u32::from(self.width) / HORIZONTAL_GRANULARITY * HORIZONTAL_GRANULARITY;
        let vertical_active = u32::from(self.height);
        let refresh_hz = u64::from(self.refresh_hz);
        let vertical_sync_width = u32::from(self.vertical_sync_width());

        let hperiod_numerator = 1_000_000_000u64.checked_sub(MIN_VSYNC_BACK_PORCH_US * 1_000 * refresh_hz)?;
        let hperiod_denominator = u64::from((vertical_active + MIN_VERTICAL_PORCH) * 2).checked_mul(refresh_hz)?;
        let horizontal_period = hperiod_numerator.checked_mul(2)? / hperiod_denominator;
        if horizontal_period == 0 {
            return None;
        }

        let vsync_back_porch = (MIN_VSYNC_BACK_PORCH_US * 1_000 / horizontal_period + 1)
            .max(u64::from(vertical_sync_width + MIN_VERTICAL_PORCH));
        let vertical_total = u64::from(vertical_active)
            .checked_add(vsync_back_porch)?
            .checked_add(u64::from(MIN_VERTICAL_PORCH))?;

        let blanking_percentage = (30_000u64.saturating_sub(300 * horizontal_period / 1_000)).max(20_000);
        let mut horizontal_blanking =
            u64::from(horizontal_active).checked_mul(blanking_percentage)? / (100_000 - blanking_percentage);
        horizontal_blanking -= horizontal_blanking % 16;

        let horizontal_total = u64::from(horizontal_active).checked_add(horizontal_blanking)?;
        let horizontal_sync_end = u64::from(horizontal_active).checked_add(horizontal_blanking / 2)?;
        let mut horizontal_sync_start = horizontal_sync_end.checked_sub(horizontal_total * 8 / 100)?;
        horizontal_sync_start += 8 - horizontal_sync_start % 8;

        let mut pixel_clock_khz = horizontal_total.checked_mul(1_000_000)? / horizontal_period;
        pixel_clock_khz -= pixel_clock_khz % CLOCK_STEP_KHZ;

        Some(Timing {
            horizontal_active: u16::try_from(horizontal_active).ok()?,
            vertical_active: self.height,
            horizontal_blanking: u16::try_from(horizontal_blanking).ok()?,
            vertical_blanking: u16::try_from(vertical_total.checked_sub(u64::from(vertical_active))?).ok()?,
            horizontal_sync_offset: u16::try_from(horizontal_sync_start.checked_sub(u64::from(horizontal_active))?)
                .ok()?,
            horizontal_sync_width: u16::try_from(horizontal_sync_end.checked_sub(horizontal_sync_start)?).ok()?,
            vertical_sync_offset: MIN_VERTICAL_PORCH as u16,
            vertical_sync_width: vertical_sync_width as u16,
            pixel_clock_10khz: u16::try_from(pixel_clock_khz / 10).ok()?,
            flags: DIGITAL_SEPARATE_SYNC_POSITIVE_VERTICAL,
        })
    }

    fn vertical_sync_width(self) -> u16 {
        let width = u32::from(self.width);
        let height = u32::from(self.height);

        if height % 3 == 0 && height * 4 / 3 == width {
            4
        } else if height % 9 == 0 && height * 16 / 9 == width {
            5
        } else if height % 10 == 0 && height * 16 / 10 == width {
            6
        } else if height % 4 == 0 && height * 5 / 4 == width {
            7
        } else if height % 9 == 0 && height * 15 / 9 == width {
            7
        } else {
            10
        }
    }

    fn detailed_timing(self) -> Option<[u8; DESCRIPTOR_SIZE]> {
        Some(self.fixed_timing()?.descriptor(self))
    }

    fn cvt_detailed_timing(self) -> Option<[u8; DESCRIPTOR_SIZE]> {
        Some(self.cvt_timing()?.descriptor(self))
    }
}

pub fn build(width: u32, height: u32, compatibility_mode: Option<(u32, u32)>) -> Option<[u8; BLOCK_SIZE]> {
    let preferred_mode = DisplayMode::new(width, height, 60)?;
    let preferred_timing = preferred_mode.cvt_detailed_timing()?;
    let high_refresh_timing = preferred_mode
        .with_refresh(120)
        .detailed_timing()
        .unwrap_or(preferred_timing);
    let compatibility_timing = compatibility_mode
        .filter(|&(compatibility_width, compatibility_height)| {
            compatibility_width != width || compatibility_height != height
        })
        .and_then(|(compatibility_width, compatibility_height)| {
            DisplayMode::new(compatibility_width, compatibility_height, 60)
        })
        .and_then(DisplayMode::cvt_detailed_timing);

    let descriptors = if let Some(compatibility_timing) = compatibility_timing {
        [
            preferred_timing,
            high_refresh_timing,
            compatibility_timing,
            text_descriptor(MONITOR_NAME_TAG, MONITOR_NAME),
        ]
    } else {
        [
            preferred_timing,
            high_refresh_timing,
            text_descriptor(MONITOR_NAME_TAG, MONITOR_NAME),
            range_limits_descriptor(),
        ]
    };

    let mut block = BaseBlock {
        header: [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00],
        manufacturer_id: manufacturer_id(MANUFACTURER),
        product_code: PRODUCT_CODE.to_le_bytes(),
        serial_number: SERIAL_NUMBER.to_le_bytes(),
        manufacture_week: MANUFACTURE_WEEK,
        manufacture_year_since_1990: (MANUFACTURE_YEAR - 1990) as u8,
        version: 1,
        revision: 4,
        video_input: DIGITAL_INPUT,
        horizontal_size_cm: millimeters_to_centimeters(preferred_mode.physical_width_mm()),
        vertical_size_cm: millimeters_to_centimeters(preferred_mode.physical_height_mm()),
        gamma: GAMMA_2_2,
        features: PREFERRED_TIMING_PRESENT,
        chromaticity: [0; 10],
        established_timings: [0; 3],
        standard_timings: [0x01; 16],
        descriptors,
        extension_count: 0,
        checksum: 0,
    };

    block.checksum = checksum(block.as_bytes());
    Some(copy_bytes(block.as_bytes()))
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
    descriptor[9] = (MAX_PIXEL_CLOCK_MHZ / 10) as u8;
    descriptor
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
