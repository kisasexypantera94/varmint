//! https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-4040006
use crate::virtio::{common, device::Device, virtq};
use applevisor::{error::Result, memory::Memory};
use num_enum::TryFromPrimitive;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes, Ref};

const DEVICE_ID: u32 = 16;

pub const MAX_SCANOUTS: usize = 16;

pub const WIDTH: usize = 1024;
pub const HEIGHT: usize = 768;
const FORMAT_B8G8R8X8_UNORM: u32 = 2;
const BYTES_PER_PIXEL: usize = 4;

pub const FLAG_FENCE: u32 = 1 << 0;
pub const FLAG_INFO_RING_IDX: u32 = 1 << 1;

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
}

pub struct DisplayBuffer {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
    pub dirty: bool,
}

impl DisplayBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height],
            dirty: true,
        }
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
    framebuffer: Vec<u8>,
    display: &'a Mutex<DisplayBuffer>,
}

impl<'a> Gpu<'a> {
    pub fn new(display: &'a Mutex<DisplayBuffer>) -> Gpu<'a> {
        Gpu {
            resources: HashMap::new(),
            scanout_resource: None,
            framebuffer: Vec::new(),
            display,
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
            num_scanouts: 1,
            ..Default::default()
        };

        let offset = offset as usize;
        data.copy_from_slice(&cfg.as_bytes()[offset..offset + data.len()]);
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
                                width: 1024,
                                height: 768,
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
                        }

                        Gpu::write_ok_nodata(&chain_data, mem)
                    }
                    Ok(CtrlType::TransferToHost2d) => {
                        let (val, _) = TransferToHost2d::read_from_prefix(payload).unwrap();

                        let resource_id = val.resource_id;
                        let resource = self.resources.get(&resource_id)?;

                        let bytes_per_pixel = 4;

                        let fb_len =
                            resource.width as usize * resource.height as usize * bytes_per_pixel;

                        if self.framebuffer.len() != fb_len {
                            self.framebuffer.resize(fb_len, 0);
                        }

                        let mut dst_off = 0usize;

                        for entry in &resource.backing {
                            if dst_off >= fb_len {
                                break;
                            }

                            let entry_len = entry.length as usize;
                            let copy_len = entry_len.min(fb_len - dst_off); // backing might be bigger than actual fb

                            mem.read(
                                entry.addr,
                                &mut self.framebuffer[dst_off..dst_off + copy_len],
                            )
                            .ok()?;

                            dst_off += copy_len;
                        }

                        Gpu::write_ok_nodata(&chain_data, mem)
                    }
                    Ok(CtrlType::ResourceFlush) => {
                        let (val, _) = ResourceFlush::read_from_prefix(payload).unwrap();

                        if self.scanout_resource == Some(val.resource_id) {
                            self.publish_display();
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

    fn handle_external(
        &mut self,
        _queues: &[super::virtq::Queue],
        _data: &[u8],
        _mem: &mut applevisor::prelude::Memory,
    ) -> Option<super::virtq::Completion> {
        None
    }

    fn pop_external(&mut self) -> Option<Vec<u8>> {
        None
    }
}

impl<'a> Gpu<'a> {
    fn publish_display(&mut self) {
        let mut display = self.display.lock().unwrap();

        let pixels = display.width * display.height;
        if self.framebuffer.len() < pixels * BYTES_PER_PIXEL {
            return;
        }

        if display.pixels.len() != pixels {
            display.pixels.resize(pixels, 0);
        }

        for i in 0..pixels {
            let j = i * BYTES_PER_PIXEL;

            let b = self.framebuffer[j] as u32;
            let g = self.framebuffer[j + 1] as u32;
            let r = self.framebuffer[j + 2] as u32;

            display.pixels[i] = (r << 16) | (g << 8) | b;
        }

        display.dirty = true;
    }

    fn write_ok_nodata(chain_data: &virtq::ChainData, mem: &mut Memory) -> u32 {
        Gpu::write_response(chain_data, CtrlType::RespOkNoData, mem)
    }

    fn write_response(chain_data: &virtq::ChainData, r#type: CtrlType, mem: &mut Memory) -> u32 {
        let mut resp = CtrlHeader::new_zeroed();
        resp.r#type = r#type as u32;
        chain_data.write_response(resp.as_bytes(), mem)
    }
}
