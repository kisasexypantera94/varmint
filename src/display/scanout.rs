use super::{DisplayBuffer, Rect, angle::AngleCopy, iosurface::ScopedIOSurface};
use crate::virtio::gpu::{PresentFrame, PresentSource, Presentation};
use std::sync::Mutex;

pub struct ScanoutPublisher<'a> {
    display: &'a Mutex<DisplayBuffer>,
    on_changed: &'a (dyn Fn() + Sync),
    angle_copy: AngleCopy,
    iosurface: Option<ScopedIOSurface>,
}

impl<'a> ScanoutPublisher<'a> {
    pub fn new(display: &'a Mutex<DisplayBuffer>, on_changed: &'a (dyn Fn() + Sync)) -> Self {
        Self {
            display,
            on_changed,
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
        }
    }

    fn configure(&mut self, width: u32, height: u32) {
        self.display.lock().unwrap().resize(width as usize, height as usize);
        (self.on_changed)();
    }

    fn present_frame(&mut self, frame: PresentFrame<'_>) -> bool {
        let source = Rect {
            x: frame.source_rect.x as usize,
            y: frame.source_rect.y as usize,
            width: frame.source_rect.width as usize,
            height: frame.source_rect.height as usize,
        };
        let damage = Rect {
            x: frame.damage.x as usize,
            y: frame.damage.y as usize,
            width: frame.damage.width as usize,
            height: frame.damage.height as usize,
        };

        let published = match frame.source {
            PresentSource::Pixels { data, stride, .. } => {
                self.display
                    .lock()
                    .unwrap()
                    .publish_pixels(data, stride as usize, source, damage)
            }
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

                let mut display = self.display.lock().unwrap();
                display.record_angle_copies(self.angle_copy.take_stats());
                display.publish_iosurface(surface.id(), source, damage)
            }
        };

        if published {
            (self.on_changed)();
        }

        true
    }
}
