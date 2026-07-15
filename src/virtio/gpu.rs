//! https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-4040006
use crate::{
    memory::GuestMemory,
    virtio::{
        chain::ChainData,
        common,
        device::{ChainAction, ChainToken, Device, DeviceContext, ExternalEventHandler, ShmRegion},
        virgl_ffi::{
            Iovec, NativeTexture, ResourceCreate3D, ResourceCreateBlob as VirglResourceCreateBlob, Transfer3D,
            VirglFence, VirglRenderer,
        },
    },
};
use applevisor_sys::{hv_vm_map, hv_vm_unmap};
use num_enum::TryFromPrimitive;
use std::{collections::HashMap, ffi::c_void, mem::offset_of};
use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes};

mod edid;

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
    pub const EDID: u64 = 1 << 1;
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
struct GetEdid {
    hdr: CtrlHeader,
    scanout: u32,
    padding: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct RespEdidHeader {
    hdr: CtrlHeader,
    size: u32,
    padding: u32,
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
    kind: ResourceKind,
}

struct Scanout {
    resource_id: u32,
    source: ScanoutSource,
}

#[derive(Clone, Copy)]
pub struct PresentRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy)]
pub enum PresentSource<'a> {
    Pixels {
        data: &'a [u8],
        width: u32,
        height: u32,
        stride: u32,
    },
    NativeTexture(NativeTexture),
}

#[derive(Clone, Copy)]
pub struct PresentFrame<'a> {
    pub source: PresentSource<'a>,
    pub source_rect: PresentRect,
    pub damage: PresentRect,
}

#[derive(Clone, Copy)]
pub enum Presentation<'a> {
    Configure { width: u32, height: u32 },
    Frame(PresentFrame<'a>),
}

enum ScanoutSource {
    Resource {
        rect: Rect,
    },
    Blob {
        width: u32,
        height: u32,
        stride: u32,
        offset: u64,
    },
}

enum ResourceKind {
    Local2d,
    Renderer3d,
    RendererBlob(BlobResource),
}

struct BlobResource {
    size: u64,
    mapping: Option<BlobMapping>,
}

struct BlobMapping {
    guest_addr: u64,
    size: usize,
    mapped: bool,
}

impl BlobMapping {
    fn map(host_addr: u64, guest_addr: u64, size: usize) -> Result<BlobMapping, i32> {
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

        if ret != HV_SUCCESS {
            return Err(ret);
        }

        Ok(BlobMapping {
            guest_addr: guest_base,
            size: map_size,
            mapped: true,
        })
    }

    fn unmap(&mut self) -> Result<(), i32> {
        if !self.mapped {
            return Ok(());
        }

        let ret = unsafe { hv_vm_unmap(self.guest_addr, self.size) };
        if ret != HV_SUCCESS {
            return Err(ret);
        }

        self.mapped = false;
        Ok(())
    }
}

impl Drop for BlobMapping {
    fn drop(&mut self) {
        if let Err(err) = self.unmap() {
            eprintln!(
                "BlobMapping cleanup FAILED: hv_vm_unmap guest_addr={:#x} size={} err={:#x}",
                self.guest_addr, self.size, err,
            );
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

struct PendingFence {
    ctx_id: u32,
    ring_idx: Option<u8>,
    fence_id: u64,
    token: ChainToken,
    written: u32,
}

pub struct Gpu<'a> {
    resources: HashMap<u32, Resource>,
    scanout: Option<Scanout>,
    display_width: u32,
    display_height: u32,
    edid_compatibility_mode: Option<(u32, u32)>,
    events_read: u32,
    pending_fences: Vec<PendingFence>,
    submit_buf: Vec<u8>,
    renderer: &'a mut VirglRenderer,
    on_present: Box<dyn for<'frame> FnMut(Presentation<'frame>) -> bool + 'a>,
}

impl<'a> Gpu<'a> {
    pub fn new(
        renderer: &'a mut VirglRenderer,
        on_present: impl for<'frame> FnMut(Presentation<'frame>) -> bool + 'a,
    ) -> Gpu<'a> {
        Gpu {
            resources: HashMap::new(),
            scanout: None,
            display_width: 0,
            display_height: 0,
            edid_compatibility_mode: None,
            events_read: 0,
            pending_fences: Vec::new(),
            submit_buf: Vec::new(),
            renderer,
            on_present: Box::new(on_present),
        }
    }
}

impl<'a> Device for Gpu<'a> {
    fn id(&self) -> u32 {
        DEVICE_ID
    }

    fn features(&self) -> u64 {
        common::feature::VERSION_1 | feature::VIRGL | feature::EDID | feature::RESOURCE_BLOB | feature::CONTEXT_INIT
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

    fn queue_notified(&mut self, queue_idx: usize, ctx: &mut DeviceContext<'_>) {
        while let Some(chain) = ctx.pop_chain(queue_idx) {
            match self.process_chain(queue_idx, &chain.data, chain.token, ctx.mem()) {
                ChainAction::Complete(written) => ctx.complete(chain.token, written),
                ChainAction::Deferred => {}
            }
        }
    }

    fn reset(&mut self) {
        while let Some(resource_id) = self.resources.keys().next().copied() {
            self.destroy_resource(resource_id);
        }
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
    fn process_chain(
        &mut self,
        queue_idx: usize,
        chain: &ChainData,
        token: ChainToken,
        mem: &GuestMemory,
    ) -> ChainAction {
        let Some(hdr) = chain.read_obj::<CtrlHeader>(0, mem) else {
            eprintln!("virtio-gpu: unreadable command header");
            return Gpu::err(chain, CtrlType::RespErrUnspec, &CtrlHeader::new_zeroed(), mem);
        };

        if QueueType::try_from(queue_idx).unwrap() == QueueType::Cursor {
            return Gpu::ok(chain, &hdr, mem);
        }

        match CtrlType::try_from(hdr.r#type) {
            Ok(CtrlType::GetDisplayInfo) => self.cmd_get_display_info(chain, &hdr, mem),
            Ok(CtrlType::GetEdid) => self.cmd_get_edid(chain, &hdr, mem),
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
                Gpu::err(chain, CtrlType::RespErrUnspec, &hdr, mem)
            }
            Err(v) => {
                eprintln!("virtio-gpu: unknown command: 0x{:x}", v.number);
                Gpu::err(chain, CtrlType::RespErrUnspec, &hdr, mem)
            }
        }
    }

    fn err(chain: &ChainData, r#type: CtrlType, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        ChainAction::Complete(Gpu::write_response(chain, r#type, hdr, mem))
    }

    fn ok(chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        ChainAction::Complete(Gpu::write_response(chain, CtrlType::RespOkNoData, hdr, mem))
    }

    fn cmd_get_display_info(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let resp_hdr = Gpu::resp_header(CtrlType::RespOkDisplayInfo, hdr);

        let mut resp = RespDisplayInfo::new_zeroed();
        resp.pmodes[0] = DisplayOne {
            r: Rect {
                x: 0,
                y: 0,
                width: self.display_width,
                height: self.display_height,
            },
            enabled: 1,
            flags: 0,
        };

        ChainAction::Complete(chain.write_parts(&[resp_hdr.as_bytes(), resp.as_bytes()], mem))
    }

    fn cmd_get_edid(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(req) = chain.read_obj::<GetEdid>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        if req.scanout != 0 {
            return Gpu::err(chain, CtrlType::RespErrInvalidScanoutId, hdr, mem);
        }

        let width = self.display_width.max(32);
        let height = self.display_height.max(32);
        let Some(edid) = edid::build(width, height, self.edid_compatibility_mode) else {
            eprintln!("virtio-gpu: cannot build EDID for {}x{}", width, height);
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resp = RespEdidHeader {
            hdr: Gpu::resp_header(CtrlType::RespOkEdid, hdr),
            size: edid.len() as u32,
            padding: 0,
        };

        ChainAction::Complete(chain.write_parts(&[resp.as_bytes(), &edid], mem))
    }

    fn cmd_get_capset_info(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(req) = chain.read_obj::<GetCapsetInfo>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let Ok((id, ver, size)) = self.renderer.get_capset_info(req.capset_index) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let mut resp = RespCapsetInfo::new_zeroed();
        resp.hdr = Gpu::resp_header(CtrlType::RespOkCapsetInfo, hdr);
        resp.capset_id = id;
        resp.capset_max_version = ver;
        resp.capset_max_size = size;

        ChainAction::Complete(chain.write_response(resp.as_bytes(), mem))
    }

    fn cmd_get_capset(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(req) = chain.read_obj::<GetCapset>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
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

                return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
            }
        };

        let resp_hdr = Gpu::resp_header(CtrlType::RespOkCapset, hdr);
        ChainAction::Complete(chain.write_parts(&[resp_hdr.as_bytes(), &data], mem))
    }

    fn cmd_ctx_create(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(req) = chain.read_obj::<CtxCreate>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let nlen = (req.nlen as usize).min(req.debug_name.len());
        let name = std::str::from_utf8(&req.debug_name[..nlen]).unwrap_or("ctx");

        match self.renderer.create_ctx(hdr.ctx_ud, req.context_init, Some(name)) {
            Ok(()) => Gpu::ok(chain, hdr, mem),
            Err(e) => {
                eprintln!(
                    "CtxCreate FAILED ctx_id={} context_init={} err={:?}",
                    pf!(hdr, ctx_ud),
                    pf!(req, context_init),
                    e,
                );

                Gpu::err(chain, CtrlType::RespErrInvalidContextId, hdr, mem)
            }
        }
    }

    fn cmd_ctx_destroy(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        self.renderer.destroy_ctx(hdr.ctx_ud).unwrap();
        Gpu::ok(chain, hdr, mem)
    }

    fn cmd_ctx_attach_resource(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(req) = chain.read_obj::<CtxResource>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = req.resource_id;

        let Some(resource) = self.resources.get(&resource_id) else {
            eprintln!(
                "CtxAttach FAILED: resource not in local map ctx_id={} resource_id={}",
                pf!(hdr, ctx_ud),
                resource_id,
            );
            return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        if matches!(&resource.kind, ResourceKind::Local2d) {
            eprintln!(
                "CtxAttach: ignoring attach of local 2D resource {} to ctx {}",
                resource_id,
                pf!(hdr, ctx_ud),
            );
            return Gpu::ok(chain, hdr, mem);
        }

        let ctx_ud = hdr.ctx_ud;
        match self.renderer.ctx_attach_resource(hdr.ctx_ud, resource_id) {
            Ok(()) => Gpu::ok(chain, hdr, mem),
            Err(e) => {
                eprintln!(
                    "CtxAttach FAILED ctx_id={} resource_id={} err={:?}",
                    ctx_ud, resource_id, e,
                );
                Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem)
            }
        }
    }

    fn cmd_ctx_detach_resource(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(req) = chain.read_obj::<CtxResource>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = req.resource_id;

        let Some(resource) = self.resources.get(&resource_id) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        if matches!(&resource.kind, ResourceKind::Local2d) {
            eprintln!(
                "CtxDetach: ignoring detach of local 2D resource {} from ctx {}",
                resource_id,
                pf!(hdr, ctx_ud),
            );

            return Gpu::ok(chain, hdr, mem);
        }

        match self.renderer.ctx_detach_resource(hdr.ctx_ud, resource_id) {
            Ok(()) => Gpu::ok(chain, hdr, mem),
            Err(e) => {
                eprintln!(
                    "CtxDetach FAILED ctx_id={} resource_id={} err={:?}",
                    pf!(hdr, ctx_ud),
                    resource_id,
                    e,
                );
                Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem)
            }
        }
    }

    fn cmd_resource_create_3d(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(req) = chain.read_obj::<ResourceCreate3d>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = req.resource_id;
        if self.resources.contains_key(&resource_id) {
            return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        }

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
            return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
        }

        self.resources.insert(
            req.resource_id,
            Resource {
                format: req.format,
                width: req.width,
                height: req.height,
                backing: Vec::new(),
                framebuffer: Vec::new(),
                kind: ResourceKind::Renderer3d,
            },
        );

        Gpu::ok(chain, hdr, mem)
    }

    fn cmd_transfer_to_host_3d(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(req) = chain.read_obj::<TransferHost3d>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
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

        Gpu::ok(chain, hdr, mem)
    }

    fn cmd_transfer_from_host_3d(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(req) = chain.read_obj::<TransferHost3d>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
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

        Gpu::ok(chain, hdr, mem)
    }

    fn cmd_submit_3d(
        &mut self,
        chain: &ChainData,
        hdr: &CtrlHeader,
        token: ChainToken,
        mem: &GuestMemory,
    ) -> ChainAction {
        let Some(req) = chain.read_obj::<CmdSubmit3d>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
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
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        self.submit_buf.resize(req.size as usize, 0);

        if chain.read_at(payload_offset, &mut self.submit_buf, mem).is_none() {
            eprintln!(
                "Submit3d FAILED: cannot read payload ctx_id={} size={}",
                pf!(hdr, ctx_ud),
                pf!(req, size),
            );
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
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
            return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
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
                return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
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

        Gpu::ok(chain, hdr, mem)
    }

    fn cmd_resource_flush(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(val) = chain.read_obj::<ResourceFlush>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = val.resource_id;
        let flush_rect = val.r;

        if self.scanout.as_ref().map(|scanout| scanout.resource_id) != Some(resource_id) {
            return Gpu::ok(chain, hdr, mem);
        }

        let Some(resource) = self.resources.get(&resource_id) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        let native_texture = match &resource.kind {
            ResourceKind::Local2d => None,
            ResourceKind::RendererBlob(_) if !resource.backing.is_empty() => {
                if !self.readback_blob_scanout(resource_id, flush_rect, mem) {
                    return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
                }

                None
            }
            ResourceKind::Renderer3d | ResourceKind::RendererBlob(_) => {
                let Some(texture) = self.renderer.native_texture(resource_id) else {
                    eprintln!(
                        "virtio-gpu: ResourceFlush failed: 3D scanout resource={} has no native texture",
                        resource_id
                    );
                    return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
                };

                Some(texture)
            }
        };

        if !self.present_scanout(resource_id, flush_rect, native_texture) {
            eprintln!(
                "virtio-gpu: ResourceFlush failed: native scanout copy failed resource={}",
                resource_id
            );
            return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
        }

        Gpu::ok(chain, hdr, mem)
    }

    fn cmd_resource_create_blob(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(req) = chain.read_obj::<ResourceCreateBlob>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = req.resource_id;
        if self.resources.contains_key(&resource_id) {
            return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        }

        let mut iovecs = Vec::with_capacity(req.nr_entries as usize);
        let mut backing = Vec::with_capacity(req.nr_entries as usize);

        if req.nr_entries > 0 {
            let entries_base = size_of::<ResourceCreateBlob>();
            let entry_size = size_of::<MemEntry>();
            if entries_base + req.nr_entries as usize * entry_size > chain.readable_len() {
                return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
            }
            for i in 0..req.nr_entries as usize {
                let Some(e) = chain.read_obj::<MemEntry>(entries_base + i * entry_size, mem) else {
                    return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
                };
                let Some(base) = gpa_to_host(mem, e.addr, e.length as u64) else {
                    return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
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
            return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
        }

        self.resources.insert(
            req.resource_id,
            Resource {
                format: 0,
                width: 0,
                height: 0,
                backing,
                framebuffer: Vec::new(),
                kind: ResourceKind::RendererBlob(BlobResource {
                    size: req.size,
                    mapping: None,
                }),
            },
        );

        Gpu::ok(chain, hdr, mem)
    }

    fn cmd_resource_create_2d(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(val) = chain.read_obj::<ResourceCreate2d>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = val.resource_id;
        if self.resources.contains_key(&resource_id) {
            return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        }

        if val.format != 2 {
            eprintln!("virtio-gpu: unsupported format: {}", { val.format });
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        if val.width == 0 || val.height == 0 || val.width > 16384 || val.height > 16384 {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        self.resources.insert(
            val.resource_id,
            Resource {
                format: val.format,
                width: val.width,
                height: val.height,
                backing: Vec::new(),
                framebuffer: Vec::new(),
                kind: ResourceKind::Local2d,
            },
        );

        Gpu::ok(chain, hdr, mem)
    }

    fn cmd_resource_unref(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(val) = chain.read_obj::<ResourceUnref>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = val.resource_id;
        if !self.resources.contains_key(&resource_id) {
            return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        }

        self.destroy_resource(val.resource_id);

        Gpu::ok(chain, hdr, mem)
    }

    fn cmd_resource_attach_backing(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(val) = chain.read_obj::<ResourceAttachBacking>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let n_entries = val.nr_entries as usize;
        let entries_base = size_of::<ResourceAttachBacking>();
        let entry_size = size_of::<MemEntry>();

        if entries_base + n_entries * entry_size > chain.readable_len() {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        let resource_id = val.resource_id;
        let Some(resource) = self.resources.get_mut(&resource_id) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        if !resource.backing.is_empty() {
            return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
        }

        if n_entries == 0 {
            return Gpu::ok(chain, hdr, mem);
        }

        let mut backing = Vec::with_capacity(n_entries);
        for i in 0..n_entries {
            let Some(entry) = chain.read_obj::<MemEntry>(entries_base + i * entry_size, mem) else {
                return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
            };
            backing.push(entry);
        }

        if matches!(&resource.kind, ResourceKind::Renderer3d | ResourceKind::RendererBlob(_)) {
            let mut renderer_backing = Vec::with_capacity(backing.len());
            for entry in &backing {
                let Some(iov_base) = gpa_to_host(mem, entry.addr, entry.length as u64) else {
                    return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
                };
                renderer_backing.push(Iovec {
                    iov_base,
                    iov_len: entry.length as usize,
                });
            }

            if self.renderer.attach_backing(resource_id, renderer_backing).is_err() {
                return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
            }
        }

        resource.backing = backing;

        Gpu::ok(chain, hdr, mem)
    }

    fn cmd_resource_detach_backing(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(val) = chain.read_obj::<ResourceDetachBacking>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = val.resource_id;
        let Some(resource) = self.resources.get_mut(&resource_id) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        if resource.backing.is_empty() {
            return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
        }

        if matches!(&resource.kind, ResourceKind::Renderer3d | ResourceKind::RendererBlob(_)) {
            self.renderer.detach_backing(resource_id);
        }

        resource.backing.clear();

        Gpu::ok(chain, hdr, mem)
    }

    fn cmd_set_scanout(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(val) = chain.read_obj::<SetScanout>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = val.resource_id;

        if resource_id == 0 {
            self.scanout = None;
        } else {
            if !self.resources.contains_key(&resource_id) {
                return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
            }

            self.scanout = Some(Scanout {
                resource_id,
                source: ScanoutSource::Resource { rect: val.r },
            });
            self.configure_presentation();
        }

        Gpu::ok(chain, hdr, mem)
    }

    fn cmd_transfer_to_host_2d(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(val) = chain.read_obj::<TransferToHost2d>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = val.resource_id;
        let Some(resource) = self.resources.get_mut(&resource_id) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        if matches!(&resource.kind, ResourceKind::Renderer3d | ResourceKind::RendererBlob(_)) {
            return Gpu::ok(chain, hdr, mem);
        }

        let width = resource.width as usize;
        let height = resource.height as usize;
        let stride = width * BYTES_PER_PIXEL;

        let rect_x = val.r.x as usize;
        let rect_y = val.r.y as usize;
        let rect_width = val.r.width as usize;
        let rect_height = val.r.height as usize;
        let transfer_offset = val.offset as usize;

        if rect_x + rect_width > width || rect_y + rect_height > height {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
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
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        Gpu::ok(chain, hdr, mem)
    }

    fn cmd_set_scanout_blob(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(val) = chain.read_obj::<SetScanoutBlobReq>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = val.resource_id;
        let scanout_id = val.scanout_id;
        let width = val.width;
        let height = val.height;

        if scanout_id != 0 {
            return Gpu::err(chain, CtrlType::RespErrInvalidScanoutId, hdr, mem);
        }

        if resource_id == 0 {
            self.scanout = None;
            return Gpu::ok(chain, hdr, mem);
        }

        if width == 0 || height == 0 || width > 16384 || height > 16384 {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        if !self.resources.contains_key(&resource_id) {
            return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        }

        self.scanout = Some(Scanout {
            resource_id,
            source: ScanoutSource::Blob {
                width,
                height,
                stride: if val.strides[0] != 0 {
                    val.strides[0]
                } else {
                    width.saturating_mul(BYTES_PER_PIXEL as u32)
                },
                offset: val.offsets[0] as u64,
            },
        });
        self.configure_presentation();

        Gpu::ok(chain, hdr, mem)
    }

    fn readback_blob_scanout(&mut self, resource_id: u32, rect: Rect, mem: &GuestMemory) -> bool {
        let Some(Scanout {
            resource_id: scanout_resource_id,
            source:
                ScanoutSource::Blob {
                    width,
                    height,
                    stride,
                    offset,
                },
            ..
        }) = self.scanout.as_ref()
        else {
            return false;
        };

        if *scanout_resource_id != resource_id {
            return false;
        }

        let width = *width as usize;
        let height = *height as usize;
        let src_stride = *stride as usize;
        let base = *offset as usize;

        let Some(resource) = self.resources.get_mut(&resource_id) else {
            return false;
        };

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

    fn configure_presentation(&mut self) {
        let Some(scanout) = self.scanout.as_ref() else {
            return;
        };

        let (width, height) = match &scanout.source {
            ScanoutSource::Resource { rect } => (rect.width, rect.height),
            ScanoutSource::Blob { width, height, .. } => (*width, *height),
        };

        (self.on_present)(Presentation::Configure { width, height });
    }

    fn present_scanout(&mut self, resource_id: u32, damage: Rect, native_texture: Option<NativeTexture>) -> bool {
        let Some(scanout) = self.scanout.as_ref() else {
            return true;
        };

        if scanout.resource_id != resource_id {
            return true;
        }

        let (source_rect, blob_size) = match &scanout.source {
            ScanoutSource::Resource { rect } => (
                PresentRect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                },
                None,
            ),
            ScanoutSource::Blob { width, height, .. } => (
                PresentRect {
                    x: 0,
                    y: 0,
                    width: *width,
                    height: *height,
                },
                Some((*width, *height)),
            ),
        };

        let damage = PresentRect {
            x: damage.x,
            y: damage.y,
            width: damage.width,
            height: damage.height,
        };

        if let Some(texture) = native_texture {
            return (self.on_present)(Presentation::Frame(PresentFrame {
                source: PresentSource::NativeTexture(texture),
                source_rect,
                damage,
            }));
        }

        let resources = &self.resources;
        let on_present = &mut self.on_present;
        let Some(resource) = resources.get(&resource_id) else {
            return false;
        };

        let (width, height, stride) = blob_size
            .map(|(width, height)| (width, height, width.saturating_mul(BYTES_PER_PIXEL as u32)))
            .unwrap_or((
                resource.width,
                resource.height,
                resource.width.saturating_mul(BYTES_PER_PIXEL as u32),
            ));

        on_present(Presentation::Frame(PresentFrame {
            source: PresentSource::Pixels {
                data: &resource.framebuffer,
                width,
                height,
                stride,
            },
            source_rect,
            damage,
        }))
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

    fn write_response(chain: &ChainData, r#type: CtrlType, req: &CtrlHeader, mem: &GuestMemory) -> u32 {
        chain.write_response(Gpu::resp_header(r#type, req).as_bytes(), mem)
    }

    fn read_backing(backing: &[MemEntry], mut src_offset: usize, dst: &mut [u8], mem: &GuestMemory) -> Option<()> {
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

    fn resource_map_blob(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(req) = chain.read_obj::<ResourceMapBlob>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = req.resource_id;
        let offset = req.offset;

        let Some(resource) = self.resources.get_mut(&resource_id) else {
            eprintln!("ResourceMapBlob FAILED: unknown resource_id={}", resource_id,);
            return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        let ResourceKind::RendererBlob(blob) = &mut resource.kind else {
            eprintln!("ResourceMapBlob FAILED: resource_id={} is not a blob", resource_id);
            return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        let size = blob.size;
        if size == 0 {
            eprintln!("ResourceMapBlob FAILED: resource_id={} has zero blob size", resource_id);
            return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        }

        if blob.mapping.is_some() {
            eprintln!("ResourceMapBlob FAILED: resource_id={} is already mapped", resource_id);
            return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
        }

        let align = APPLE_HV_PAGE_SIZE as u64;

        if offset & (align - 1) != 0 {
            eprintln!(
                "ResourceMapBlob FAILED: unaligned offset resource_id={} offset={:#x} align={:#x}",
                resource_id, offset, align,
            );
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        let rounded_size = match align_up(size as usize, APPLE_HV_PAGE_SIZE) {
            Some(v) => v as u64,
            None => {
                eprintln!(
                    "ResourceMapBlob FAILED: size overflow resource_id={} size={}",
                    resource_id, size,
                );
                return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
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
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        }

        let map_info = match self.renderer.map_info(resource_id) {
            Ok(map_info) => map_info,
            Err(err) => {
                eprintln!(
                    "ResourceMapBlob FAILED: map_info resource_id={} err={:?}",
                    resource_id, err,
                );
                return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
            }
        };

        let map_ptr = match self.renderer.map_ptr(resource_id) {
            Ok(map_ptr) => map_ptr,
            Err(err) => {
                eprintln!(
                    "ResourceMapBlob FAILED: map_ptr resource_id={} err={:?}",
                    resource_id, err,
                );
                return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
            }
        };

        let guest_addr = HOST_VISIBLE_SHM_BASE + offset;

        let mapping = match BlobMapping::map(map_ptr, guest_addr, size as usize) {
            Ok(mapping) => mapping,
            Err(err) => {
                eprintln!(
                    "ResourceMapBlob FAILED: hv_vm_map resource_id={} map_ptr={:#x} guest_addr={:#x} size={} err={:#x}",
                    resource_id, map_ptr, guest_addr, size, err,
                );

                if let Err(unmap_err) = self.renderer.unmap(resource_id) {
                    eprintln!(
                        "ResourceMapBlob cleanup FAILED: virgl unmap resource_id={} err={:?}",
                        resource_id, unmap_err,
                    );
                }

                return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
            }
        };

        blob.mapping = Some(mapping);

        let resp = RespMapInfo {
            hdr: Gpu::resp_header(CtrlType::RespOkMapInfo, hdr),
            map_info: map_info & MAP_CACHE_MASK,
            padding: 0,
        };

        ChainAction::Complete(chain.write_response(resp.as_bytes(), mem))
    }

    fn resource_unmap_blob(&mut self, chain: &ChainData, hdr: &CtrlHeader, mem: &GuestMemory) -> ChainAction {
        let Some(req) = chain.read_obj::<ResourceUnmapBlob>(0, mem) else {
            return Gpu::err(chain, CtrlType::RespErrInvalidParameter, hdr, mem);
        };

        let resource_id = req.resource_id;

        let Some(resource) = self.resources.get_mut(&resource_id) else {
            eprintln!("ResourceUnmapBlob FAILED: unknown resource_id={}", resource_id);
            return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        let ResourceKind::RendererBlob(blob) = &mut resource.kind else {
            eprintln!("ResourceUnmapBlob FAILED: resource_id={} is not a blob", resource_id);
            return Gpu::err(chain, CtrlType::RespErrInvalidResourceId, hdr, mem);
        };

        let Some(mut mapping) = blob.mapping.take() else {
            eprintln!("ResourceUnmapBlob FAILED: resource_id={} is not mapped", resource_id);
            return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
        };

        if let Err(err) = mapping.unmap() {
            eprintln!(
                "ResourceUnmapBlob FAILED: hv_vm_unmap resource_id={} guest_addr={:#x} size={} err={:#x}",
                resource_id, mapping.guest_addr, mapping.size, err,
            );
            blob.mapping = Some(mapping);
            return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
        }

        if let Err(err) = self.renderer.unmap(resource_id) {
            eprintln!(
                "ResourceUnmapBlob FAILED: virgl unmap resource_id={} err={:?}",
                resource_id, err,
            );
            return Gpu::err(chain, CtrlType::RespErrUnspec, hdr, mem);
        }

        Gpu::ok(chain, hdr, mem)
    }

    fn destroy_resource(&mut self, resource_id: u32) {
        let Some(resource) = self.resources.remove(&resource_id) else {
            return;
        };

        if self.scanout.as_ref().is_some_and(|s| s.resource_id == resource_id) {
            self.scanout = None;
        }

        match resource.kind {
            ResourceKind::Local2d => {}
            ResourceKind::Renderer3d => self.renderer.resource_unref(resource_id),
            ResourceKind::RendererBlob(mut blob) => {
                if let Some(mapping) = blob.mapping.take() {
                    drop(mapping);

                    if let Err(err) = self.renderer.unmap(resource_id) {
                        eprintln!(
                            "destroy_resource: virgl unmap resource_id={} err={:?}",
                            resource_id, err
                        );
                    }
                }

                self.renderer.resource_unref(resource_id);
            }
        }
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

    fn on_event(&mut self, event: ExternalEvent, ctx: &mut DeviceContext<'_>) {
        match event {
            ExternalEvent::DisplayResized { width, height } => {
                if width == 0 || height == 0 {
                    return;
                }

                self.edid_compatibility_mode.get_or_insert((width, height));
                self.display_width = width;
                self.display_height = height;
                self.events_read |= EVENT_DISPLAY;
                ctx.config_changed();
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

                        ctx.complete(p.token, p.written);
                    } else {
                        i += 1;
                    }
                }
            }
        }
    }
}

fn gpa_to_host(mem: &GuestMemory, gpa: u64, len: u64) -> Option<*mut c_void> {
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
