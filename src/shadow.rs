//! Small, bounded shadow textures. Movement/scaling only transforms the cached
//! texture; blur is three separable box passes, linear in the texture area.
use crate::{geometry::Point, render::rounded_box_path, style::ShadowStyle};
use std::collections::VecDeque;
use tiny_skia::{BlendMode, FillRule, FilterQuality, Paint, Pixmap, PixmapPaint, Transform};

const MAX_TEXTURE_EDGE: u32 = 512;
const CACHE_BYTES: usize = 2 * 1024 * 1024;

struct Texture {
    key: (u64, u64),
    pixmap: Pixmap,
    extent: f64,
}

#[derive(Default)]
pub(crate) struct Shadows {
    style: Option<ShadowStyle>,
    textures: VecDeque<Texture>,
    bytes: usize,
}

impl Shadows {
    pub fn enabled(&self) -> bool {
        self.style.is_some()
    }

    pub fn configure(&mut self, style: Option<ShadowStyle>) {
        self.style = style;
        self.textures.clear();
        self.bytes = 0;
    }

    pub fn draw(
        &mut self,
        target: &mut Pixmap,
        center: Point,
        size: f64,
        reference_size: f64,
        reference_radius: f64,
        opacity: f64,
    ) {
        let Some(style) = self.style else {
            return;
        };
        if size <= 0.0 || reference_size <= 0.0 || !reference_size.is_finite() || opacity <= 0.0 {
            return;
        }
        let key = (reference_size.to_bits(), reference_radius.to_bits());
        if let Some(index) = self.textures.iter().position(|texture| texture.key == key) {
            let texture = self.textures.remove(index).unwrap();
            self.textures.push_back(texture);
        } else {
            let Some(texture) = make_texture(key, reference_size, reference_radius, style) else {
                return;
            };
            let bytes = texture.pixmap.data().len();
            while self.bytes + bytes > CACHE_BYTES || self.textures.len() >= 16 {
                let Some(old) = self.textures.pop_front() else {
                    break;
                };
                self.bytes -= old.pixmap.data().len();
            }
            self.bytes += bytes;
            self.textures.push_back(texture);
        }
        let texture = self.textures.back().unwrap();
        let extent = texture.extent * size / reference_size;
        let scale = (extent / texture.pixmap.width() as f64) as f32;
        target.draw_pixmap(
            0,
            0,
            texture.pixmap.as_ref(),
            &PixmapPaint {
                opacity: (opacity * style.opacity).clamp(0.0, 1.0) as f32,
                quality: FilterQuality::Bilinear,
                ..PixmapPaint::default()
            },
            Transform::from_row(
                scale,
                0.0,
                0.0,
                scale,
                (center.x - extent / 2.0) as f32,
                (center.y - extent / 2.0) as f32,
            ),
            None,
        );
    }
}

fn make_texture(key: (u64, u64), size: f64, radius: f64, style: ShadowStyle) -> Option<Texture> {
    let shadow_size = size + 2.0 * style.spread;
    if shadow_size <= 0.0 {
        return None;
    }
    // Three box passes can extend 3 * ceil(sigma) texels from the silhouette.
    let padding = 1.5 * style.blur_radius
        + style.spread.max(0.0)
        + style.offset_x.abs().max(style.offset_y.abs())
        + 4.0;
    let extent = size + padding * 2.0;
    if !extent.is_finite() || extent <= 0.0 {
        return None;
    }
    let content_edge = ((extent * 2.0).ceil() as u32).clamp(1, MAX_TEXTURE_EDGE - 6);
    let edge = content_edge + 6;
    let scale = content_edge as f64 / extent;
    let extent = edge as f64 / scale;
    let center = Point {
        x: edge as f64 / 2.0,
        y: edge as f64 / 2.0,
    };
    let mut pixmap = Pixmap::new(edge, edge)?;
    let path = rounded_box_path(
        Point {
            x: center.x + style.offset_x * scale,
            y: center.y + style.offset_y * scale,
        },
        shadow_size * scale,
        (radius + style.spread).clamp(0.0, shadow_size / 2.0) * scale,
    )?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    let mut alpha: Vec<u8> = pixmap.pixels().iter().map(|p| p.alpha()).collect();
    let blur = (style.blur_radius * scale / 2.0).round() as usize;
    if blur > 0 {
        let mut scratch = vec![0; alpha.len()];
        for _ in 0..3 {
            box_pass(&alpha, &mut scratch, edge as usize, blur, false);
            box_pass(&scratch, &mut alpha, edge as usize, blur, true);
        }
    }
    for (pixel, alpha) in pixmap.pixels_mut().iter_mut().zip(alpha) {
        let a = (alpha as f32 * style.color.alpha).round() as u8;
        *pixel = tiny_skia::PremultipliedColorU8::from_rgba(
            (style.color.red * a as f32).round() as u8,
            (style.color.green * a as f32).round() as u8,
            (style.color.blue * a as f32).round() as u8,
            a,
        )?;
    }
    // A translucent circle must not expose a black shadow underneath itself.
    paint.blend_mode = BlendMode::DestinationOut;
    let interior = rounded_box_path(center, size * scale, radius * scale)?;
    pixmap.fill_path(
        &interior,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    Some(Texture {
        key,
        pixmap,
        extent,
    })
}

fn box_pass(source: &[u8], target: &mut [u8], edge: usize, radius: usize, vertical: bool) {
    let diameter = (2 * radius + 1) as u64;
    for line in 0..edge {
        let index = |position| {
            if vertical {
                position * edge + line
            } else {
                line * edge + position
            }
        };
        let mut sum: u64 = (0..=radius.min(edge - 1))
            .map(|p| source[index(p)] as u64)
            .sum();
        for p in 0..edge {
            target[index(p)] = ((sum + diameter / 2) / diameter) as u8;
            if p >= radius {
                sum -= source[index(p - radius)] as u64;
            }
            if p + radius + 1 < edge {
                sum += source[index(p + radius + 1)] as u64;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "writes a PNG preview to the temporary directory"]
    fn visual_preview() {
        let mut canvas = Pixmap::new(640, 220).unwrap();
        canvas.fill(tiny_skia::Color::from_rgba8(215, 225, 232, 255));
        for (x, radius, offset, spread) in [
            (110.0, 48.0, 4.0, 0.0),
            (320.0, 28.0, -8.0, 4.0),
            (530.0, 0.0, 8.0, -4.0),
        ] {
            let mut shadows = Shadows::default();
            shadows.configure(Some(ShadowStyle {
                offset_x: offset,
                offset_y: offset,
                spread,
                ..ShadowStyle::default()
            }));
            let center = Point { x, y: 110.0 };
            shadows.draw(&mut canvas, center, 96.0, 96.0, radius, 1.0);
            let path = rounded_box_path(center, 96.0, radius).unwrap();
            let mut paint = Paint::default();
            paint.set_color_rgba8(255, 255, 255, 120);
            canvas.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        let path =
            std::env::temp_dir().join(format!("waypie-shadow-preview-{}.png", std::process::id()));
        canvas.save_png(&path).unwrap();
        eprintln!("{}", path.display());
    }
    #[test]
    fn box_filter_matches_zero_padded_reference() {
        let source = (0..49).map(|i| (i * 5) as u8).collect::<Vec<_>>();
        for radius in [0, 1, 3, 12] {
            for vertical in [false, true] {
                let mut target = vec![0; 49];
                box_pass(&source, &mut target, 7, radius, vertical);
                for y in 0..7 {
                    for x in 0..7 {
                        let mut sum = 0_u64;
                        for delta in -(radius as i32)..=radius as i32 {
                            let (sx, sy) = if vertical {
                                (x, y + delta)
                            } else {
                                (x + delta, y)
                            };
                            if (0..7).contains(&sx) && (0..7).contains(&sy) {
                                sum += source[(sy * 7 + sx) as usize] as u64;
                            }
                        }
                        let diameter = (2 * radius + 1) as u64;
                        assert_eq!(
                            target[(y * 7 + x) as usize],
                            ((sum + diameter / 2) / diameter) as u8
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn cache_is_bounded_and_animation_reuses_texture() {
        let mut shadows = Shadows::default();
        let mut target = Pixmap::new(64, 64).unwrap();
        let center = Point { x: 32.0, y: 32.0 };
        shadows.draw(&mut target, center, 40.0, 40.0, 20.0, 1.0);
        assert_eq!(shadows.bytes, 0);
        shadows.configure(Some(ShadowStyle::default()));
        for size in 1..100 {
            shadows.draw(&mut target, center, size as f64, 40.0, 20.0, 1.0);
        }
        assert_eq!(shadows.textures.len(), 1);
        assert_eq!(
            shadows.textures[0]
                .pixmap
                .pixel(
                    shadows.textures[0].pixmap.width() / 2,
                    shadows.textures[0].pixmap.height() / 2
                )
                .unwrap()
                .alpha(),
            0
        );
        for size in 100..120 {
            shadows.draw(&mut target, center, size as f64, size as f64, 20.0, 1.0);
        }
        assert!(shadows.bytes <= CACHE_BYTES);
    }
}
