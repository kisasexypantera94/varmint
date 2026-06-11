//! https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-4040006
use crate::virtio::{MmioTransport, common, device::Device, virtq};
use applevisor::{error::Result, memory::Memory};
use num_enum::TryFromPrimitive;
use std::{collections::HashMap, mem::offset_of, sync::Mutex};
use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes, Ref};

const DEVICE_ID: u32 = 16;

pub const MAX_SCANOUTS: usize = 16;

const BYTES_PER_PIXEL: usize = 4;

const EVENT_DISPLAY: u32 = 1;

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromPrimitive)]
#[repr(u32)]
enum CtrlType {
    // 2d commands
    GetDisplayInfo = 0x0100,
    ResourceCreate2d,
    ResourceUnref,
    SetScanout,
    ResourceFlush,
    TransferToHost2d,
    ResourceAttachBacking,
    ResourceDetachBacking,
    GetCapsetInfo,
    GetCapset,
    GetEdid,
    ResourceAssignUuid,
    ResourceCreateBlob,
    SetScanoutBlob,

    // cursor commands
    UpdateCursor = 0x0300,
    MoveCursor,

    // ok responses
    RespOkNoData = 0x1100,
    RespOkDisplayInfo,
    RespOkCapsetInfo,
    RespOkCapset,
    RespOkEdid,
    RespOkResourceUuid,
    RespOkMapInfo,

    // error responses
    RespErrUnspec = 0x1200,
    RespErrOutOfMemory,
    RespErrInvalidScanoutId,
    RespErrInvalidResourceId,
    RespErrInvalidContextId,
    RespErrInvalidParameter,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct DisplayOne {
    r: Rect,
    enabled: u32,
    flags: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct RespDisplayInfo {
    hdr: CtrlHeader,
    pmodes: [DisplayOne; MAX_SCANOUTS],
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct ResourceCreate2d {
    resource_id: u32,
    format: u32,
    width: u32,
    height: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct ResourceUnref {
    resource_id: u32,
    padding: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct SetScanout {
    r: Rect,
    scanout_id: u32,
    resource_id: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct ResourceFlush {
    r: Rect,
    resource_id: u32,
    padding: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct TransferToHost2d {
    r: Rect,
    offset: u64,
    resource_id: u32,
    padding: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct ResourceAttachBacking {
    resource_id: u32,
    nr_entries: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct MemEntry {
    addr: u64,
    length: u32,
    padding: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct ResourceDetachBacking {
    resource_id: u32,
    padding: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct CursorPos {
    scanout_id: u32,
    x: u32,
    y: u32,
    padding: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct UpdateCursor {
    pos: CursorPos,
    resource_id: u32,
    hot_x: u32,
    hot_y: u32,
    padding: u32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromPrimitive)]
#[repr(usize)]
enum QueueType {
    Control = 0,
    Cursor = 1,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct CtrlHeader {
    r#type: u32,
    flags: u32,
    fence_id: u64,
    ctx_ud: u32,
    ring_idx: u8,
    padding: [u8; 3],
}

impl CtrlHeader {
    fn new(addr: u64, mem: &mut Memory) -> Result<CtrlHeader> {
        let mut h = CtrlHeader {
            r#type: mem.read_u32(addr)?,
            flags: mem.read_u32(addr + 4)?,
            fence_id: mem.read_u64(addr + 8)?,
            ctx_ud: mem.read_u32(addr + 16)?,
            ring_idx: mem.read_u8(addr + 20)?,
            padding: [0; 3],
        };

        mem.read(addr + 21, &mut h.padding)?;

        Ok(h)
    }
}

struct Resource {
    format: u32,
    width: u32,
    height: u32,
    backing: Vec<MemEntry>,
    framebuffer: Vec<u8>,
}

pub struct DisplayBuffer {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
    pub dirty: bool,
    pub seq: u64,
}

impl DisplayBuffer {
    pub fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            pixels: Vec::new(),
            seq: 0,
            dirty: true,
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.pixels.resize(width * height, 0);
        self.dirty = true;
        self.seq = self.seq.wrapping_add(1);
    }
}

#[repr(C)]
#[derive(Default, zerocopy::IntoBytes, zerocopy::Immutable)]
struct Config {
    events_read: u32,
    events_clear: u32,
    num_scanouts: u32,
    num_capsets: u32,
}

pub struct Gpu<'a> {
    resources: HashMap<u32, Resource>,
    scanout_resource: Option<u32>,
    display: &'a Mutex<DisplayBuffer>,

    scanout_width: u32,
    scanout_height: u32,

    events_read: u32,
}

impl<'a> Gpu<'a> {
    pub fn new(display: &'a Mutex<DisplayBuffer>) -> Gpu<'a> {
        let (width, height) = {
            let display = display.lock().unwrap();
            (display.width, display.height)
        };

        Gpu {
            resources: HashMap::new(),
            scanout_resource: None,
            display,
            scanout_width: width as u32,
            scanout_height: height as u32,
            events_read: 0,
        }
    }
}

impl<'a> Device for Gpu<'a> {
    fn id(&self) -> u32 {
        DEVICE_ID
    }

    fn features(&self) -> u64 {
        common::feature::VERSION_1
    }

    fn read_config(&self, offset: u64, data: &mut [u8]) {
        let cfg = Config {
            events_read: self.events_read,
            num_scanouts: 1,
            ..Default::default()
        };

        let offset = offset as usize;
        data.copy_from_slice(&cfg.as_bytes()[offset..offset + data.len()]);
    }

    fn write_config(&mut self, offset: u64, data: &[u8]) {
        if offset as usize == offset_of!(Config, events_clear) {
            let events_clear = u32::from_le_bytes(data.try_into().unwrap());
            self.events_read &= !events_clear;
        }
    }

    fn num_queues(&self) -> u16 {
        2
    }

    fn process_chain(
        &mut self,
        queue_idx: usize,
        queue: &super::virtq::Queue,
        head_idx: u16,
        mem: &mut applevisor::prelude::Memory,
    ) -> Option<u32> {
        let queue_type = QueueType::try_from(queue_idx).unwrap();
        match queue_type {
            QueueType::Control => {
                let chain_data = queue.collect_chain(head_idx, mem)?;

                let (hdr, payload) = CtrlHeader::read_from_prefix(&chain_data.readable).ok()?;

                Some(match CtrlType::try_from(hdr.r#type) {
                    Ok(CtrlType::GetDisplayInfo) => {
                        let mut resp = RespDisplayInfo::new_zeroed();

                        resp.hdr.r#type = CtrlType::RespOkDisplayInfo as u32;

                        resp.pmodes[0] = DisplayOne {
                            r: Rect {
                                x: 0,
                                y: 0,
                                width: self.scanout_width,
                                height: self.scanout_height,
                            },
                            enabled: 1,
                            flags: 0,
                        };

                        chain_data.write_response(resp.as_bytes(), mem)
                    }
                    Ok(CtrlType::ResourceCreate2d) => {
                        let (val, _) = ResourceCreate2d::read_from_prefix(payload).unwrap();

                        if val.format != 2 {
                            let format = val.format;
                            eprintln!("unsupported gpu format: {}", format);
                            return Some(Gpu::write_response(
                                &chain_data,
                                CtrlType::RespErrInvalidParameter,
                                mem,
                            ));
                        }

                        self.resources.insert(
                            val.resource_id,
                            Resource {
                                format: val.format,
                                width: val.width,
                                height: val.height,
                                backing: Vec::new(),
                                framebuffer: Vec::new(),
                            },
                        );

                        Gpu::write_ok_nodata(&chain_data, mem)
                    }
                    Ok(CtrlType::ResourceUnref) => {
                        let (val, _) = ResourceUnref::read_from_prefix(payload).unwrap();

                        let resource_id = val.resource_id;
                        self.resources.remove(&resource_id);

                        if self.scanout_resource == Some(resource_id) {
                            self.scanout_resource = None;
                        }

                        Gpu::write_ok_nodata(&chain_data, mem)
                    }
                    Ok(CtrlType::ResourceAttachBacking) => {
                        let (val, entries_raw) =
                            ResourceAttachBacking::read_from_prefix(payload).unwrap();

                        let n_entries = val.nr_entries as usize;
                        let entries_len = n_entries * core::mem::size_of::<MemEntry>();

                        let Some(entries_raw) = entries_raw.get(..entries_len) else {
                            return Some(Gpu::write_response(
                                &chain_data,
                                CtrlType::RespErrInvalidParameter,
                                mem,
                            ));
                        };

                        let entries = Ref::<_, [MemEntry]>::from_bytes(entries_raw).ok()?.to_vec();

                        let resource_id = val.resource_id;
                        self.resources.get_mut(&resource_id).unwrap().backing = entries;

                        Gpu::write_ok_nodata(&chain_data, mem)
                    }
                    Ok(CtrlType::ResourceDetachBacking) => {
                        let (val, _) = ResourceDetachBacking::read_from_prefix(payload).unwrap();

                        let resource_id = val.resource_id;

                        self.resources
                            .get_mut(&resource_id)
                            .unwrap()
                            .backing
                            .clear();

                        Gpu::write_ok_nodata(&chain_data, mem)
                    }
                    Ok(CtrlType::SetScanout) => {
                        let (val, _) = SetScanout::read_from_prefix(payload).unwrap();

                        if val.resource_id == 0 {
                            self.scanout_resource = None;
                        } else {
                            self.scanout_resource = Some(val.resource_id);

                            let resource_id = val.resource_id;
                            let resource = self.resources.get(&resource_id).unwrap();

                            self.display
                                .lock()
                                .unwrap()
                                .resize(resource.width as usize, resource.height as usize);
                        }

                        Gpu::write_ok_nodata(&chain_data, mem)
                    }
                    Ok(CtrlType::TransferToHost2d) => {
                        let (val, _) = TransferToHost2d::read_from_prefix(payload).unwrap();

                        let resource_id = val.resource_id;
                        let resource = self.resources.get_mut(&resource_id)?;

                        let width = resource.width as usize;
                        let height = resource.height as usize;
                        let stride = width * BYTES_PER_PIXEL;

                        let fb_len = height * stride;

                        if resource.framebuffer.len() != fb_len {
                            resource.framebuffer.resize(fb_len, 0);
                        }

                        let rect_x = val.r.x as usize;
                        let rect_y = val.r.y as usize;
                        let rect_width = val.r.width as usize;
                        let rect_height = val.r.height as usize;
                        let transfer_offset = val.offset as usize;

                        for row in 0..rect_height {
                            let src_offset = transfer_offset + row * stride;
                            let dst_offset = (rect_y + row) * stride + rect_x * BYTES_PER_PIXEL;
                            let row_len = rect_width * BYTES_PER_PIXEL;

                            Gpu::read_backing(
                                &resource.backing,
                                src_offset,
                                &mut resource.framebuffer[dst_offset..dst_offset + row_len],
                                mem,
                            )?;
                        }

                        Gpu::write_ok_nodata(&chain_data, mem)
                    }
                    Ok(CtrlType::ResourceFlush) => {
                        let (val, _) = ResourceFlush::read_from_prefix(payload).unwrap();

                        if self.scanout_resource == Some(val.resource_id) {
                            let resource_id = val.resource_id;
                            let resource = self.resources.get(&resource_id)?;
                            self.publish_display_rect(resource, val.r);
                        }

                        Gpu::write_ok_nodata(&chain_data, mem)
                    }
                    Ok(t) => {
                        eprintln!("unhandled: {:?}", t);

                        Gpu::write_response(&chain_data, CtrlType::RespErrUnspec, mem)
                    }
                    Err(v) => {
                        eprintln!("unknown type: 0x{:x}", v.number);
                        Gpu::write_response(&chain_data, CtrlType::RespErrUnspec, mem)
                    }
                })
            }
            QueueType::Cursor => {
                let chain_data = queue.collect_chain(head_idx, mem)?;

                Some(Gpu::write_ok_nodata(&chain_data, mem))
            }
        }
    }

    fn reset(&mut self) {
        self.resources.clear();
        self.scanout_resource = None;
    }
}

impl<'a> Gpu<'a> {
    fn publish_display_rect(&self, resource: &Resource, rect: Rect) {
        let mut display = self.display.lock().unwrap();

        let res_width = resource.width as usize;
        let res_height = resource.height as usize;

        let x0 = (rect.x as usize).min(res_width).min(display.width);
        let y0 = (rect.y as usize).min(res_height).min(display.height);
        let x1 = ((rect.x + rect.width) as usize)
            .min(res_width)
            .min(display.width);
        let y1 = ((rect.y + rect.height) as usize)
            .min(res_height)
            .min(display.height);

        if x0 >= x1 || y0 >= y1 {
            return;
        }

        let src_stride = res_width * BYTES_PER_PIXEL;

        for y in y0..y1 {
            let src_row = y * src_stride;
            let dst_row = y * display.width;

            for x in x0..x1 {
                let j = src_row + x * BYTES_PER_PIXEL;

                if j + 2 >= resource.framebuffer.len() {
                    return;
                }

                let b = resource.framebuffer[j] as u32;
                let g = resource.framebuffer[j + 1] as u32;
                let r = resource.framebuffer[j + 2] as u32;

                display.pixels[dst_row + x] = (r << 16) | (g << 8) | b;
            }
        }

        display.dirty = true;
        display.seq = display.seq.wrapping_add(1);
    }

    fn write_ok_nodata(chain_data: &virtq::ChainData, mem: &mut Memory) -> u32 {
        Gpu::write_response(chain_data, CtrlType::RespOkNoData, mem)
    }

    fn write_response(chain_data: &virtq::ChainData, r#type: CtrlType, mem: &mut Memory) -> u32 {
        let mut resp = CtrlHeader::new_zeroed();
        resp.r#type = r#type as u32;
        chain_data.write_response(resp.as_bytes(), mem)
    }

    fn read_backing(
        backing: &[MemEntry],
        mut src_offset: usize,
        dst: &mut [u8],
        mem: &mut Memory,
    ) -> Option<()> {
        let mut written = 0usize;

        for entry in backing {
            let entry_len = entry.length as usize;

            if src_offset >= entry_len {
                src_offset -= entry_len;
                continue;
            }

            let in_entry_off = src_offset;
            let available = entry_len - in_entry_off;
            let need = dst.len() - written;
            let copy_len = available.min(need);

            mem.read(
                entry.addr + in_entry_off as u64,
                &mut dst[written..written + copy_len],
            )
            .ok()?;

            written += copy_len;
            src_offset = 0;

            if written == dst.len() {
                return Some(());
            }
        }

        None
    }

    pub fn resize_display(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }

        self.scanout_width = width;
        self.scanout_height = height;
        self.events_read |= EVENT_DISPLAY;
    }
}

impl<'a> MmioTransport<Gpu<'a>> {
    pub fn resize_display(&mut self, width: u32, height: u32) {
        self.device_mut().resize_display(width, height);
        self.raise_config_interrupt();
    }
}
