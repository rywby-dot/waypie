use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use cosmic_text::{
    Align, Attrs, Buffer, Color as TextColor, Family, FontSystem, Metrics, Shaping, SwashCache,
    Wrap,
};
use image::ImageReader;
use tiny_skia::{
    Color as SkColor, FillRule, Mask, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke,
    Transform,
};

use crate::{
    config::{Config, Item, item_at_path},
    geometry::{Point, angular_distance, direction_angle, radial_position},
    style::{CircleStyle, Color, StyleSheet},
};

pub struct Scene<'a> {
    pub config: &'a Config,
    pub styles: &'a StyleSheet,
    pub path: &'a [usize],
    pub centers: &'a [Point],
    pub pointer: Option<Point>,
    pub hovered: Option<Target>,
    pub hover_origins: &'a HashMap<Target, f64>,
    pub hover_progress: f64,
    pub icon_root: &'a Path,
    pub item_reveal: f64,
    pub close_scale: f64,
    pub close_opacity: f64,
    pub action: Option<ActionFrame>,
}

#[derive(Clone, Copy)]
pub struct ActionFrame {
    pub index: usize,
    pub start: Point,
    pub target: Point,
    pub position_progress: f64,
    pub growth_progress: f64,
    pub opacity: f64,
    pub final_scale: f64,
}

struct IndicatorFrame<'a> {
    item: &'a Item,
    submenu_style: &'a CircleStyle,
    styles: &'a StyleSheet,
    active: bool,
    reveal: f64,
    opacity: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Center,
    Parent(usize),
    Item(usize),
}

pub struct Renderer {
    fonts: FontSystem,
    glyphs: SwashCache,
    icons: HashMap<(PathBuf, u32, [u8; 4]), Pixmap>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            fonts: FontSystem::new(),
            glyphs: SwashCache::new(),
            icons: HashMap::new(),
        }
    }

    pub fn clear_icons(&mut self) {
        self.icons.clear();
    }

    pub fn render(&mut self, pixmap: &mut Pixmap, scene: &Scene<'_>) {
        let overlay = scene.styles.circle(&["overlay"]).unwrap_or_default();
        pixmap.fill(to_skia(overlay.background_color, overlay.opacity));
        let current = item_at_path(&scene.config.menu, scene.path);
        if scene.centers.is_empty() {
            return;
        }
        self.draw_connectors(pixmap, scene);

        for depth in 0..scene.path.len() {
            let item = item_at_path(&scene.config.menu, &scene.path[..depth]);
            let target = Target::Parent(depth);
            let activity = activity(scene, target);
            let active = activity > 0.0;
            let style = animated_style(scene.styles, item, Role::History, activity);
            self.draw_item(
                pixmap,
                scene.centers[depth],
                item,
                &style,
                scene.styles,
                scene.icon_root,
                active && !scene.config.active_label_in_center,
                active && scene.config.active_label_in_center,
                true,
                scene.close_scale,
                scene.close_opacity,
            );
        }

        let center = *scene.centers.last().unwrap();
        let center_activity = activity(scene, Target::Center);
        let center_active = center_activity > 0.0;
        let center_style = animated_style(scene.styles, current, Role::Center, center_activity);
        let center_override = if scene.config.active_label_in_center {
            match scene.hovered {
                Some(Target::Item(index)) => {
                    current.items.get(index).map(|item| item.label.as_str())
                }
                Some(Target::Parent(_)) => Some(""),
                _ => None,
            }
        } else {
            None
        };
        self.draw_item_with_label(
            pixmap,
            center,
            current,
            &center_style,
            scene.styles,
            scene.icon_root,
            center_active && !scene.config.active_label_in_center,
            false,
            center_override,
            false,
            scene.close_scale,
            scene.close_opacity,
        );

        let pointer_angle = scene
            .pointer
            .filter(|_| matches!(scene.hovered, Some(Target::Item(_))))
            .map(|pointer| {
                direction_angle(Point {
                    x: pointer.x - center.x,
                    y: pointer.y - center.y,
                })
            });
        for (index, item) in current.items.iter().enumerate() {
            let target = Target::Item(index);
            let activity = activity(scene, target);
            let active = activity > 0.0;
            let style = animated_style(scene.styles, item, Role::Item, activity);
            let active_style = item_style(scene.styles, item, Role::Item, true);
            let factor = if activity > 0.0 {
                activity
            } else if let Some(pointer_angle) = pointer_angle {
                let difference = angular_distance(pointer_angle, item.angle.unwrap_or(0.0));
                active_style.follow_distance * (1.0 + difference.to_radians().cos()) / 2.0
            } else {
                0.0
            };
            let distance =
                (scene.config.menu_radius + active_style.distance.unwrap_or(0.0) * factor).max(0.0);
            let position = radial_position(center, item.angle.unwrap_or(0.0), distance);
            let mut position = center.lerp(position, scene.item_reveal);
            let mut geometry_scale = scene.item_reveal * scene.close_scale;
            let mut opacity = scene.close_opacity;
            if let Some(action) = scene.action.filter(|action| action.index == index) {
                position = action.start.lerp(action.target, action.position_progress);
                geometry_scale =
                    scene.item_reveal * (1.0 + (action.final_scale - 1.0) * action.growth_progress);
                opacity = action.opacity;
            }
            let hide_label = scene.config.active_label_in_center && active && item.icon.is_some();
            self.draw_item(
                pixmap,
                position,
                item,
                &style,
                scene.styles,
                scene.icon_root,
                active && !scene.config.active_label_in_center,
                hide_label,
                true,
                geometry_scale,
                opacity,
            );
        }
    }

    fn draw_connectors(&mut self, pixmap: &mut Pixmap, scene: &Scene<'_>) {
        if scene.centers.len() < 2 {
            return;
        }
        let style = scene.styles.circle(&["connector"]).unwrap_or_default();
        let width = style.width.unwrap_or(0.0);
        if width <= 0.0 {
            return;
        }
        let mut paint = Paint::default();
        paint.set_color(to_skia(style.color, style.opacity * scene.close_opacity));
        let stroke = Stroke {
            width: width as f32,
            ..Stroke::default()
        };
        for (depth, pair) in scene.centers.windows(2).enumerate() {
            let start_item = item_at_path(&scene.config.menu, &scene.path[..depth]);
            let end_item = item_at_path(&scene.config.menu, &scene.path[..depth + 1]);
            let start_style = item_style(scene.styles, start_item, Role::History, false);
            let end_style = item_style(scene.styles, end_item, Role::Center, false);
            let start_radius =
                start_style.width.unwrap_or(0.0) * start_style.scale * scene.close_scale / 2.0;
            let end_radius =
                end_style.width.unwrap_or(0.0) * end_style.scale * scene.close_scale / 2.0;
            let delta = Point {
                x: pair[1].x - pair[0].x,
                y: pair[1].y - pair[0].y,
            };
            let length = delta.x.hypot(delta.y);
            if length <= start_radius + end_radius || length <= f64::EPSILON {
                continue;
            }
            let start = Point {
                x: pair[0].x + delta.x / length * start_radius,
                y: pair[0].y + delta.y / length * start_radius,
            };
            let end = Point {
                x: pair[1].x - delta.x / length * end_radius,
                y: pair[1].y - delta.y / length * end_radius,
            };
            let mut path = PathBuilder::new();
            path.move_to(start.x as f32, start.y as f32);
            path.line_to(end.x as f32, end.y as f32);
            if let Some(path) = path.finish() {
                pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_item(
        &mut self,
        pixmap: &mut Pixmap,
        center: Point,
        item: &Item,
        style: &CircleStyle,
        styles: &StyleSheet,
        icon_root: &Path,
        active: bool,
        hide_label: bool,
        indicators: bool,
        geometry_scale: f64,
        opacity: f64,
    ) {
        self.draw_item_with_label(
            pixmap,
            center,
            item,
            style,
            styles,
            icon_root,
            active,
            hide_label,
            None,
            indicators,
            geometry_scale,
            opacity,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_item_with_label(
        &mut self,
        pixmap: &mut Pixmap,
        center: Point,
        item: &Item,
        style: &CircleStyle,
        styles: &StyleSheet,
        icon_root: &Path,
        active: bool,
        hide_label: bool,
        label_override: Option<&str>,
        indicators: bool,
        geometry_scale: f64,
        opacity: f64,
    ) {
        let size = style.width.unwrap_or(0.0) * style.scale * geometry_scale;
        if size <= 0.0 {
            return;
        }
        if indicators && item.is_submenu() {
            self.draw_indicators(
                pixmap,
                center,
                size,
                IndicatorFrame {
                    item,
                    submenu_style: style,
                    styles,
                    active,
                    reveal: geometry_scale,
                    opacity,
                },
            );
        }
        let mut faded_style = style.clone();
        faded_style.opacity *= opacity;
        faded_style.text_opacity = Some(style.content_opacity() * opacity);
        draw_rounded_box(pixmap, center, size, &faded_style);

        let show_icon = item.icon.is_some() && label_override.is_none() && !active;
        let icon_drawn =
            show_icon && self.draw_icon(pixmap, center, size, item, &faded_style, icon_root);
        if icon_drawn || hide_label {
            return;
        }
        let label = label_override.unwrap_or(&item.label);
        if !label.is_empty() {
            self.draw_text(pixmap, center, size, label, &faded_style);
        }
    }

    fn draw_indicators(
        &mut self,
        pixmap: &mut Pixmap,
        center: Point,
        circle_size: f64,
        frame: IndicatorFrame<'_>,
    ) {
        let selectors = if frame.active {
            vec!["submenu-indicator", "submenu-indicator.active"]
        } else {
            vec!["submenu-indicator"]
        };
        let Ok(style) = frame.styles.circle(&selectors) else {
            return;
        };
        let size = style.width.unwrap_or(0.0) * frame.reveal;
        if size <= 0.0 || style.protrusion <= 0.0 {
            return;
        }
        let protrusion = style.protrusion * frame.reveal;
        let radius = (circle_size / 2.0 - size / 2.0 + protrusion).max(0.0);
        let mut paint = Paint::default();
        paint.set_color(to_skia(
            style.color,
            style.opacity * frame.opacity * frame.reveal,
        ));
        let clip = if style.cut_indicators {
            let rect = Rect::from_xywh(
                (center.x - circle_size / 2.0) as f32,
                (center.y - circle_size / 2.0) as f32,
                circle_size as f32,
                circle_size as f32,
            );
            rect.and_then(|rect| {
                let mut mask = Mask::new(pixmap.width(), pixmap.height())?;
                mask.fill_path(
                    &rounded_rect(rect, frame.submenu_style.radius(circle_size) as f32),
                    FillRule::Winding,
                    true,
                    Transform::identity(),
                );
                mask.invert();
                Some(mask)
            })
        } else {
            None
        };
        for child in &frame.item.items {
            let position = radial_position(center, child.angle.unwrap_or(0.0), radius);
            if let Some(path) =
                PathBuilder::from_circle(position.x as f32, position.y as f32, (size / 2.0) as f32)
            {
                pixmap.fill_path(
                    &path,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    clip.as_ref(),
                );
            }
        }
    }

    fn draw_text(
        &mut self,
        pixmap: &mut Pixmap,
        center: Point,
        size: f64,
        text: &str,
        style: &CircleStyle,
    ) {
        let available = ((size - style.border_width * 2.0) * style.text_fill).max(1.0) as f32;
        let font_size = style.font_size as f32;
        let line_height = (style.font_size * 1.15) as f32;
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(font_size, line_height));
        {
            let mut buffer = buffer.borrow_with(&mut self.fonts);
            buffer.set_size(Some(available), Some(available));
            buffer.set_wrap(Wrap::WordOrGlyph);
            let attrs = Attrs::new().family(Family::Name(&style.font_family));
            buffer.set_text(text, &attrs, Shaping::Advanced, Some(Align::Center));
            buffer.shape_until_scroll(true);
        }
        let color = style.color;
        let alpha = (color.alpha as f64 * style.content_opacity()).clamp(0.0, 1.0);
        let text_color = TextColor::rgba(
            (color.red * 255.0).round() as u8,
            (color.green * 255.0).round() as u8,
            (color.blue * 255.0).round() as u8,
            (alpha * 255.0).round() as u8,
        );
        let origin_x = (center.x - available as f64 / 2.0).round() as i32;
        let content_height = buffer
            .layout_runs()
            .map(|run| run.line_top + run.line_height)
            .fold(0.0_f32, f32::max)
            .min(available);
        let origin_y = (center.y - content_height as f64 / 2.0).round() as i32;
        let mut borrowed = buffer.borrow_with(&mut self.fonts);
        borrowed.draw(
            &mut self.glyphs,
            text_color,
            |x, y, width, height, color| {
                let Some(rect) = Rect::from_xywh(
                    (origin_x + x) as f32,
                    (origin_y + y) as f32,
                    width as f32,
                    height as f32,
                ) else {
                    return;
                };
                let mut paint = Paint::default();
                paint.set_color_rgba8(color.r(), color.g(), color.b(), color.a());
                pixmap.fill_rect(rect, &paint, Transform::identity(), None);
            },
        );
    }

    fn draw_icon(
        &mut self,
        pixmap: &mut Pixmap,
        center: Point,
        circle_size: f64,
        item: &Item,
        style: &CircleStyle,
        icon_root: &Path,
    ) -> bool {
        let (Some(theme), Some(icon)) = (&item.icon_theme, &item.icon) else {
            return false;
        };
        let path = icon_root.join(theme).join(icon);
        if !path.is_file() {
            return false;
        }
        let size = style.icon_fill.map_or_else(
            || {
                style.icon_size.map_or(circle_size * 0.55, |icon_size| {
                    icon_size * circle_size / style.width.unwrap_or(circle_size)
                })
            },
            |fill| (circle_size - style.border_width * 2.0).max(0.0) * fill,
        );
        let size = size.round().max(1.0) as u32;
        let rgba = [
            (style.color.red * 255.0) as u8,
            (style.color.green * 255.0) as u8,
            (style.color.blue * 255.0) as u8,
            255,
        ];
        let key = (path.clone(), size, rgba);
        if !self.icons.contains_key(&key) {
            let loaded = if path.extension().is_some_and(|extension| extension == "svg") {
                load_svg(&path, size, rgba)
            } else {
                load_raster(&path, size)
            };
            let Some(icon) = loaded else {
                return false;
            };
            self.icons.insert(key.clone(), icon);
        }
        let icon = &self.icons[&key];
        let x = (center.x - icon.width() as f64 / 2.0) as i32;
        let y = (center.y - icon.height() as f64 / 2.0) as i32;
        pixmap.draw_pixmap(
            x,
            y,
            icon.as_ref(),
            &PixmapPaint {
                opacity: style.content_opacity() as f32,
                ..PixmapPaint::default()
            },
            Transform::identity(),
            None,
        );
        true
    }
}

#[derive(Clone, Copy)]
enum Role {
    Item,
    Center,
    History,
}

fn item_style(styles: &StyleSheet, item: &Item, role: Role, active: bool) -> CircleStyle {
    let mut selectors = vec!["circle"];
    match role {
        Role::Item => {
            selectors.push("circle.item");
            if item.is_submenu() {
                selectors.push("circle.submenu");
            }
        }
        Role::Center => selectors.push("circle.center"),
        Role::History => selectors.push("circle.history"),
    }
    if active {
        selectors.push("circle.active");
        match role {
            Role::Item if item.is_submenu() => selectors.push("circle.submenu.active"),
            Role::Item => selectors.push("circle.item.active"),
            Role::Center => selectors.push("circle.center.active"),
            Role::History => selectors.push("circle.history.active"),
        }
    }
    styles.circle(&selectors).unwrap_or_default()
}

fn animated_style(styles: &StyleSheet, item: &Item, role: Role, activity: f64) -> CircleStyle {
    let resting = item_style(styles, item, role, false);
    if activity <= 0.0 {
        return resting;
    }
    let mut active = item_style(styles, item, role, true);
    active.scale = resting.scale + (active.scale - resting.scale) * activity;
    active
}

fn activity(scene: &Scene<'_>, target: Target) -> f64 {
    let origin = scene.hover_origins.get(&target).copied().unwrap_or(0.0);
    let destination = f64::from(scene.hovered == Some(target));
    origin + (destination - origin) * scene.hover_progress
}

fn draw_rounded_box(pixmap: &mut Pixmap, center: Point, size: f64, style: &CircleStyle) {
    let Some(rect) = Rect::from_xywh(
        (center.x - size / 2.0) as f32,
        (center.y - size / 2.0) as f32,
        size as f32,
        size as f32,
    ) else {
        return;
    };
    let radius = style.radius(size) as f32;
    let path = if radius >= size as f32 / 2.0 - f32::EPSILON {
        PathBuilder::from_circle(center.x as f32, center.y as f32, size as f32 / 2.0)
            .expect("a positive circle size")
    } else {
        rounded_rect(rect, radius)
    };
    let mut fill = Paint::default();
    fill.set_color(to_skia(style.background_color, style.opacity));
    pixmap.fill_path(&path, &fill, FillRule::Winding, Transform::identity(), None);
    if style.border_width > 0.0 {
        let mut border = Paint::default();
        border.set_color(to_skia(style.border_color, style.opacity));
        pixmap.stroke_path(
            &path,
            &border,
            &Stroke {
                width: style.border_width as f32,
                ..Stroke::default()
            },
            Transform::identity(),
            None,
        );
    }
}

fn rounded_rect(rect: Rect, radius: f32) -> tiny_skia::Path {
    let radius = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let mut path = PathBuilder::new();
    path.move_to(rect.left() + radius, rect.top());
    path.line_to(rect.right() - radius, rect.top());
    path.quad_to(rect.right(), rect.top(), rect.right(), rect.top() + radius);
    path.line_to(rect.right(), rect.bottom() - radius);
    path.quad_to(
        rect.right(),
        rect.bottom(),
        rect.right() - radius,
        rect.bottom(),
    );
    path.line_to(rect.left() + radius, rect.bottom());
    path.quad_to(
        rect.left(),
        rect.bottom(),
        rect.left(),
        rect.bottom() - radius,
    );
    path.line_to(rect.left(), rect.top() + radius);
    path.quad_to(rect.left(), rect.top(), rect.left() + radius, rect.top());
    path.close();
    path.finish().unwrap()
}

fn to_skia(color: Color, opacity: f64) -> SkColor {
    SkColor::from_rgba(
        color.red,
        color.green,
        color.blue,
        (color.alpha as f64 * opacity).clamp(0.0, 1.0) as f32,
    )
    .unwrap_or(SkColor::TRANSPARENT)
}

fn load_raster(path: &Path, size: u32) -> Option<Pixmap> {
    let image = ImageReader::open(path).ok()?.decode().ok()?;
    let mut data = image
        .resize(size, size, image::imageops::FilterType::Lanczos3)
        .to_rgba8()
        .into_raw();
    for pixel in data.chunks_exact_mut(4) {
        let alpha = pixel[3] as u16;
        pixel[0] = (pixel[0] as u16 * alpha / 255) as u8;
        pixel[1] = (pixel[1] as u16 * alpha / 255) as u8;
        pixel[2] = (pixel[2] as u16 * alpha / 255) as u8;
    }
    Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(size, size)?)
}

fn load_svg(path: &Path, size: u32, color: [u8; 4]) -> Option<Pixmap> {
    let mut source = fs::read_to_string(path).ok()?;
    let replacement = format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2]);
    if source.contains("currentColor") {
        source = source.replace("currentColor", &replacement);
    } else if !source.contains("fill=\"") && !source.contains("stroke=\"") {
        source = source.replacen("<svg", &format!("<svg fill=\"{replacement}\""), 1);
    }
    let tree =
        resvg::usvg::Tree::from_data(source.as_bytes(), &resvg::usvg::Options::default()).ok()?;
    let tree_size = tree.size();
    let scale = (size as f32 / tree_size.width()).min(size as f32 / tree_size.height());
    let mut pixmap = Pixmap::new(size, size)?;
    let transform = Transform::from_scale(scale, scale).post_translate(
        (size as f32 - tree_size.width() * scale) / 2.0,
        (size as f32 - tree_size.height() * scale) / 2.0,
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(pixmap)
}
