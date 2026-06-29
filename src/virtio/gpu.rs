//! https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-4040006
use crate::{
    iosurface::ScopedIOSurface,
    virtio::{
        chain::ChainData,
        common,
        device::{ChainAction, ChainToken, Device, Effect, ExternalEventHandler, ShmRegion},
        virgl_ffi::{
            Iovec, ResourceCreate3D, ResourceCreateBlob as VirglResourceCreateBlob, Transfer3D, VirglFence,
            VirglRenderer,
        },
    },
};
use applevisor::memory::Memory;
use applevisor_sys::{hv_vm_map, hv_vm_unmap};
use num_enum::TryFromPrimitive;
use std::{
    collections::{HashMap, VecDeque},
    ffi::c_void,
    mem::offset_of,
    sync::Mutex,
};
use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes};

const DEVICE_ID: u32 = 16;

const MAX_SCANOUTS: usize = 16;

const BYTES_PER_PIXEL: usize = 4;

const EVENT_DISPLAY: u32 = 1;

const HOST_VISIBLE_SHM_ID: u64 = 1;
const HOST_VISIBLE_SHM_BASE: u64 = 0x8_0000_0000;
const HOST_VISIBLE_SHM_SIZE: u64 = 4 * 1024 * 1024 * 1024;
const APPLE_HV_PAGE_SIZE: usize = 0x1000;

const MAP_CACHE_MASK: u32 = 0x0f;

mod feature {
    pub const VIRGL: u64 = 1 << 0;
    pub const RESOURCE_BLOB: u64 = 1 << 3;
    pub const CONTEXT_INIT: u64 = 1 << 4;
}

macro_rules! pf {
    ($obj:expr, $field:ident) => {{
        let v = $obj.$field;
        v
    }};
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromPrimitive)]
#[repr(u32)]
enum CtrlType {
    // 2d commands
    GetDisplayInfo = 0x0100,
    ResourceCreate2d,      // 0x0101
    ResourceUnref,         // 0x0102
    SetScanout,            // 0x0103
    ResourceFlush,         // 0x0104
    TransferToHost2d,      // 0x0105
    ResourceAttachBacking, // 0x0106
    ResourceDetachBacking, // 0x0107
    GetCapsetInfo,         // 0x0108
    GetCapset,             // 0x0109
    GetEdid,               // 0x010a
    ResourceAssignUuid,    // 0x010b
    ResourceCreateBlob,    // 0x010c
    SetScanoutBlob,        // 0x010d

    // 3d commands (context + venus)
    CtxCreate = 0x0200,
    CtxDestroy,         // 0x0201
    CtxAttachResource,  // 0x0202
    CtxDetachResource,  // 0x0203
    ResourceCreate3d,   // 0x0204
    TransferToHost3d,   // 0x0205
    TransferFromHost3d, // 0x0206
    Submit3d,           // 0x0207
    ResourceMapBlob,    // 0x0208
    ResourceUnmapBlob,  // 0x0209

    // cursor commands
    UpdateCursor = 0x0300,
    MoveCursor, // 0x0301

    // ok responses
    RespOkNoData = 0x1100,
    RespOkDisplayInfo,  // 0x1101
    RespOkCapsetInfo,   // 0x1102
    RespOkCapset,       // 0x1103
    RespOkEdid,         // 0x1104
    RespOkResourceUuid, // 0x1105
    RespOkMapInfo,      // 0x1106

    // error responses
    RespErrUnspec = 0x1200,
    RespErrOutOfMemory,       // 0x1201
    RespErrInvalidScanoutId,  // 0x1202
    RespErrInvalidResourceId, // 0x1203
    RespErrInvalidContextId,  // 0x1204
    RespErrInvalidParameter,  // 0x1205
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
    pmodes: [DisplayOne; MAX_SCANOUTS],
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct ResourceCreate2d {
    hdr: CtrlHeader,
    resource_id: u32,
    format: u32,
    width: u32,
    height: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct ResourceUnref {
    hdr: CtrlHeader,
    resource_id: u32,
    padding: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct SetScanout {
    hdr: CtrlHeader,
    r: Rect,
    scanout_id: u32,
    resource_id: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct ResourceFlush {
    hdr: CtrlHeader,
    r: Rect,
    resource_id: u32,
    padding: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct SetScanoutBlobReq {
    hdr: CtrlHeader,
    r: Rect,
    scanout_id: u32,
    resource_id: u32,
    width: u32,
    height: u32,
    format: u32,
    padding: u32,
    strides: [u32; 4],
    offsets: [u32; 4],
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct TransferToHost2d {
    hdr: CtrlHeader,
    r: Rect,
    offset: u64,
    resource_id: u32,
    padding: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct ResourceAttachBacking {
    hdr: CtrlHeader,
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
    hdr: CtrlHeader,
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

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct GetCapsetInfo {
    hdr: CtrlHeader,
    capset_index: u32,
    padding: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct RespCapsetInfo {
    hdr: CtrlHeader,
    capset_id: u32,
    capset_max_version: u32,
    capset_max_size: u32,
    padding: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct GetCapset {
    hdr: CtrlHeader,
    capset_id: u32,
    capset_version: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct CtxCreate {
    hdr: CtrlHeader,
    nlen: u32,
    context_init: u32,
    debug_name: [u8; 64],
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct CtxResource {
    hdr: CtrlHeader,
    resource_id: u32,
    padding: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct ResourceCreate3d {
    hdr: CtrlHeader,
    resource_id: u32,
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
    padding: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct Box3d {
    x: u32,
    y: u32,
    z: u32,
    w: u32,
    h: u32,
    d: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct TransferHost3d {
    hdr: CtrlHeader,
    r#box: Box3d,
    offset: u64,
    resource_id: u32,
    level: u32,
    stride: u32,
    layer_stride: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct ResourceCreateBlob {
    hdr: CtrlHeader,
    resource_id: u32,
    blob_mem: u32,
    blob_flags: u32,
    nr_entries: u32,
    blob_id: u64,
    size: u64,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct ResourceMapBlob {
    hdr: CtrlHeader,
    resource_id: u32,
    padding: u32,
    offset: u64,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct ResourceUnmapBlob {
    hdr: CtrlHeader,
    resource_id: u32,
    padding: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct RespMapInfo {
    hdr: CtrlHeader,
    map_info: u32,
    padding: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct CmdSubmit3d {
    hdr: CtrlHeader,
    size: u32,
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
const FLAG_RING_IDX: u32 = 1 << 1;

struct Resource {
    format: u32,
    width: u32,
    height: u32,
    backing: Vec<MemEntry>,
    framebuffer: Vec<u8>,
    is_3d: bool,
    mapped_gpa: Option<u64>,
    mapped_size: usize,
    blob_size: u64,

    scanout_stride: u32,
    scanout_offset: u64,

    iosurface: Option<ScopedIOSurface>,
}

pub struct DisplayBuffer {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
    pub dirty_rect: Option<(usize, usize, usize, usize)>,
    pub iosurface_id: Option<u32>,
    pub seq: u64,
}

impl DisplayBuffer {
    pub fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            pixels: Vec::new(),
            seq: 0,
            dirty_rect: None,
            iosurface_id: None,
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        if self.width == width && self.height == height {
            return;
        }

        self.width = width;
        self.height = height;
        self.pixels.resize(width * height, 0);
        self.dirty_rect = Some((0, 0, width, height));
        self.iosurface_id = None;
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

struct PendingFence {
    ctx_id: u32,
    ring_idx: Option<u8>,
    fence_id: u64,
    token: ChainToken,
    written: u32,
}

pub struct Gpu<'a> {
    resources: HashMap<u32, Resource>,
    scanout_resource: Option<u32>,
    display: &'a Mutex<DisplayBuffer>,

    scanout_width: u32,
    scanout_height: u32,

    events_read: u32,

    pending_fences: Vec<PendingFence>,
    submit_buf: Vec<u8>,
    renderer: &'a mut VirglRenderer,
}

impl<'a> Gpu<'a> {
    pub fn new(display: &'a Mutex<DisplayBuffer>, renderer: &'a mut VirglRenderer) -> Gpu<'a> {
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
            pending_fences: Vec::new(),
            submit_buf: Vec::new(),
            renderer,
        }
    }
}

impl<'a> Device for Gpu<'a> {
    fn id(&self) -> u32 {
        DEVICE_ID
    }

    fn features(&self) -> u64 {
        common::feature::VERSION_1 | feature::VIRGL | feature::RESOURCE_BLOB | feature::CONTEXT_INIT
    }

    fn read_config(&self, offset: u64, data: &mut [u8]) {
        let cfg = Config {
            events_read: self.events_read,
            events_clear: 0,
            num_scanouts: 1,
            num_capsets: self.renderer.get_num_capsets(),
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
        token: ChainToken,
        mem: &mut Memory,
    ) -> ChainAction {
        let Some(hdr) = chain.read_obj::<CtrlHeader>(0, mem) else {
            eprintln!("virtio-gpu: unreadable command header");
            return self.err(chain, CtrlType::RespErrUnspec, &CtrlHeader::new_zeroed(), mem);
        };

        if QueueType::try_from(queue_idx).unwrap() == QueueType::Cursor {
            return self.ok(chain, &hdr, mem);
        }

        match CtrlType::try_from(hdr.r#type) {
            Ok(CtrlType::GetDisplayInfo) => self.cmd_get_display_info(chain, &hdr, mem),
            Ok(CtrlType::GetCapsetInfo) => self.cmd_get_capset_info(chain, &hdr, mem),
            Ok(CtrlType::GetCapset) => self.cmd_get_capset(chain, &hdr, mem),
            Ok(CtrlType::CtxCreate) => self.cmd_ctx_create(chain, &hdr, mem),
            Ok(CtrlType::CtxDestroy) => self.cmd_ctx_destroy(chain, &hdr, mem),
            Ok(CtrlType::CtxAttachResource) => self.cmd_ctx_attach_resource(chain, &hdr, mem),
            Ok(CtrlType::CtxDetachResource) => self.cmd_ctx_detach_resource(chain, &hdr, mem),
            Ok(CtrlType::ResourceCreate3d) => self.cmd_resource_create_3d(chain, &hdr, mem),
            Ok(CtrlType::TransferToHost3d) => self.cmd_transfer_to_host_3d(chain, &hdr, mem),
            Ok(CtrlType::TransferFromHost3d) => self.cmd_transfer_from_host_3d(chain, &hdr, mem),
            Ok(CtrlType::Submit3d) => self.cmd_submit_3d(chain, &hdr, token, mem),
            Ok(CtrlType::ResourceFlush) => self.cmd_resource_flush(chain, &hdr, mem),
            Ok(CtrlType::ResourceCreateBlob) => self.cmd_resource_create_blob(chain, &hdr, mem),
            Ok(CtrlType::ResourceMapBlob) => self.resource_map_blob(chain, &hdr, mem),
            Ok(CtrlType::ResourceUnmapBlob) => self.resource_unmap_blob(chain, &hdr, mem),
            Ok(CtrlType::ResourceCreate2d) => self.cmd_resource_create_2d(chain, &hdr, mem),
            Ok(CtrlType::ResourceUnref) => self.cmd_resource_unref(chain, &hdr, mem),
            Ok(CtrlType::ResourceAttachBacking) => self.cmd_resource_attach_backing(chain, &hdr, mem),
            Ok(CtrlType::ResourceDetachBacking) => self.cmd_resource_detach_backing(chain, &hdr, mem),
            Ok(CtrlType::SetScanout) => self.cmd_set_scanout(chain, &hdr, mem),
            Ok(CtrlType::TransferToHost2d) => self.cmd_transfer_to_host_2d(chain, &hdr, mem),
            Ok(CtrlType::SetScanoutBlob) => self.cmd_set_scanout_blob(chain, &hdr, mem),
            Ok(cmd) => {
                eprintln!("virtio-gpu: unhandled command: {:?}", cmd);
                self.err(chain, CtrlType::RespErrUnspec, &hdr, mem)
            }
            Err(v) => {
                eprintln!("virtio-gpu: unknown command: 0x{:x}", v.number);
                self.err(chain, CtrlType::RespErrUnspec, &hdr, mem)
            }
        }
    }

    fn reset(&mut self) {
        self.resources.clear();
        self.scanout_resource = None;
        self.pending_fences.clear();
    }

    fn shared_memory_region(&self, id: u32) -> Option<ShmRegion> {
        if id as u64 == HOST_VISIBLE_SHM_ID {
            Some(ShmRegion {
                base: HOST_VISIBLE_SHM_BASE,
                len: HOST_VISIBLE_SHM_SIZE,
            })
        } else {
            None
        }
    }
}

impl<'a> Gpu<'a> {
    fn err(&self, chain: &ChainData, r#type: CtrlType, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        ChainAction::Complete(Gpu::write_response(chain, r#type, hdr, mem))
    }

    fn ok(&self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        ChainAction::Complete(Gpu::write_response(chain, CtrlType::RespOkNoData, hdr, mem))
    }

    fn cmd_get_display_info(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let resp_hdr = Gpu::resp_header(CtrlType::RespOkDisplayInfo, hdr);

        let mut resp = RespDisplayInfo::new_zeroed();
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

        ChainAction::Complete(chain.write_parts(&[resp_hdr.as_bytes(), resp.as_bytes()], mem))
    }

    fn cmd_get_capset_info(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(req) = chain.read_obj::<GetCapsetInfo>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let Ok((id, ver, size)) = self.renderer.get_capset_info(req.capset_index) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let mut resp = RespCapsetInfo::new_zeroed();
        resp.hdr = Gpu::resp_header(CtrlType::RespOkCapsetInfo, hdr);
        resp.capset_id = id;
        resp.capset_max_version = ver;
        resp.capset_max_size = size;

        ChainAction::Complete(chain.write_response(resp.as_bytes(), mem))
    }

    fn cmd_get_capset(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(req) = chain.read_obj::<GetCapset>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let data = match self.renderer.get_capset(req.capset_id, req.capset_version) {
            Ok(data) => data,
            Err(err) => {
                eprintln!(
                    "GetCapset FAILED: capset_id={} version={} err={:?}",
                    pf!(req, capset_id),
                    pf!(req, capset_version),
                    err,
                );

                return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
            }
        };

        let resp_hdr = Gpu::resp_header(CtrlType::RespOkCapset, hdr);
        ChainAction::Complete(chain.write_parts(&[resp_hdr.as_bytes(), &data], mem))
    }

    fn cmd_ctx_create(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(req) = chain.read_obj::<CtxCreate>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let nlen = (req.nlen as usize).min(req.debug_name.len());
        let name = std::str::from_utf8(&req.debug_name[..nlen]).unwrap_or("ctx");

        match self.renderer.create_ctx(hdr.ctx_ud, req.context_init, Some(name)) {
            Ok(()) => self.ok(chain, hdr, mem),
            Err(e) => {
                eprintln!(
                    "CtxCreate FAILED ctx_id={} context_init={} err={:?}",
                    pf!(hdr, ctx_ud),
                    pf!(req, context_init),
                    e,
                );

                self.err(chain, CtrlType::RespErrInvalidContextId, hdr, mem)
            }
        }
    }

    fn cmd_ctx_destroy(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        self.renderer.destroy_ctx(hdr.ctx_ud).unwrap();
        self.ok(chain, hdr, mem)
    }

    fn cmd_ctx_attach_resource(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(req) = chain.read_obj::<CtxResource>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = req.resource_id;

        let Some(resource) = self.resources.get(&resource_id) else {
            eprintln!(
                "CtxAttach FAILED: resource not in local map ctx_id={} resource_id={}",
                pf!(hdr, ctx_ud),
                resource_id,
            );
            return self.err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        if !resource.is_3d {
            eprintln!(
                "CtxAttach: ignoring attach of local 2D resource {} to ctx {}",
                resource_id,
                pf!(hdr, ctx_ud),
            );
            return self.ok(chain, hdr, mem);
        }

        let ctx_ud = hdr.ctx_ud;
        match self.renderer.ctx_attach_resource(hdr.ctx_ud, resource_id) {
            Ok(()) => self.ok(chain, hdr, mem),
            Err(e) => {
                eprintln!(
                    "CtxAttach FAILED ctx_id={} resource_id={} err={:?}",
                    ctx_ud, resource_id, e,
                );
                self.err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem)
            }
        }
    }

    fn cmd_ctx_detach_resource(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(req) = chain.read_obj::<CtxResource>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = req.resource_id;

        let Some(resource) = self.resources.get(&resource_id) else {
            return self.err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        if !resource.is_3d {
            eprintln!(
                "CtxDetach: ignoring detach of local 2D resource {} from ctx {}",
                resource_id,
                pf!(hdr, ctx_ud),
            );

            return self.ok(chain, hdr, mem);
        }

        match self.renderer.ctx_detach_resource(hdr.ctx_ud, resource_id) {
            Ok(()) => self.ok(chain, hdr, mem),
            Err(e) => {
                eprintln!(
                    "CtxDetach FAILED ctx_id={} resource_id={} err={:?}",
                    pf!(hdr, ctx_ud),
                    resource_id,
                    e,
                );
                self.err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem)
            }
        }
    }

    fn cmd_resource_create_3d(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(req) = chain.read_obj::<ResourceCreate3d>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let info = ResourceCreate3D {
            target: req.target,
            format: req.format,
            bind: req.bind,
            width: req.width,
            height: req.height,
            depth: req.depth,
            array_size: req.array_size,
            last_level: req.last_level,
            nr_samples: req.nr_samples,
            flags: req.flags,
        };

        if self.renderer.resource_create_3d(req.resource_id, info).is_err() {
            return self.err(chain, CtrlType::RespErrUnspec, hdr, mem);
        }

        self.resources.insert(
            req.resource_id,
            Resource {
                format: req.format,
                width: req.width,
                height: req.height,
                backing: Vec::new(),
                framebuffer: Vec::new(),
                is_3d: true,
                mapped_gpa: None,
                mapped_size: 0,
                blob_size: 0,
                scanout_stride: req.width.saturating_mul(BYTES_PER_PIXEL as u32),
                scanout_offset: 0,
                iosurface: None,
            },
        );

        self.ok(chain, hdr, mem)
    }

    fn cmd_transfer_to_host_3d(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(req) = chain.read_obj::<TransferHost3d>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        self.renderer
            .transfer_write(
                hdr.ctx_ud,
                req.resource_id,
                Transfer3D {
                    x: req.r#box.x,
                    y: req.r#box.y,
                    z: req.r#box.z,
                    w: req.r#box.w,
                    h: req.r#box.h,
                    d: req.r#box.d,
                    level: req.level,
                    stride: req.stride,
                    layer_stride: req.layer_stride,
                    offset: req.offset,
                },
            )
            .unwrap();

        self.ok(chain, hdr, mem)
    }

    fn cmd_transfer_from_host_3d(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(req) = chain.read_obj::<TransferHost3d>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        self.renderer
            .transfer_read(
                hdr.ctx_ud,
                req.resource_id,
                Transfer3D {
                    x: req.r#box.x,
                    y: req.r#box.y,
                    z: req.r#box.z,
                    w: req.r#box.w,
                    h: req.r#box.h,
                    d: req.r#box.d,
                    level: req.level,
                    stride: req.stride,
                    layer_stride: req.layer_stride,
                    offset: req.offset,
                },
            )
            .unwrap();

        self.ok(chain, hdr, mem)
    }

    fn cmd_submit_3d(
        &mut self,
        chain: &ChainData,
        hdr: &CtrlHeader,
        token: ChainToken,
        mem: &mut Memory,
    ) -> ChainAction {
        let Some(req) = chain.read_obj::<CmdSubmit3d>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let payload_offset = size_of::<CmdSubmit3d>();
        let readable_len = chain.readable_len();
        let has_ring_idx = (hdr.flags & FLAG_RING_IDX) != 0;

        if req.size as usize > readable_len.saturating_sub(payload_offset) {
            eprintln!(
                "Submit3d FAILED: invalid size ctx_id={} submit_size={} readable_len={} payload_offset={}",
                pf!(hdr, ctx_ud),
                pf!(req, size),
                readable_len,
                payload_offset,
            );
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        self.submit_buf.resize(req.size as usize, 0);

        if chain.read_at(payload_offset, &mut self.submit_buf, mem).is_none() {
            eprintln!(
                "Submit3d FAILED: cannot read payload ctx_id={} size={}",
                pf!(hdr, ctx_ud),
                pf!(req, size),
            );
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        let submit_result = self.renderer.submit_command(hdr.ctx_ud, &mut self.submit_buf);

        if let Err(err) = submit_result {
            eprintln!(
                "Submit3d ERR ctx_id={} size={} flags={:#x} fence_id={} ring_idx={} err={:?}",
                pf!(hdr, ctx_ud),
                pf!(req, size),
                pf!(hdr, flags),
                pf!(hdr, fence_id),
                pf!(hdr, ring_idx),
                err,
            );
            return self.err(chain, CtrlType::RespErrUnspec, hdr, mem);
        }

        if hdr.flags & FLAG_FENCE != 0 {
            let fence = VirglFence {
                fence_id: hdr.fence_id,
                ctx_id: hdr.ctx_ud,
                ring_idx: has_ring_idx.then_some(hdr.ring_idx.into()),
            };

            if let Err(err) = self.renderer.create_fence(fence) {
                eprintln!(
                    "CreateFence ERR ctx_id={} fence_id={} ring_idx={} err={:?}",
                    pf!(hdr, ctx_ud),
                    pf!(hdr, fence_id),
                    pf!(hdr, ring_idx),
                    err,
                );
                return self.err(chain, CtrlType::RespErrUnspec, hdr, mem);
            }

            let written = Gpu::write_response(chain, CtrlType::RespOkNoData, hdr, mem);

            self.pending_fences.push(PendingFence {
                ctx_id: hdr.ctx_ud,
                ring_idx: has_ring_idx.then_some(hdr.ring_idx),
                fence_id: hdr.fence_id,
                token,
                written,
            });

            return ChainAction::Deferred;
        }

        self.ok(chain, hdr, mem)
    }

    fn cmd_resource_flush(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(val) = chain.read_obj::<ResourceFlush>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = val.resource_id;
        let flush_rect = val.r;

        if self.scanout_resource != Some(resource_id) {
            return self.ok(chain, hdr, mem);
        }

        let Some(resource) = self.resources.get(&resource_id) else {
            return self.err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        let is_3d = resource.is_3d;
        let is_guest_backed_blob = resource.blob_size != 0 && !resource.backing.is_empty();

        let mut iosurface_id = None;

        if is_guest_backed_blob {
            if !self.readback_blob_scanout(resource_id, flush_rect, mem) {
                return self.err(chain, CtrlType::RespErrUnspec, hdr, mem);
            }
        } else if is_3d {
            let Some(source) = self.renderer.native_source_info(resource_id) else {
                eprintln!(
                    "virtio-gpu: ResourceFlush failed: 3D scanout resource={} has no native source",
                    resource_id
                );
                return self.err(chain, CtrlType::RespErrUnspec, hdr, mem);
            };

            iosurface_id = self.copy_metal_texture_to_iosurface(resource_id, source);

            if iosurface_id.is_none() {
                eprintln!(
                    "virtio-gpu: ResourceFlush failed: native scanout copy failed resource={}",
                    resource_id
                );
                return self.err(chain, CtrlType::RespErrUnspec, hdr, mem);
            }
        }

        let Some(resource) = self.resources.get(&resource_id) else {
            return self.err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        self.publish_display_rect(resource_id, resource, flush_rect, iosurface_id);

        self.ok(chain, hdr, mem)
    }

    fn cmd_resource_create_blob(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(req) = chain.read_obj::<ResourceCreateBlob>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let mut iovecs = Vec::with_capacity(req.nr_entries as usize);
        let mut backing = Vec::with_capacity(req.nr_entries as usize);

        if req.nr_entries > 0 {
            let entries_base = size_of::<ResourceCreateBlob>();
            let entry_size = size_of::<MemEntry>();
            if entries_base + req.nr_entries as usize * entry_size > chain.readable_len() {
                return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
            }
            for i in 0..req.nr_entries as usize {
                let Some(e) = chain.read_obj::<MemEntry>(entries_base + i * entry_size, mem) else {
                    return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
                };
                let Some(base) = gpa_to_host(mem, e.addr, e.length as u64) else {
                    return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
                };
                backing.push(e);
                iovecs.push(Iovec {
                    iov_base: base,
                    iov_len: e.length as usize,
                });
            }
        }

        let blob = VirglResourceCreateBlob {
            blob_mem: req.blob_mem,
            blob_flags: req.blob_flags,
            blob_id: req.blob_id,
            size: req.size,
        };

        let iovecs_opt = if iovecs.is_empty() { None } else { Some(iovecs) };
        let ctx_ud = hdr.ctx_ud;
        let resource_id = req.resource_id;
        let blob_mem = req.blob_mem;
        let blob_flags = req.blob_flags;
        let blob_id = req.blob_id;
        let blob_size = req.size;
        let nr_entries = req.nr_entries;

        let create_blob_result = self
            .renderer
            .resource_create_blob(ctx_ud, resource_id, blob, iovecs_opt);

        if let Err(err) = create_blob_result {
            eprintln!(
                "ResourceCreateBlob FAILED ctx_id={} resource_id={} blob_mem={:#x} blob_flags={:#x} blob_id={:#x} size={} nr_entries={} err={:?}",
                ctx_ud, resource_id, blob_mem, blob_flags, blob_id, blob_size, nr_entries, err,
            );
            return self.err(chain, CtrlType::RespErrUnspec, hdr, mem);
        }

        self.resources.insert(
            req.resource_id,
            Resource {
                format: 0,
                width: 0,
                height: 0,
                backing,
                framebuffer: Vec::new(),
                is_3d: true,
                mapped_gpa: None,
                mapped_size: 0,
                blob_size: req.size,
                scanout_stride: 0,
                scanout_offset: 0,
                iosurface: None,
            },
        );

        self.ok(chain, hdr, mem)
    }

    fn cmd_resource_create_2d(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(val) = chain.read_obj::<ResourceCreate2d>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        if val.format != 2 {
            eprintln!("virtio-gpu: unsupported format: {}", { val.format });
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        if val.width == 0 || val.height == 0 || val.width > 16384 || val.height > 16384 {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        self.resources.insert(
            val.resource_id,
            Resource {
                format: val.format,
                width: val.width,
                height: val.height,
                backing: Vec::new(),
                framebuffer: Vec::new(),
                is_3d: false,
                scanout_offset: 0,
                scanout_stride: 0,
                mapped_gpa: None,
                mapped_size: 0,
                blob_size: 0,
                iosurface: None,
            },
        );

        self.ok(chain, hdr, mem)
    }

    fn cmd_resource_unref(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(val) = chain.read_obj::<ResourceUnref>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = val.resource_id;

        if let Some(resource) = self.resources.remove(&resource_id)
            && resource.is_3d
        {
            self.renderer.resource_unref(resource_id);
        }

        if self.scanout_resource == Some(resource_id) {
            self.scanout_resource = None;
        }

        self.ok(chain, hdr, mem)
    }

    fn cmd_resource_attach_backing(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(val) = chain.read_obj::<ResourceAttachBacking>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let n_entries = val.nr_entries as usize;
        let entries_base = size_of::<ResourceAttachBacking>();
        let entry_size = size_of::<MemEntry>();

        if entries_base + n_entries * entry_size > chain.readable_len() {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        let resource_id = val.resource_id;
        let Some(resource) = self.resources.get_mut(&resource_id) else {
            return self.err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        let mut backing = Vec::with_capacity(n_entries);
        for i in 0..n_entries {
            let Some(entry) = chain.read_obj::<MemEntry>(entries_base + i * entry_size, mem) else {
                return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
            };
            backing.push(entry);
        }

        let renderer_backing = backing
            .iter()
            .map(|b| Iovec {
                iov_base: gpa_to_host(mem, b.addr, b.length as u64).unwrap(),
                iov_len: b.length as usize,
            })
            .collect();

        if resource.is_3d {
            self.renderer.attach_backing(resource_id, renderer_backing).unwrap();
        }

        resource.backing = backing;

        self.ok(chain, hdr, mem)
    }

    fn cmd_resource_detach_backing(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(val) = chain.read_obj::<ResourceDetachBacking>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = val.resource_id;
        let Some(resource) = self.resources.get_mut(&resource_id) else {
            return self.err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        resource.backing.clear();

        self.ok(chain, hdr, mem)
    }

    fn cmd_set_scanout(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(val) = chain.read_obj::<SetScanout>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = val.resource_id;

        if resource_id == 0 {
            self.scanout_resource = None;
        } else {
            let Some(resource) = self.resources.get(&resource_id) else {
                return self.err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
            };

            self.scanout_resource = Some(resource_id);
            self.display
                .lock()
                .unwrap()
                .resize(resource.width as usize, resource.height as usize);
        }

        self.ok(chain, hdr, mem)
    }

    fn cmd_transfer_to_host_2d(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(val) = chain.read_obj::<TransferToHost2d>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = val.resource_id;
        let Some(resource) = self.resources.get_mut(&resource_id) else {
            return self.err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        let width = resource.width as usize;
        let height = resource.height as usize;
        let is_3d = resource.is_3d;
        let stride = width * BYTES_PER_PIXEL;

        let rect_x = val.r.x as usize;
        let rect_y = val.r.y as usize;
        let rect_width = val.r.width as usize;
        let rect_height = val.r.height as usize;
        let transfer_offset = val.offset as usize;

        if rect_x + rect_width > width || rect_y + rect_height > height {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        if is_3d {
            return self.ok(chain, hdr, mem);
        }

        let fb_len = height * stride;
        resource.framebuffer.resize(fb_len, 0);

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
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        self.ok(chain, hdr, mem)
    }

    fn cmd_set_scanout_blob(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(val) = chain.read_obj::<SetScanoutBlobReq>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = val.resource_id;
        let scanout_id = val.scanout_id;
        let width = val.width;
        let height = val.height;
        let format = val.format;

        if scanout_id != 0 {
            return self.err(chain, CtrlType::RespErrInvalidScanoutId, hdr, mem);
        }

        if resource_id == 0 {
            self.scanout_resource = None;
            return self.ok(chain, hdr, mem);
        }

        if width == 0 || height == 0 || width > 16384 || height > 16384 {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        let Some(resource) = self.resources.get_mut(&resource_id) else {
            return self.err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        resource.width = width;
        resource.height = height;
        resource.format = format;
        resource.scanout_stride = if val.strides[0] != 0 {
            val.strides[0]
        } else {
            width.saturating_mul(BYTES_PER_PIXEL as u32)
        };
        resource.scanout_offset = val.offsets[0] as u64;

        self.scanout_resource = Some(resource_id);
        self.display.lock().unwrap().resize(width as usize, height as usize);

        self.ok(chain, hdr, mem)
    }

    fn copy_metal_texture_to_iosurface(
        &mut self,
        resource_id: u32,
        native_source: crate::virtio::virgl_ffi::NativeSourceInfo,
    ) -> Option<u32> {
        let width = native_source.width;
        let height = native_source.height;

        let Some(resource) = self.resources.get_mut(&resource_id) else {
            return None;
        };

        let recreate = match resource.iosurface.as_ref() {
            Some(surface) => surface.width() != width || surface.height() != height,
            None => true,
        };

        if recreate {
            resource.iosurface = ScopedIOSurface::new_bgra(width, height);
        }

        let Some(surface) = resource.iosurface.as_ref() else {
            return None;
        };

        let ok = crate::angle_egl::copy_metal_texture_to_iosurface(
            native_source.handle as *mut c_void,
            surface.as_ptr(),
            width,
            height,
        );

        if ok {
            Some(surface.id())
        } else {
            eprintln!("virtio-gpu: ANGLE producer copy failed; falling back to readback");
            None
        }
    }

    fn readback_blob_scanout(&mut self, resource_id: u32, rect: Rect, mem: &Memory) -> bool {
        let Some(resource) = self.resources.get_mut(&resource_id) else {
            return false;
        };

        let width = resource.width as usize;
        let height = resource.height as usize;

        if width == 0 || height == 0 || width > 16384 || height > 16384 {
            eprintln!(
                "readback_blob_scanout: invalid size res={} {}x{}",
                resource_id, width, height
            );
            return false;
        }

        if resource.backing.is_empty() {
            eprintln!("readback_blob_scanout: no backing res={}", resource_id);
            return false;
        }

        let src_stride = if resource.scanout_stride != 0 {
            resource.scanout_stride as usize
        } else {
            width * BYTES_PER_PIXEL
        };

        let dst_stride = width * BYTES_PER_PIXEL;
        let fb_len = height * dst_stride;

        if resource.framebuffer.len() != fb_len {
            resource.framebuffer.resize(fb_len, 0);
        }

        let rect_x = (rect.x as usize).min(width);
        let rect_y = (rect.y as usize).min(height);
        let rect_w = (rect.width as usize).min(width.saturating_sub(rect_x));
        let rect_h = (rect.height as usize).min(height.saturating_sub(rect_y));

        if rect_w == 0 || rect_h == 0 {
            return true;
        }

        let row_len = rect_w * BYTES_PER_PIXEL;
        let base = resource.scanout_offset as usize;

        for row in 0..rect_h {
            let src_offset = base + (rect_y + row) * src_stride + rect_x * BYTES_PER_PIXEL;
            let dst_offset = (rect_y + row) * dst_stride + rect_x * BYTES_PER_PIXEL;

            if Gpu::read_backing(
                &resource.backing,
                src_offset,
                &mut resource.framebuffer[dst_offset..dst_offset + row_len],
                mem,
            )
            .is_none()
            {
                eprintln!(
                    "readback_blob_scanout: read_backing failed res={} row={} src_offset={} row_len={} stride={} base={}",
                    resource_id, row, src_offset, row_len, src_stride, base
                );
                return false;
            }
        }

        true
    }

    fn publish_display_rect(&self, _resource_id: u32, resource: &Resource, rect: Rect, iosurface_id: Option<u32>) {
        let mut display = self.display.lock().unwrap();

        let res_width = resource.width as usize;
        let res_height = resource.height as usize;

        let x0 = (rect.x as usize).min(res_width).min(display.width);
        let y0 = (rect.y as usize).min(res_height).min(display.height);
        let x1 = ((rect.x + rect.width) as usize).min(res_width).min(display.width);
        let y1 = ((rect.y + rect.height) as usize).min(res_height).min(display.height);

        if x0 >= x1 || y0 >= y1 {
            return;
        }

        if iosurface_id.is_none() {
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
        }

        display.iosurface_id = iosurface_id;

        display.dirty_rect = match display.dirty_rect {
            Some((old_x, old_y, old_w, old_h)) => {
                let old_x1 = old_x.saturating_add(old_w);
                let old_y1 = old_y.saturating_add(old_h);
                let nx0 = old_x.min(x0);
                let ny0 = old_y.min(y0);
                let nx1 = old_x1.max(x1);
                let ny1 = old_y1.max(y1);
                Some((nx0, ny0, nx1 - nx0, ny1 - ny0))
            }
            None => Some((x0, y0, x1 - x0, y1 - y0)),
        };

        display.seq = display.seq.wrapping_add(1);
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

    fn write_response(chain: &ChainData, r#type: CtrlType, req: &CtrlHeader, mem: &mut Memory) -> u32 {
        chain.write_response(Gpu::resp_header(r#type, req).as_bytes(), mem)
    }

    fn read_backing(backing: &[MemEntry], mut src_offset: usize, dst: &mut [u8], mem: &Memory) -> Option<()> {
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

            mem.read(entry.addr + in_entry_off as u64, &mut dst[written..written + copy_len])
                .ok()?;

            written += copy_len;
            src_offset = 0;

            if written == dst.len() {
                return Some(());
            }
        }

        None
    }

    fn resource_map_blob(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(req) = chain.read_obj::<ResourceMapBlob>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = req.resource_id;
        let offset = req.offset;

        let Some(resource) = self.resources.get(&resource_id) else {
            eprintln!("ResourceMapBlob FAILED: unknown resource_id={}", resource_id,);
            return self.err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        let size = resource.blob_size;
        if size == 0 {
            eprintln!("ResourceMapBlob FAILED: resource_id={} has zero blob_size", resource_id,);
            return self.err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        }

        let align = APPLE_HV_PAGE_SIZE as u64;

        if offset & (align - 1) != 0 {
            eprintln!(
                "ResourceMapBlob FAILED: unaligned offset resource_id={} offset={:#x} align={:#x}",
                resource_id, offset, align,
            );
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        let rounded_size = match align_up(size as usize, APPLE_HV_PAGE_SIZE) {
            Some(v) => v as u64,
            None => {
                eprintln!(
                    "ResourceMapBlob FAILED: size overflow resource_id={} size={}",
                    resource_id, size,
                );
                return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
            }
        };

        if offset
            .checked_add(rounded_size)
            .is_none_or(|end| end > HOST_VISIBLE_SHM_SIZE)
        {
            eprintln!(
                "ResourceMapBlob FAILED: rounded mapping does not fit resource_id={} offset={:#x} size={} rounded_size={} shm_size={}",
                resource_id, offset, size, rounded_size, HOST_VISIBLE_SHM_SIZE,
            );
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        let map_info = match self.renderer.map_info(resource_id) {
            Ok(map_info) => map_info,
            Err(err) => {
                eprintln!(
                    "ResourceMapBlob FAILED: map_info resource_id={} err={:?}",
                    resource_id, err,
                );
                return self.err(chain, CtrlType::RespErrUnspec, hdr, mem);
            }
        };

        let map_ptr = match self.renderer.map_ptr(resource_id) {
            Ok(map_ptr) => map_ptr,
            Err(err) => {
                eprintln!(
                    "ResourceMapBlob FAILED: map_ptr resource_id={} err={:?}",
                    resource_id, err,
                );
                return self.err(chain, CtrlType::RespErrUnspec, hdr, mem);
            }
        };

        let guest_addr = HOST_VISIBLE_SHM_BASE + offset;

        let (mapped_gpa, mapped_size) = match map_blob_to_guest(map_ptr, guest_addr, size as usize) {
            Ok(mapping) => mapping,
            Err(err) => {
                eprintln!(
                    "ResourceMapBlob FAILED: hv_vm_map resource_id={} map_ptr={:#x} guest_addr={:#x} size={} err={:#x}",
                    resource_id, map_ptr, guest_addr, size, err,
                );
                return self.err(chain, CtrlType::RespErrUnspec, hdr, mem);
            }
        };

        let Some(resource) = self.resources.get_mut(&resource_id) else {
            let _ = unmap_blob_from_guest(mapped_gpa, mapped_size);
            return self.err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        resource.mapped_gpa = Some(mapped_gpa);
        resource.mapped_size = mapped_size;

        let resp = RespMapInfo {
            hdr: Gpu::resp_header(CtrlType::RespOkMapInfo, hdr),
            map_info: map_info & MAP_CACHE_MASK,
            padding: 0,
        };

        ChainAction::Complete(chain.write_response(resp.as_bytes(), mem))
    }

    fn resource_unmap_blob(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &mut Memory) -> ChainAction {
        let Some(req) = chain.read_obj::<ResourceUnmapBlob>(0, mem) else {
            return self.err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = req.resource_id;

        let (mapped_gpa, mapped_size) = {
            let Some(resource) = self.resources.get_mut(&resource_id) else {
                eprintln!("ResourceUnmapBlob FAILED: unknown resource_id={}", resource_id,);
                return self.err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
            };

            let Some(mapped_gpa) = resource.mapped_gpa.take() else {
                eprintln!("ResourceUnmapBlob FAILED: resource_id={} is not mapped", resource_id,);
                return self.err(chain, CtrlType::RespErrUnspec, hdr, mem);
            };

            let mapped_size = resource.mapped_size;
            resource.mapped_size = 0;

            (mapped_gpa, mapped_size)
        };

        if let Err(err) = unmap_blob_from_guest(mapped_gpa, mapped_size) {
            eprintln!(
                "ResourceUnmapBlob FAILED: hv_vm_unmap resource_id={} guest_addr={:#x} size={} err={:#x}",
                resource_id, mapped_gpa, mapped_size, err,
            );
            return self.err(chain, CtrlType::RespErrUnspec, hdr, mem);
        }

        if let Err(err) = self.renderer.unmap(resource_id) {
            eprintln!(
                "ResourceUnmapBlob FAILED: virgl unmap resource_id={} err={:?}",
                resource_id, err,
            );
            return self.err(chain, CtrlType::RespErrUnspec, hdr, mem);
        }

        self.ok(chain, hdr, mem)
    }
}

pub enum ExternalEvent {
    DisplayResized {
        width: u32,
        height: u32,
    },
    FenceSignaled {
        ctx_id: u32,
        ring_idx: Option<u8>,
        fence_id: u64,
    },
    PollRendererFences,
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
            ExternalEvent::PollRendererFences => {
                if !self.pending_fences.is_empty() {
                    self.renderer.poll_ctxs();
                }
            }
            ExternalEvent::FenceSignaled {
                ctx_id,
                ring_idx,
                fence_id,
            } => {
                let mut i = 0;

                while i < self.pending_fences.len() {
                    let p = &self.pending_fences[i];

                    let same_timeline = match (p.ring_idx, ring_idx) {
                        (Some(pr), Some(sr)) => p.ctx_id == ctx_id && pr == sr,
                        (None, None) => true,
                        _ => false,
                    };

                    if same_timeline && p.fence_id <= fence_id {
                        let p = self.pending_fences.remove(i);

                        emit(Effect::Complete {
                            token: p.token,
                            written: p.written,
                        });
                    } else {
                        i += 1;
                    }
                }
            }
        }
    }
}

fn gpa_to_host(mem: &Memory, gpa: u64, len: u64) -> Option<*mut c_void> {
    let base_gpa = mem.guest_addr()?;
    let size = mem.size() as u64;

    if gpa < base_gpa {
        return None;
    }
    if gpa.checked_add(len)? > base_gpa.checked_add(size)? {
        return None;
    }
    let offset = gpa - base_gpa;
    Some(unsafe { mem.host_addr().add(offset as usize) } as *mut c_void)
}

const HV_SUCCESS: i32 = 0;
const HV_MEMORY_READ: u64 = 1 << 0;
const HV_MEMORY_WRITE: u64 = 1 << 1;

fn align_up(value: usize, align: usize) -> Option<usize> {
    let mask = align.checked_sub(1)?;
    value.checked_add(mask).map(|v| v & !mask)
}

fn align_down_u64(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

fn map_blob_to_guest(host_addr: u64, guest_addr: u64, size: usize) -> Result<(u64, usize), i32> {
    let page_size = APPLE_HV_PAGE_SIZE as u64;

    let host_base = align_down_u64(host_addr, page_size);
    let guest_base = align_down_u64(guest_addr, page_size);

    let host_delta = host_addr - host_base;
    let guest_delta = guest_addr - guest_base;

    if host_delta != guest_delta {
        eprintln!(
            "blob mapping alignment mismatch: host_addr={:#x} guest_addr={:#x} host_delta={:#x} guest_delta={:#x}",
            host_addr, guest_addr, host_delta, guest_delta,
        );
        return Err(-1);
    }

    let map_size = align_up(size.checked_add(host_delta as usize).ok_or(-1)?, APPLE_HV_PAGE_SIZE).ok_or(-1)?;

    let ret = unsafe {
        hv_vm_map(
            host_base as *const c_void,
            guest_base,
            map_size,
            HV_MEMORY_READ | HV_MEMORY_WRITE,
        )
    };

    if ret == HV_SUCCESS {
        Ok((guest_base, map_size))
    } else {
        Err(ret)
    }
}

fn unmap_blob_from_guest(guest_base: u64, size: usize) -> Result<(), i32> {
    let ret = unsafe { hv_vm_unmap(guest_base, size) };

    if ret == HV_SUCCESS { Ok(()) } else { Err(ret) }
}
