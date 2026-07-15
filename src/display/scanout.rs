use super::{DisplayBuffer, angle::AngleCopy, iosurface::ScopedIOSurface};
use crate::virtio::gpu::{PresentFrame, PresentSource, Presentation};
use std::sync::Mutex;
use zerocopy::IntoBytes;

pub struct ScanoutPublisher<'a> {
    display: &'a Mutex<DisplayBuffer>,
    angle_copy: AngleCopy,
    iosurface: Option<ScopedIOSurface>,
}

impl<'a> ScanoutPublisher<'a> {
    pub fn new(display: &'a Mutex<DisplayBuffer>) -> Self {
        Self {
            display,
            angle_copy: AngleCopy::new(),
            iosurface: None,
        }
    }

    pub fn present(&mut self, presentation: Presentation<'_>) -> bool {
        match presentation {
            Presentation::Configure { width, height } => {
                self.configure(width, height);
                true
            }
            Presentation::Frame(frame) => self.present_frame(frame),
            Presentation::Reset => {
                self.reset();
                true
            }
        }
    }

    fn configure(&mut self, width: u32, height: u32) {
        self.iosurface = None;
        self.display.lock().unwrap().resize(width as usize, height as usize);
    }

    fn reset(&mut self) {
        self.iosurface = None;
    }

    fn present_frame(&mut self, frame: PresentFrame<'_>) -> bool {
        let iosurface_id = match frame.source {
            PresentSource::Pixels { .. } => None,
            PresentSource::NativeTexture(texture) => {
                let recreate = self.iosurface.as_ref().is_none_or(|surface| {
                    surface.width() != frame.source_rect.width || surface.height() != frame.source_rect.height
                });

                if recreate {
                    self.iosurface = ScopedIOSurface::new_bgra(frame.source_rect.width, frame.source_rect.height);
                }

                let Some(surface) = self.iosurface.as_ref() else {
                    return false;
                };

                if !self.angle_copy.copy_native_texture_to_iosurface(
                    texture,
                    surface.as_ptr(),
                    (
                        frame.source_rect.x,
                        frame.source_rect.y,
                        frame.source_rect.width,
                        frame.source_rect.height,
                    ),
                ) {
                    eprintln!("virtio-gpu: ANGLE producer copy failed; falling back to readback");
                    return false;
                }

                Some(surface.id())
            }
        };

        self.publish(frame, iosurface_id);
        true
    }

    fn publish(&self, frame: PresentFrame<'_>, iosurface_id: Option<u32>) {
        let source_x = frame.source_rect.x as usize;
        let source_y = frame.source_rect.y as usize;
        let source_width = frame.source_rect.width as usize;
        let source_height = frame.source_rect.height as usize;

        let mut display = self.display.lock().unwrap();

        let flush_x0 = frame.damage.x as usize;
        let flush_y0 = frame.damage.y as usize;
        let flush_x1 = flush_x0.saturating_add(frame.damage.width as usize);
        let flush_y1 = flush_y0.saturating_add(frame.damage.height as usize);
        let source_x1 = source_x.saturating_add(source_width);
        let source_y1 = source_y.saturating_add(source_height);

        let src_x0 = flush_x0.max(source_x);
        let src_y0 = flush_y0.max(source_y);
        let src_x1 = flush_x1.min(source_x1);
        let src_y1 = flush_y1.min(source_y1);

        if src_x0 >= src_x1 || src_y0 >= src_y1 {
            return;
        }

        let x0 = (src_x0 - source_x).min(display.width);
        let y0 = (src_y0 - source_y).min(display.height);
        let x1 = (src_x1 - source_x).min(display.width);
        let y1 = (src_y1 - source_y).min(display.height);

        if x0 >= x1 || y0 >= y1 {
            return;
        }

        if let PresentSource::Pixels { data, stride, .. } = frame.source {
            let stride = stride as usize;
            let copy_src_x0 = source_x + x0;
            let copy_src_y0 = source_y + y0;
            let copy_src_x1 = source_x + x1;
            let copy_src_y1 = source_y + y1;

            if data.len() < copy_src_y1 * stride {
                return;
            }

            let dst_w = display.width;
            let dst = display.pixels.as_mut_slice();

            for (src_y, dst_y) in (copy_src_y0..copy_src_y1).zip(y0..y1) {
                let src_row = src_y * stride;
                let src = &data[src_row + copy_src_x0 * 4..src_row + copy_src_x1 * 4];
                let dst_row = &mut dst[dst_y * dst_w + x0..dst_y * dst_w + x1];
                dst_row.as_mut_bytes().copy_from_slice(src);
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
}
