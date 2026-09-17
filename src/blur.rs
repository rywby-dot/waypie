//! Native background blur: regions only, no desktop capture or CPU blur.
use crate::{app::App, geometry::Point};
use smithay_client_toolkit::compositor::CompositorState;
use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum, globals::GlobalList, protocol::wl_surface::WlSurface,
};
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1::{self as manager, ExtBackgroundEffectManagerV1},
    ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
};

#[derive(Clone, Copy)]
pub(crate) struct BlurShape {
    pub center: Point,
    pub size: f64,
    pub radius: f64,
}

#[derive(Default)]
pub(crate) struct BackgroundBlur {
    manager: Option<ExtBackgroundEffectManagerV1>,
    supported: bool,
    effect: Option<(WlSurface, ExtBackgroundEffectSurfaceV1)>,
    previous: Vec<[i32; 4]>,
    scratch: Vec<[i32; 4]>,
}

impl BackgroundBlur {
    pub fn bind(&mut self, globals: &GlobalList, qh: &QueueHandle<App>) {
        self.manager = globals.bind(qh, 1..=1, ()).ok();
    }

    pub fn available(&self) -> bool {
        self.supported && self.manager.is_some()
    }

    pub fn clear(&mut self) {
        if let Some((_, effect)) = self.effect.take() {
            effect.destroy();
        }
        self.previous.clear();
        self.scratch.clear();
    }

    pub fn apply(
        &mut self,
        surface: &WlSurface,
        compositor: &CompositorState,
        qh: &QueueHandle<App>,
        shapes: &[BlurShape],
        width: u32,
        height: u32,
    ) {
        if self.effect.as_ref().is_some_and(|(old, _)| old != surface) {
            self.clear();
        }
        self.scratch.clear();
        if self.available() {
            for shape in shapes {
                append_region(&mut self.scratch, *shape, width, height);
            }
        }
        if self.scratch == self.previous {
            return;
        }
        if self.effect.is_none() {
            let Some(manager) = &self.manager else {
                return;
            };
            self.effect = Some((
                surface.clone(),
                manager.get_background_effect(surface, qh, ()),
            ));
        }
        let effect = &self.effect.as_ref().unwrap().1;
        if self.scratch.is_empty() {
            effect.set_blur_region(None);
        } else {
            let region = compositor.wl_compositor().create_region(qh, ());
            for &[x, y, w, h] in &self.scratch {
                region.add(x, y, w, h);
            }
            effect.set_blur_region(Some(&region));
            region.destroy();
        }
        std::mem::swap(&mut self.scratch, &mut self.previous);
    }
}

// Wayland regions contain integer rectangles, not alpha masks. Use conservative
// scanlines inside the rendered shape, merging equal adjacent spans. No region
// ever includes the full-screen transparent input layer or off-screen history.
fn append_region(rects: &mut Vec<[i32; 4]>, shape: BlurShape, width: u32, height: u32) {
    let BlurShape {
        center,
        size,
        radius,
    } = shape;
    if !size.is_finite() || size <= 0.0 || !center.x.is_finite() || !center.y.is_finite() {
        return;
    }
    let radius = radius.clamp(0.0, size / 2.0);
    let left = center.x - size / 2.0;
    let top = center.y - size / 2.0;
    let bottom = top + size;
    let first = top.ceil().clamp(0.0, height as f64) as i32;
    let end = bottom.floor().clamp(0.0, height as f64) as i32;
    let start = rects.len();
    for y in first..end {
        let edge = (y as f64 - top).min(bottom - (y + 1) as f64);
        let inset = if edge >= radius || radius == 0.0 {
            0.0
        } else if radius >= size / 2.0 - f64::EPSILON {
            radius - (radius * radius - (radius - edge).powi(2)).max(0.0).sqrt()
        } else {
            // Same quadratic corners as render::rounded_rect.
            radius * (1.0 - (edge / radius).max(0.0).sqrt()).powi(2)
        };
        let x = (left + inset).ceil().clamp(0.0, width as f64) as i32;
        let right = (left + size - inset).floor().clamp(0.0, width as f64) as i32;
        if right <= x {
            continue;
        }
        if rects.len() > start {
            let last = rects.last_mut().unwrap();
            if last[0] == x && last[2] == right - x && last[1] + last[3] == y {
                last[3] += 1;
                continue;
            }
        }
        rects.push([x, y, right - x, 1]);
    }
}

impl Dispatch<ExtBackgroundEffectManagerV1, ()> for App {
    fn event(
        app: &mut Self,
        _: &ExtBackgroundEffectManagerV1,
        event: manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let manager::Event::Capabilities { flags } = event {
            app.background_blur.supported =
                matches!(flags, WEnum::Value(value) if value.contains(manager::Capability::Blur));
            app.request_redraw();
        }
    }
}
wayland_client::delegate_noop!(App: ignore ExtBackgroundEffectSurfaceV1);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn square_merges_and_is_clipped_to_output() {
        let mut rects = vec![];
        append_region(
            &mut rects,
            BlurShape {
                center: Point { x: 0.0, y: 0.0 },
                size: 100.0,
                radius: 0.0,
            },
            30,
            40,
        );
        assert_eq!(rects, vec![[0, 0, 30, 40]]);
    }
    #[test]
    fn circle_region_never_extends_outside_circle() {
        let mut rects = vec![];
        append_region(
            &mut rects,
            BlurShape {
                center: Point { x: 50.25, y: 50.5 },
                size: 80.0,
                radius: 40.0,
            },
            100,
            100,
        );
        assert!(!rects.is_empty());
        for [x, y, w, h] in rects {
            for px in [x, x + w] {
                for py in [y, y + h] {
                    assert!((px as f64 - 50.25).hypot(py as f64 - 50.5) <= 40.000001);
                }
            }
        }
    }
    #[test]
    fn invisible_and_offscreen_shapes_have_no_region() {
        let mut rects = vec![];
        for size in [0.0, -1.0, f64::NAN, 20.0] {
            append_region(
                &mut rects,
                BlurShape {
                    center: Point {
                        x: -100.0,
                        y: -100.0,
                    },
                    size,
                    radius: 0.0,
                },
                100,
                100,
            );
        }
        assert!(rects.is_empty());
    }
}
