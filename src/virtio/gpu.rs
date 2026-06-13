//! https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-4040006
use crate::virtio::{
    chain::ChainData,
    common,
    device::{ChainAction, ChainToken, Device, Effect, ExternalEventHandler},
};
use applevisor::memory::Memory;
use num_enum::TryFromPrimitive;
use std::{collections::HashMap, mem::offset_of, sync::Mutex};
use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes};

const DEVICE_ID: u32 = 16;

const MAX_SCANOUTS: usize = 16;

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

const FLAG_FENCE: u32 = 1 << 0;

const PAYLOAD_OFFSET: usize = size_of::<CtrlHeader>();

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
        chain: &ChainData,
        _token: ChainToken,
        mem: &mut Memory,
    ) -> ChainAction {
        let Some(hdr) = chain.read_obj::<CtrlHeader>(0, mem) else {
            eprintln!("virtio-gpu: unreadable command header");
            let written = Gpu::write_response(
                chain,
                CtrlType::RespErrUnspec,
                &CtrlHeader::new_zeroed(),
                mem,
            );
            return ChainAction::Complete(written);
        };

        let written = match QueueType::try_from(queue_idx).unwrap() {
            QueueType::Cursor => Gpu::write_response(chain, CtrlType::RespOkNoData, &hdr, mem),
            QueueType::Control => match CtrlType::try_from(hdr.r#type) {
                Ok(CtrlType::GetDisplayInfo) => {
                    let mut resp = RespDisplayInfo::new_zeroed();

                    resp.hdr = Gpu::resp_header(CtrlType::RespOkDisplayInfo, &hdr);
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

                    chain.write_response(resp.as_bytes(), mem)
                }
                Ok(cmd) => {
                    let resp = self.control(cmd, chain, mem);
                    Gpu::write_response(chain, resp, &hdr, mem)
                }
                Err(v) => {
                    eprintln!("virtio-gpu: unknown command: 0x{:x}", v.number);
                    Gpu::write_response(chain, CtrlType::RespErrUnspec, &hdr, mem)
                }
            },
        };

        ChainAction::Complete(written)
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
        if resource.framebuffer.len() < y1 * src_stride {
            return;
        }

        let dst_w = display.width;
        let dst = display.pixels.as_mut_slice();

        for y in y0..y1 {
            let s = y * src_stride;
            let src = &resource.framebuffer[s + x0 * BYTES_PER_PIXEL..s + x1 * BYTES_PER_PIXEL];
            let drow = &mut dst[y * dst_w + x0..y * dst_w + x1];
            drow.as_mut_bytes().copy_from_slice(src);
        }

        display.dirty = true;
        display.seq = display.seq.wrapping_add(1);
    }

    fn control(&mut self, cmd: CtrlType, chain: &ChainData, mem: &mut Memory) -> CtrlType {
        match cmd {
            CtrlType::ResourceCreate2d => {
                let Some(val) = chain.read_obj::<ResourceCreate2d>(PAYLOAD_OFFSET, mem) else {
                    return CtrlType::RespErrInvalidParameter;
                };

                if val.format != 2 {
                    eprintln!("virtio-gpu: unsupported format: {}", { val.format });
                    return CtrlType::RespErrInvalidParameter;
                }

                if val.width == 0 || val.height == 0 || val.width > 16384 || val.height > 16384 {
                    return CtrlType::RespErrInvalidParameter;
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

                CtrlType::RespOkNoData
            }

            CtrlType::ResourceUnref => {
                let Some(val) = chain.read_obj::<ResourceUnref>(PAYLOAD_OFFSET, mem) else {
                    return CtrlType::RespErrInvalidParameter;
                };

                let resource_id = val.resource_id;
                self.resources.remove(&resource_id);

                if self.scanout_resource == Some(resource_id) {
                    self.scanout_resource = None;
                }

                CtrlType::RespOkNoData
            }

            CtrlType::ResourceAttachBacking => {
                let Some(val) = chain.read_obj::<ResourceAttachBacking>(PAYLOAD_OFFSET, mem) else {
                    return CtrlType::RespErrInvalidParameter;
                };

                let n_entries = val.nr_entries as usize;
                let entries_base = PAYLOAD_OFFSET + size_of::<ResourceAttachBacking>();
                let entry_size = size_of::<MemEntry>();

                if entries_base + n_entries * entry_size > chain.readable_len() {
                    return CtrlType::RespErrInvalidParameter;
                }

                let resource_id = val.resource_id;
                let Some(resource) = self.resources.get_mut(&resource_id) else {
                    return CtrlType::RespErrInvalidResourceId;
                };

                let mut backing = Vec::with_capacity(n_entries);
                for i in 0..n_entries {
                    let Some(entry) =
                        chain.read_obj::<MemEntry>(entries_base + i * entry_size, mem)
                    else {
                        return CtrlType::RespErrInvalidParameter;
                    };
                    backing.push(entry);
                }

                resource.backing = backing;

                CtrlType::RespOkNoData
            }

            CtrlType::ResourceDetachBacking => {
                let Some(val) = chain.read_obj::<ResourceDetachBacking>(PAYLOAD_OFFSET, mem) else {
                    return CtrlType::RespErrInvalidParameter;
                };

                let resource_id = val.resource_id;
                let Some(resource) = self.resources.get_mut(&resource_id) else {
                    return CtrlType::RespErrInvalidResourceId;
                };

                resource.backing.clear();

                CtrlType::RespOkNoData
            }

            CtrlType::SetScanout => {
                let Some(val) = chain.read_obj::<SetScanout>(PAYLOAD_OFFSET, mem) else {
                    return CtrlType::RespErrInvalidParameter;
                };

                let resource_id = val.resource_id;

                if resource_id == 0 {
                    self.scanout_resource = None;
                } else {
                    let Some(resource) = self.resources.get(&resource_id) else {
                        return CtrlType::RespErrInvalidResourceId;
                    };

                    self.scanout_resource = Some(resource_id);
                    self.display
                        .lock()
                        .unwrap()
                        .resize(resource.width as usize, resource.height as usize);
                }

                CtrlType::RespOkNoData
            }

            CtrlType::TransferToHost2d => {
                let Some(val) = chain.read_obj::<TransferToHost2d>(PAYLOAD_OFFSET, mem) else {
                    return CtrlType::RespErrInvalidParameter;
                };

                let resource_id = val.resource_id;
                let Some(resource) = self.resources.get_mut(&resource_id) else {
                    return CtrlType::RespErrInvalidResourceId;
                };

                let width = resource.width as usize;
                let height = resource.height as usize;
                let stride = width * BYTES_PER_PIXEL;

                let rect_x = val.r.x as usize;
                let rect_y = val.r.y as usize;
                let rect_width = val.r.width as usize;
                let rect_height = val.r.height as usize;
                let transfer_offset = val.offset as usize;

                if rect_x + rect_width > width || rect_y + rect_height > height {
                    return CtrlType::RespErrInvalidParameter;
                }

                let fb_len = height * stride;
                if resource.framebuffer.len() != fb_len {
                    resource.framebuffer.resize(fb_len, 0);
                }

                let row_len = rect_width * BYTES_PER_PIXEL;

                let ok = if rect_x == 0 && rect_width == width {
                    let start = rect_y * stride;
                    Gpu::read_backing(
                        &resource.backing,
                        transfer_offset,
                        &mut resource.framebuffer[start..start + rect_height * stride],
                        mem,
                    )
                    .is_some()
                } else {
                    (0..rect_height).all(|row| {
                        let src_offset = transfer_offset + row * stride;
                        let dst_offset = (rect_y + row) * stride + rect_x * BYTES_PER_PIXEL;
                        Gpu::read_backing(
                            &resource.backing,
                            src_offset,
                            &mut resource.framebuffer[dst_offset..dst_offset + row_len],
                            mem,
                        )
                        .is_some()
                    })
                };

                if !ok {
                    return CtrlType::RespErrInvalidParameter;
                }

                CtrlType::RespOkNoData
            }

            CtrlType::ResourceFlush => {
                let Some(val) = chain.read_obj::<ResourceFlush>(PAYLOAD_OFFSET, mem) else {
                    return CtrlType::RespErrInvalidParameter;
                };

                let resource_id = val.resource_id;
                if self.scanout_resource == Some(resource_id) {
                    let Some(resource) = self.resources.get(&resource_id) else {
                        return CtrlType::RespErrInvalidResourceId;
                    };
                    self.publish_display_rect(resource, val.r);
                }

                CtrlType::RespOkNoData
            }

            cmd => {
                eprintln!("virtio-gpu: unhandled command: {:?}", cmd);
                CtrlType::RespErrUnspec
            }
        }
    }

    fn resp_header(r#type: CtrlType, req: &CtrlHeader) -> CtrlHeader {
        let mut resp = CtrlHeader::new_zeroed();
        resp.r#type = r#type as u32;
        if req.flags & FLAG_FENCE != 0 {
            resp.flags = FLAG_FENCE;
            resp.fence_id = req.fence_id;
        }
        resp
    }

    fn write_response(
        chain: &ChainData,
        r#type: CtrlType,
        req: &CtrlHeader,
        mem: &mut Memory,
    ) -> u32 {
        chain.write_response(Gpu::resp_header(r#type, req).as_bytes(), mem)
    }

    fn read_backing(
        backing: &[MemEntry],
        mut src_offset: usize,
        dst: &mut [u8],
        mem: &Memory,
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
}

pub enum ExternalEvent {
    DisplayResized { width: u32, height: u32 },
}

impl<'a> ExternalEventHandler for Gpu<'a> {
    type Event<'b> = ExternalEvent;

    fn on_event(&mut self, event: ExternalEvent, mut emit: impl FnMut(Effect)) {
        match event {
            ExternalEvent::DisplayResized { width, height } => {
                if width == 0 || height == 0 {
                    return;
                }

                self.scanout_width = width;
                self.scanout_height = height;
                self.events_read |= EVENT_DISPLAY;
                emit(Effect::Config);
            }
        }
    }
}
