use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
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
    geometry::{Point, radial_position},
    model::{MenuState, Target},
    style::{CircleStyle, Color, StyleSheet},
};

pub struct Scene<'a> {
    pub config: &'a Config,
    pub styles: &'a StyleSheet,
    pub state: &'a MenuState,
    pub icon_root: &'a Path,
}

struct IndicatorFrame<'a> {
    item: &'a Item,
    submenu_style: &'a CircleStyle,
    styles: &'a StyleSheet,
    active: bool,
}

struct ItemFrame<'a> {
    center: Point,
    item: &'a Item,
    style: &'a CircleStyle,
    styles: &'a StyleSheet,
    icon_root: &'a Path,
    active: bool,
    content: ItemContent<'a>,
    indicators: bool,
}

#[derive(Clone, Copy)]
enum ItemContent<'a> {
    Default,
    Label(&'a str),
    IconOrBlank,
}

pub struct Renderer {
    fonts: FontSystem,
    glyphs: SwashCache,
    icons: HashMap<(PathBuf, u32, [u8; 4]), Pixmap>,
    font_specs: HashMap<String, FontSpec>,
    configured_families: Vec<String>,
    resolved_families: HashMap<String, String>,
}

#[derive(Clone)]
struct FontSpec {
    family: String,
    file: PathBuf,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            fonts: empty_font_system(),
            glyphs: SwashCache::new(),
            icons: HashMap::new(),
            font_specs: HashMap::new(),
            configured_families: vec![],
            resolved_families: HashMap::new(),
        }
    }

    pub fn configure_fonts(&mut self, mut families: Vec<String>) {
        families.sort_unstable();
        families.dedup();
        if families == self.configured_families {
            return;
        }
        let mut database = cosmic_text::fontdb::Database::new();
        let mut resolved = HashMap::new();
        for requested in &families {
            let spec = self
                .font_specs
                .get(requested)
                .cloned()
                .or_else(|| resolve_font(requested));
            let Some(spec) = spec else {
                eprintln!("waypie: font-family {requested:?} was not found by fc-match");
                continue;
            };
            if database.load_font_file(&spec.file).is_ok() {
                resolved.insert(requested.clone(), spec.family.clone());
                self.font_specs.insert(requested.clone(), spec);
            }
        }
        if let Some(family) = resolved.get("Sans").or_else(|| resolved.values().next()) {
            database.set_sans_serif_family(family);
            database.set_serif_family(family);
            database.set_monospace_family(family);
        }
        self.fonts = FontSystem::new_with_locale_and_db(current_locale(), database);
        self.glyphs = SwashCache::new();
        self.configured_families = families;
        self.resolved_families = resolved;
    }

    pub fn clear_icons(&mut self) {
        self.icons.clear();
    }

    pub fn render(&mut self, pixmap: &mut Pixmap, scene: &Scene<'_>) {
        let overlay = scene.styles.circle(&["overlay"]).unwrap_or_default();
        pixmap.fill(to_skia(overlay.background_color, overlay.opacity));
        let path = scene.state.path();
        let centers = scene.state.centers();
        let current = scene.state.current(scene.config);
        if centers.is_empty() {
            return;
        }
        self.draw_connectors(pixmap, scene);

        for depth in 0..path.len() {
            let item = item_at_path(&scene.config.menu, &path[..depth]);
            let active = scene.state.active() == Some(Target::Parent(depth));
            let style = item_style(scene.styles, item, Role::History, active);
            self.draw_item(
                pixmap,
                ItemFrame {
                    center: centers[depth],
                    item,
                    style: &style,
                    styles: scene.styles,
                    icon_root: scene.icon_root,
                    active,
                    content: if active {
                        ItemContent::IconOrBlank
                    } else {
                        ItemContent::Default
                    },
                    indicators: true,
                },
            );
        }

        let center = *centers.last().unwrap();
        let center_active = scene.state.active() == Some(Target::Center);
        let center_style = item_style(scene.styles, current, Role::Center, center_active);
        let center_content = match scene.state.active() {
            Some(Target::Item(index)) => current
                .items
                .get(index)
                .map_or(ItemContent::Default, |item| ItemContent::Label(&item.label)),
            Some(Target::Parent(_)) => ItemContent::IconOrBlank,
            _ => ItemContent::Default,
        };
        self.draw_item(
            pixmap,
            ItemFrame {
                center,
                item: current,
                style: &center_style,
                styles: scene.styles,
                icon_root: scene.icon_root,
                active: center_active,
                content: center_content,
                indicators: false,
            },
        );

        for (index, item) in current.items.iter().enumerate() {
            let active = scene.state.active() == Some(Target::Item(index));
            let style = item_style(scene.styles, item, Role::Item, active);
            let distance = (scene.config.menu_radius
                + if active {
                    style.distance.unwrap_or(0.0)
                } else {
                    0.0
                })
            .max(0.0);
            let position = radial_position(center, item.angle.unwrap_or(0.0), distance);
            self.draw_item(
                pixmap,
                ItemFrame {
                    center: position,
                    item,
                    style: &style,
                    styles: scene.styles,
                    icon_root: scene.icon_root,
                    active,
                    content: ItemContent::Default,
                    indicators: true,
                },
            );
        }
    }

    fn draw_connectors(&mut self, pixmap: &mut Pixmap, scene: &Scene<'_>) {
        let centers = scene.state.centers();
        let path = scene.state.path();
        if centers.len() < 2 {
            return;
        }
        let style = scene.styles.circle(&["connector"]).unwrap_or_default();
        let width = style.width.unwrap_or(0.0);
        if width <= 0.0 {
            return;
        }
        let mut paint = Paint::default();
        paint.set_color(to_skia(style.color, style.opacity));
        let stroke = Stroke {
            width: width as f32,
            ..Stroke::default()
        };
        for (depth, pair) in centers.windows(2).enumerate() {
            let start_item = item_at_path(&scene.config.menu, &path[..depth]);
            let end_item = item_at_path(&scene.config.menu, &path[..depth + 1]);
            let start_style = item_style(scene.styles, start_item, Role::History, false);
            let end_style = item_style(scene.styles, end_item, Role::Center, false);
            let start_radius = start_style.width.unwrap_or(0.0) * start_style.scale / 2.0;
            let end_radius = end_style.width.unwrap_or(0.0) * end_style.scale / 2.0;
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

    fn draw_item(&mut self, pixmap: &mut Pixmap, frame: ItemFrame<'_>) {
        let size = frame.style.width.unwrap_or(0.0) * frame.style.scale;
        if size <= 0.0 {
            return;
        }
        if frame.indicators && frame.item.is_submenu() {
            self.draw_indicators(
                pixmap,
                frame.center,
                size,
                IndicatorFrame {
                    item: frame.item,
                    submenu_style: frame.style,
                    styles: frame.styles,
                    active: frame.active,
                },
            );
        }
        draw_rounded_box(pixmap, frame.center, size, frame.style);

        let show_icon = frame.item.icon.is_some()
            && matches!(
                frame.content,
                ItemContent::Default | ItemContent::IconOrBlank
            );
        let icon_drawn = show_icon
            && self.draw_icon(
                pixmap,
                frame.center,
                size,
                frame.item,
                frame.style,
                frame.icon_root,
            );
        if icon_drawn {
            return;
        }
        if matches!(frame.content, ItemContent::IconOrBlank) {
            return;
        }
        let label = match frame.content {
            ItemContent::Default => &frame.item.label,
            ItemContent::Label(label) => label,
            ItemContent::IconOrBlank => unreachable!(),
        };
        if !label.is_empty() {
            self.draw_text(pixmap, frame.center, size, label, frame.style);
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
        let size = style.width.unwrap_or(0.0);
        if size <= 0.0 || style.protrusion <= 0.0 {
            return;
        }
        let protrusion = style.protrusion;
        let radius = (circle_size / 2.0 - size / 2.0 + protrusion).max(0.0);
        let mut paint = Paint::default();
        paint.set_color(to_skia(style.color, style.opacity));
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
        let family = self
            .resolved_families
            .get(&style.font_family)
            .cloned()
            .unwrap_or_else(|| style.font_family.clone());
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(font_size, line_height));
        {
            let mut buffer = buffer.borrow_with(&mut self.fonts);
            buffer.set_size(Some(available), Some(available));
            buffer.set_wrap(Wrap::WordOrGlyph);
            let attrs = Attrs::new().family(Family::Name(&family));
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

fn empty_font_system() -> FontSystem {
    FontSystem::new_with_locale_and_db(current_locale(), cosmic_text::fontdb::Database::new())
}

fn current_locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        .map(|locale| {
            locale
                .split(['.', '@'])
                .next()
                .unwrap_or("en-US")
                .replace('_', "-")
        })
        .unwrap_or_else(|| "en-US".into())
}

fn resolve_font(requested: &str) -> Option<FontSpec> {
    let output = Command::new("fc-match")
        .args(["-f", "%{family[0]}\n%{file}\n", requested])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let output = String::from_utf8(output.stdout).ok()?;
    let mut lines = output.lines();
    let family = lines.next()?.trim();
    let file = PathBuf::from(lines.next()?.trim());
    (!family.is_empty() && file.is_file()).then(|| FontSpec {
        family: family.to_string(),
        file,
    })
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
