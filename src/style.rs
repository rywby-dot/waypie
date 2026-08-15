use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::Path,
    time::Duration,
};

use crate::animation::Spring;
use anyhow::{Context, Result, bail};

const MAX_ANIMATION_DURATION: Duration = Duration::from_secs(60 * 60);
const CIRCLE_PROPERTIES: &[&str] = &[
    "background-color",
    "border-color",
    "border-width",
    "border-radius",
    "color",
    "cut-indicators",
    "distance",
    "font-size",
    "font-family",
    "font-weight",
    "follow-distance",
    "icon-fill",
    "icon-size",
    "opacity",
    "text-fill",
    "text-opacity",
    "protrusion",
    "scale",
    "width",
];
const ITEM_KEY_PROPERTIES: &[&str] = &[
    "angle",
    "color",
    "distance",
    "off",
    "font-family",
    "font-size",
    "font-weight",
    "opacity",
    "scale",
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub alpha: f32,
}

impl Color {
    pub const TRANSPARENT: Self = Self {
        red: 0.0,
        green: 0.0,
        blue: 0.0,
        alpha: 0.0,
    };
    pub const WHITE: Self = Self {
        red: 1.0,
        green: 1.0,
        blue: 1.0,
        alpha: 1.0,
    };
}

#[derive(Clone, Debug)]
pub struct CircleStyle {
    pub background_color: Color,
    pub border_color: Color,
    pub border_width: f64,
    pub border_radius: Radius,
    pub color: Color,
    pub cut_indicators: bool,
    pub distance: Option<f64>,
    pub font_size: f64,
    pub font_family: String,
    pub font_weight: u16,
    pub follow_distance: f64,
    pub icon_fill: Option<f64>,
    pub icon_size: Option<f64>,
    pub opacity: f64,
    pub text_fill: f64,
    pub text_opacity: Option<f64>,
    pub protrusion: f64,
    pub scale: f64,
    pub width: Option<f64>,
}

impl Default for CircleStyle {
    fn default() -> Self {
        Self {
            background_color: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
            border_radius: Radius::Percent(0.5),
            color: Color::WHITE,
            cut_indicators: true,
            distance: None,
            font_size: 14.0,
            font_family: "Sans".into(),
            font_weight: 400,
            follow_distance: 0.0,
            icon_fill: None,
            icon_size: None,
            opacity: 1.0,
            text_fill: 1.0,
            text_opacity: None,
            protrusion: 0.0,
            scale: 1.0,
            width: None,
        }
    }
}

impl CircleStyle {
    pub fn content_opacity(&self) -> f64 {
        self.text_opacity.unwrap_or(self.opacity)
    }

    pub fn radius(&self, size: f64) -> f64 {
        match self.border_radius {
            Radius::Pixels(value) => value.min(size / 2.0),
            Radius::Percent(value) => (size * value).min(size / 2.0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ItemKeyStyle {
    pub angle: f64,
    pub color: Color,
    pub distance: f64,
    pub enabled: bool,
    pub font_family: String,
    pub font_size: f64,
    pub font_weight: u16,
    pub opacity: f64,
    pub scale: f64,
}

impl Default for ItemKeyStyle {
    fn default() -> Self {
        Self {
            angle: 0.0,
            color: Color::WHITE,
            distance: 8.0,
            enabled: true,
            font_family: "Sans".into(),
            font_size: 12.0,
            font_weight: 600,
            opacity: 1.0,
            scale: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Radius {
    Pixels(f64),
    Percent(f64),
}

#[derive(Clone, Copy, Debug)]
pub struct AnimationStyle {
    pub off: bool,
    pub color_duration: Duration,
    pub opacity_duration: Duration,
    pub icon_duration: Duration,
    pub item_delete_duration: Duration,
    pub close_duration: Duration,
    pub connector_duration: Duration,
    pub action_scale: f64,
    pub hover_spring: Spring,
    pub follow_spring: Spring,
    pub menu_move_spring: Spring,
    pub item_create_spring: Spring,
    pub action_spring: Spring,
    pub submenu_indicator_spring: Spring,
}

#[derive(Clone, Debug)]
pub struct StyleSheet {
    rules: HashMap<String, HashMap<String, String>>,
}

impl StyleSheet {
    pub fn load(path: &Path) -> Result<Self> {
        let source =
            fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
        validate_style_syntax(&source)?;
        let sheet = Self::parse(&source);
        sheet.validate()?;
        if sheet.circle(&["circle"])?.width.is_none() {
            bail!("style.css requires circle {{ width: ...; }}");
        }
        Ok(sheet)
    }

    fn validate(&self) -> Result<()> {
        for (selector, properties) in &self.rules {
            if selector == "animation" {
                for name in properties.keys() {
                    if !is_animation_property(name) {
                        bail!("unknown animation property: {name}");
                    }
                }
                self.animation()
                    .with_context(|| format!("invalid {selector} style"))?;
                continue;
            }
            if selector == "item-key" || selector == "item-key.active" {
                let mut style = ItemKeyStyle::default();
                for (name, value) in properties {
                    if !ITEM_KEY_PROPERTIES.contains(&name.as_str()) {
                        bail!("unknown {selector} property: {name}");
                    }
                    apply_item_key_property(&mut style, name, value)
                        .with_context(|| format!("invalid {selector}.{name}"))?;
                }
                continue;
            }
            if !is_circle_selector(selector) {
                bail!("unknown style selector: {selector}");
            }
            let mut style = CircleStyle::default();
            for (name, value) in properties {
                if !CIRCLE_PROPERTIES.contains(&name.as_str()) {
                    bail!("unknown {selector} property: {name}");
                }
                apply_circle_property(&mut style, name, value)
                    .with_context(|| format!("invalid {selector}.{name}"))?;
            }
        }
        Ok(())
    }

    pub fn parse(source: &str) -> Self {
        let source = strip_comments(source);
        let mut rules: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut rest = source.as_str();
        while let Some(open) = rest.find('{') {
            let selectors = &rest[..open];
            let after_open = &rest[open + 1..];
            let Some(close) = after_open.find('}') else {
                break;
            };
            let block = &after_open[..close];
            let mut declarations = parse_declarations(block);
            if has_bare_flag(block, "off") {
                declarations.insert("off".into(), "true".into());
            }
            for selector in selectors.split(',') {
                rules
                    .entry(selector.trim().to_ascii_lowercase())
                    .or_default()
                    .extend(declarations.clone());
            }
            rest = &after_open[close + 1..];
        }
        if let Some(circle) = rules.get_mut("circle") {
            circle.remove("distance");
        }
        Self { rules }
    }

    pub fn circle(&self, selectors: &[&str]) -> Result<CircleStyle> {
        let mut style = CircleStyle::default();
        for selector in selectors {
            if let Some(properties) = self.rules.get(*selector) {
                for (name, value) in properties {
                    apply_circle_property(&mut style, name, value)?;
                }
            }
        }
        Ok(style)
    }

    pub fn color_style(&self, selector: &str) -> Result<CircleStyle> {
        self.circle(&[selector])
    }

    pub fn item_key(&self, active: bool) -> Result<ItemKeyStyle> {
        let mut style = ItemKeyStyle::default();
        for selector in if active {
            ["item-key", "item-key.active"].as_slice()
        } else {
            ["item-key"].as_slice()
        } {
            if let Some(properties) = self.rules.get(*selector) {
                for (name, value) in properties {
                    apply_item_key_property(&mut style, name, value)?;
                }
            }
        }
        Ok(style)
    }

    pub fn animation(&self) -> Result<AnimationStyle> {
        let rule = self.rules.get("animation");
        if rule.is_some_and(|properties| properties.contains_key("off")) {
            return Ok(AnimationStyle {
                off: true,
                color_duration: Duration::ZERO,
                opacity_duration: Duration::ZERO,
                icon_duration: Duration::ZERO,
                item_delete_duration: Duration::ZERO,
                close_duration: Duration::ZERO,
                connector_duration: Duration::ZERO,
                action_scale: 1.0,
                hover_spring: Spring::default(),
                follow_spring: Spring::default(),
                menu_move_spring: Spring::default(),
                item_create_spring: Spring::default(),
                action_spring: Spring::default(),
                submenu_indicator_spring: Spring::default(),
            });
        }
        let duration = |name: &str, fallback: Option<&str>| -> Result<Duration> {
            let value = rule
                .and_then(|values| values.get(name))
                .or_else(|| fallback.and_then(|fallback| rule.and_then(|r| r.get(fallback))));
            value.map_or(Ok(Duration::ZERO), |value| parse_duration(value, name))
        };
        let number = |name: &str, default: f64| -> Result<f64> {
            rule.and_then(|values| values.get(name))
                .map_or(Ok(default), |value| parse_positive(value, name))
        };
        let spring = |prefix: &str| -> Result<Spring> {
            let damping_ratio = number(&format!("{prefix}-damping-ratio"), 1.0)?;
            if !(0.1..=10.0).contains(&damping_ratio) {
                bail!("{prefix}-damping-ratio must be between 0.1 and 10");
            }
            let epsilon = number(&format!("{prefix}-epsilon"), 0.0001)?;
            if epsilon >= 1.0 {
                bail!("{prefix}-epsilon must be less than 1");
            }
            let spring = Spring {
                damping_ratio,
                stiffness: number(&format!("{prefix}-stiffness"), 1000.0)?,
                epsilon,
            };
            if spring.checked_duration().is_none() {
                bail!("{prefix} spring parameters produce an unsupported duration");
            }
            if spring.duration() > MAX_ANIMATION_DURATION {
                bail!("{prefix} spring duration cannot exceed one hour");
            }
            Ok(spring)
        };
        Ok(AnimationStyle {
            off: false,
            color_duration: duration("color-duration", None)?,
            opacity_duration: duration("opacity-duration", None)?,
            icon_duration: duration("icon-duration", None)?,
            item_delete_duration: duration("item-delete-duration", None)?,
            close_duration: duration("close-duration", None)?,
            connector_duration: duration("connector-duration", None)?,
            action_scale: number("action-scale", 1.3)?,
            hover_spring: spring("hover")?,
            follow_spring: spring("follow")?,
            menu_move_spring: spring("menu-move")?,
            item_create_spring: spring("item-create")?,
            action_spring: spring("action")?,
            submenu_indicator_spring: spring("submenu-indicator")?,
        })
    }

    pub fn font_families(&self) -> Vec<String> {
        let mut families = BTreeSet::new();
        families.extend(
            self.rules
                .values()
                .filter_map(|properties| properties.get("font-family"))
                .map(|family| family.trim_matches(['\'', '"']).to_string()),
        );
        if families.is_empty() {
            families.insert(CircleStyle::default().font_family);
        }
        families.into_iter().collect()
    }

    pub fn font_requests(&self) -> Vec<(String, u16)> {
        let mut requests = BTreeSet::new();
        let circle_selectors: &[&[&str]] = &[
            &["circle"],
            &["circle", "circle.active"],
            &["circle", "circle.item"],
            &[
                "circle",
                "circle.item",
                "circle.active",
                "circle.item.active",
            ],
            &["circle", "circle.item", "circle.submenu"],
            &[
                "circle",
                "circle.item",
                "circle.submenu",
                "circle.active",
                "circle.submenu.active",
            ],
            &["circle", "circle.center"],
            &[
                "circle",
                "circle.center",
                "circle.active",
                "circle.center.active",
            ],
            &["circle", "circle.history"],
            &[
                "circle",
                "circle.history",
                "circle.active",
                "circle.history.active",
            ],
        ];
        for selectors in circle_selectors {
            if let Ok(style) = self.circle(selectors) {
                requests.insert((style.font_family, style.font_weight));
            }
        }
        for active in [false, true] {
            if let Ok(style) = self.item_key(active)
                && style.enabled
            {
                requests.insert((style.font_family, style.font_weight));
            }
        }
        requests.into_iter().collect()
    }

    pub fn raw(&self, selector: &str, property: &str) -> Option<&str> {
        self.rules
            .get(selector)
            .and_then(|rule| rule.get(property))
            .map(String::as_str)
    }
}

fn is_circle_selector(selector: &str) -> bool {
    matches!(
        selector,
        "overlay"
            | "circle"
            | "circle.active"
            | "circle.item"
            | "circle.item.active"
            | "circle.submenu"
            | "circle.submenu.active"
            | "circle.center"
            | "circle.center.active"
            | "circle.history"
            | "circle.history.active"
            | "circle.previous"
            | "connector"
            | "connector.active"
            | "submenu-indicator"
            | "submenu-indicator.active"
            | "submenu-indicator.return"
            | "submenu-indicator.return.active"
            | "parent-link"
            | "configurator-history"
    )
}

fn is_animation_property(name: &str) -> bool {
    matches!(
        name,
        "off"
            | "color-duration"
            | "opacity-duration"
            | "icon-duration"
            | "connector-duration"
            | "item-delete-duration"
            | "close-duration"
            | "action-scale"
            | "hover-duration"
            | "follow-duration"
            | "menu-duration"
            | "menu-move-duration"
            | "item-create-duration"
            | "action-duration"
            | "submenu-indicator-duration"
    ) || [
        "hover",
        "follow",
        "menu-move",
        "item-create",
        "action",
        "submenu-indicator",
    ]
    .iter()
    .any(|prefix| {
        name.strip_prefix(prefix)
            .and_then(|suffix| suffix.strip_prefix('-'))
            .is_some_and(|suffix| matches!(suffix, "damping-ratio" | "stiffness" | "epsilon"))
    })
}

fn strip_comments(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find("/*") {
        output.push_str(&rest[..start]);
        let comment = &rest[start + 2..];
        let Some(end) = comment.find("*/") else {
            return output;
        };
        rest = &comment[end + 2..];
    }
    output.push_str(rest);
    output
}

fn validate_style_syntax(source: &str) -> Result<()> {
    let mut comments = source;
    while let Some(start) = comments.find("/*") {
        if comments[..start].contains("*/") {
            bail!("style contains an unmatched comment terminator");
        }
        let after_start = &comments[start + 2..];
        let Some(end) = after_start.find("*/") else {
            bail!("style contains an unclosed comment");
        };
        comments = &after_start[end + 2..];
    }
    if comments.contains("*/") {
        bail!("style contains an unmatched comment terminator");
    }
    let source = strip_comments(source);
    let mut depth = 0_u8;
    for character in source.chars() {
        match character {
            '{' if depth == 0 => depth = 1,
            '{' => bail!("nested style blocks are not supported"),
            '}' if depth == 1 => depth = 0,
            '}' => bail!("style contains an unmatched closing brace"),
            _ => {}
        }
    }
    if depth != 0 {
        bail!("style contains an unclosed block");
    }
    for block in source
        .split('{')
        .skip(1)
        .filter_map(|part| part.split_once('}'))
    {
        let declarations = block
            .0
            .lines()
            .filter(|line| !line.trim().eq_ignore_ascii_case("off"))
            .collect::<Vec<_>>()
            .join("\n");
        for declaration in declarations.split(';').map(str::trim) {
            if !declaration.is_empty() && !declaration.contains(':') {
                bail!("invalid style declaration: {declaration}");
            }
        }
    }
    Ok(())
}

fn parse_declarations(source: &str) -> HashMap<String, String> {
    let source = source
        .lines()
        .filter(|line| !line.trim().eq_ignore_ascii_case("off"))
        .collect::<Vec<_>>()
        .join("\n");
    source
        .split(';')
        .filter_map(|declaration| declaration.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect()
}

fn has_bare_flag(source: &str, flag: &str) -> bool {
    source
        .split(';')
        .flat_map(str::lines)
        .any(|line| line.trim().eq_ignore_ascii_case(flag))
}

fn apply_circle_property(style: &mut CircleStyle, name: &str, value: &str) -> Result<()> {
    match name {
        "background-color" => style.background_color = parse_color(value, name)?,
        "border-color" => style.border_color = parse_color(value, name)?,
        "border-width" => style.border_width = parse_pixels(value, name)?,
        "border-radius" => style.border_radius = parse_radius(value)?,
        "color" => style.color = parse_color(value, name)?,
        "cut-indicators" => {
            style.cut_indicators = match value.to_ascii_lowercase().as_str() {
                "true" => true,
                "false" => false,
                _ => bail!("cut-indicators must be true or false"),
            }
        }
        "distance" => style.distance = Some(parse_signed_pixels(value, name)?),
        "font-size" => style.font_size = parse_pixels(value, name)?,
        "font-family" => style.font_family = value.trim_matches(['\'', '"']).to_string(),
        "font-weight" => style.font_weight = parse_font_weight(value)?,
        "follow-distance" => style.follow_distance = parse_percentage(value, name, true)?,
        "icon-fill" => style.icon_fill = Some(parse_percentage(value, name, false)?),
        "icon-size" => style.icon_size = Some(parse_pixels(value, name)?),
        "opacity" => style.opacity = parse_opacity(value, name)?,
        "text-fill" => style.text_fill = parse_percentage(value, name, false)?,
        "text-opacity" => style.text_opacity = Some(parse_opacity(value, name)?),
        "protrusion" => style.protrusion = parse_pixels(value, name)?,
        "scale" => style.scale = parse_positive(value, name)?,
        "width" => style.width = Some(parse_pixels(value, name)?),
        _ => {}
    }
    Ok(())
}

fn apply_item_key_property(style: &mut ItemKeyStyle, name: &str, value: &str) -> Result<()> {
    match name {
        "angle" => style.angle = parse_angle(value, name)?,
        "color" => style.color = parse_color(value, name)?,
        "distance" => style.distance = parse_signed_pixels(value, name)?,
        "off" => style.enabled = false,
        "font-family" => style.font_family = value.trim_matches(['\'', '"']).to_string(),
        "font-size" => style.font_size = parse_pixels(value, name)?,
        "font-weight" => style.font_weight = parse_font_weight(value)?,
        "opacity" => style.opacity = parse_opacity(value, name)?,
        "scale" => style.scale = parse_positive(value, name)?,
        _ => {}
    }
    Ok(())
}

fn parse_angle(value: &str, name: &str) -> Result<f64> {
    let value = value.trim().strip_suffix("deg").unwrap_or(value.trim());
    let angle: f64 = value
        .parse()
        .with_context(|| format!("invalid {name}: {value}"))?;
    if !angle.is_finite() || !(0.0..360.0).contains(&angle) {
        bail!("{name} must be between 0 and 359 degrees");
    }
    Ok(angle)
}

fn parse_font_weight(value: &str) -> Result<u16> {
    match value.trim().to_ascii_lowercase().as_str() {
        "normal" => Ok(400),
        "bold" => Ok(700),
        value => {
            let weight: u16 = value.parse()?;
            if !(1..=1000).contains(&weight) {
                bail!("font-weight must be between 1 and 1000");
            }
            Ok(weight)
        }
    }
}

fn parse_color(value: &str, name: &str) -> Result<Color> {
    let value = value.trim().to_ascii_lowercase();
    if value == "transparent" {
        return Ok(Color::TRANSPARENT);
    }
    if let Some(hex) = value.strip_prefix('#')
        && (hex.len() == 6 || hex.len() == 8)
    {
        let channel = |offset| u8::from_str_radix(&hex[offset..offset + 2], 16);
        return Ok(Color {
            red: channel(0)? as f32 / 255.0,
            green: channel(2)? as f32 / 255.0,
            blue: channel(4)? as f32 / 255.0,
            alpha: if hex.len() == 8 {
                channel(6)? as f32 / 255.0
            } else {
                1.0
            },
        });
    }
    if let Some(parts) = value
        .strip_prefix("rgba(")
        .or_else(|| value.strip_prefix("rgb("))
        .and_then(|parts| parts.strip_suffix(')'))
    {
        let values = parts
            .split(',')
            .map(str::trim)
            .map(str::parse::<f32>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if values.len() == 3 || values.len() == 4 {
            if values.iter().any(|value| !value.is_finite()) {
                bail!("invalid {name}: {value}");
            }
            return Ok(Color {
                red: values[0].clamp(0.0, 255.0) / 255.0,
                green: values[1].clamp(0.0, 255.0) / 255.0,
                blue: values[2].clamp(0.0, 255.0) / 255.0,
                alpha: values.get(3).copied().unwrap_or(1.0).clamp(0.0, 1.0),
            });
        }
    }
    bail!("invalid {name}: {value}")
}

fn parse_radius(value: &str) -> Result<Radius> {
    if let Some(percent) = value.trim().strip_suffix('%') {
        let percent = percent.trim().parse::<f64>()?;
        if !percent.is_finite() {
            bail!("invalid border-radius: {value}");
        }
        return Ok(Radius::Percent(percent.max(0.0) / 100.0));
    }
    Ok(Radius::Pixels(parse_pixels(value, "border-radius")?))
}

fn parse_pixels(value: &str, name: &str) -> Result<f64> {
    Ok(parse_signed_pixels(value, name)?.max(0.0))
}

fn parse_signed_pixels(value: &str, name: &str) -> Result<f64> {
    let value = value.trim().strip_suffix("px").unwrap_or(value.trim());
    let number: f64 = value
        .parse()
        .with_context(|| format!("invalid {name}: {value}"))?;
    if !number.is_finite() {
        bail!("invalid {name}: {value}");
    }
    Ok(number)
}

fn parse_percentage(value: &str, name: &str, bounded: bool) -> Result<f64> {
    let Some(value) = value.trim().strip_suffix('%') else {
        bail!("invalid {name}: {value}");
    };
    let number: f64 = value.parse()?;
    if !number.is_finite() || number < 0.0 || bounded && number > 100.0 {
        bail!("invalid {name}: {value}%");
    }
    Ok(number / 100.0)
}

fn parse_opacity(value: &str, name: &str) -> Result<f64> {
    let number: f64 = value.parse()?;
    if !number.is_finite() || !(0.0..=1.0).contains(&number) {
        bail!("{name} must be between 0 and 1");
    }
    Ok(number)
}

fn parse_positive(value: &str, name: &str) -> Result<f64> {
    let number: f64 = value.parse()?;
    if !number.is_finite() || number <= 0.0 {
        bail!("{name} must be positive");
    }
    Ok(number)
}

fn parse_duration(value: &str, name: &str) -> Result<Duration> {
    let value = value.trim().to_ascii_lowercase();
    let (number, multiplier) = if let Some(number) = value.strip_suffix("ms") {
        (number, 0.001)
    } else if let Some(number) = value.strip_suffix('s') {
        (number, 1.0)
    } else {
        bail!("{name} must use ms or s");
    };
    let seconds = number.trim().parse::<f64>()? * multiplier;
    if !seconds.is_finite() || seconds < 0.0 {
        bail!("{name} must be a finite non-negative duration");
    }
    let duration =
        Duration::try_from_secs_f64(seconds).with_context(|| format!("invalid {name}: {value}"))?;
    if duration > MAX_ANIMATION_DURATION {
        bail!("{name} cannot exceed one hour");
    }
    Ok(duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_styles_are_valid() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for path in ["config/style.css", "config/style_foot.css"] {
            StyleSheet::load(&root.join(path)).unwrap();
        }
    }

    #[test]
    fn cascade_and_large_fill_values_match_python() {
        let sheet = StyleSheet::parse(
            "circle { width: 70px; color: #ffffff; text-fill: 130%; }\n\
             circle.item { opacity: 0.4; }",
        );
        let style = sheet.circle(&["circle", "circle.item"]).unwrap();
        assert_eq!(style.width, Some(70.0));
        assert_eq!(style.opacity, 0.4);
        assert_eq!(style.text_fill, 1.3);
    }

    #[test]
    fn alpha_hex_is_supported() {
        let color = parse_color("#242424cc", "color").unwrap();
        assert!((color.alpha - 0.8).abs() < 0.001);
    }

    #[test]
    fn comments_do_not_corrupt_unicode_values() {
        let sheet = StyleSheet::parse("/* кириллица */ circle { font-family: 'Тест'; }");
        assert_eq!(sheet.circle(&["circle"]).unwrap().font_family, "Тест");
    }

    #[test]
    fn non_finite_function_colors_are_rejected() {
        assert!(parse_color("rgb(NaN, 0, 0)", "color").is_err());
        assert!(parse_color("rgba(0, 0, 0, inf)", "color").is_err());
    }

    #[test]
    fn specialized_rules_are_validated_before_rendering() {
        assert!(
            StyleSheet::parse("circle.active { opacity: 2; }")
                .validate()
                .is_err()
        );
        assert!(
            StyleSheet::parse("item-key.active { angle: 360; }")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn malformed_style_structure_is_rejected() {
        assert!(validate_style_syntax("circle { width: 70px;").is_err());
        assert!(validate_style_syntax("circle { width 70px; }").is_err());
    }

    #[test]
    fn unknown_selectors_and_properties_are_rejected() {
        assert!(
            StyleSheet::parse("circle { opacitty: 1; }")
                .validate()
                .is_err()
        );
        assert!(
            StyleSheet::parse("circle.typo { opacity: 1; }")
                .validate()
                .is_err()
        );
        assert!(
            StyleSheet::parse("animation { hover-stifness: 1000; }")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn durations_support_seconds_and_fractional_milliseconds() {
        assert_eq!(
            parse_duration("0.5s", "icon-duration").unwrap(),
            Duration::from_millis(500)
        );
        assert_eq!(
            parse_duration("1.5ms", "icon-duration").unwrap(),
            Duration::from_micros(1500)
        );
    }

    #[test]
    fn active_connector_overrides_the_base_connector_style() {
        let sheet = StyleSheet::parse(
            "connector { color: #ff0000; opacity: 0.4; width: 9px; } \
             connector.active { color: #00ff00; opacity: 0.8; width: 12px; }",
        );
        let style = sheet.circle(&["connector", "connector.active"]).unwrap();
        assert_eq!(style.color, parse_color("#00ff00", "color").unwrap());
        assert_eq!(style.opacity, 0.8);
        assert_eq!(style.width, Some(12.0));
    }

    #[test]
    fn active_item_key_overrides_all_supported_properties() {
        let sheet = StyleSheet::parse(
            "item-key { angle: 359; color: #ff0000; distance: -4px; font-family: Mono; \
             font-size: 12px; font-weight: 500; opacity: 0.4; scale: 1.1; } \
             item-key.active { angle: 45deg; color: #00ff00; distance: 8px; \
             font-size: 15px; font-weight: bold; opacity: 0.9; scale: 1.4; }",
        );
        let style = sheet.item_key(true).unwrap();

        assert_eq!(style.angle, 45.0);
        assert_eq!(style.distance, 8.0);
        assert_eq!(style.font_family, "Mono");
        assert_eq!(style.font_size, 15.0);
        assert_eq!(style.font_weight, 700);
        assert_eq!(style.opacity, 0.9);
        assert_eq!(style.scale, 1.4);
        assert_eq!(style.color.green, 1.0);
    }

    #[test]
    fn active_circle_overrides_font_weight_and_requests_both_faces() {
        let sheet = StyleSheet::parse(
            "circle { font-family: Inter; font-weight: 400; } \
             circle.active { font-weight: 700; }",
        );
        let normal = sheet.circle(&["circle"]).unwrap();
        let active = sheet.circle(&["circle", "circle.active"]).unwrap();

        assert_eq!(normal.font_weight, 400);
        assert_eq!(active.font_weight, 700);
        assert!(sheet.font_requests().contains(&("Inter".into(), 400)));
        assert!(sheet.font_requests().contains(&("Inter".into(), 700)));
    }

    #[test]
    fn item_key_angle_must_be_less_than_three_sixty() {
        let sheet = StyleSheet::parse("item-key { angle: 360; }");
        assert!(sheet.item_key(false).is_err());
    }

    #[test]
    fn base_item_key_off_is_inherited_by_the_active_style() {
        let sheet = StyleSheet::parse(
            "item-key { off; color: #ff0000; } item-key.active { color: #00ff00; opacity: 1; }",
        );

        assert!(!sheet.item_key(false).unwrap().enabled);
        assert!(!sheet.item_key(true).unwrap().enabled);
    }

    #[test]
    fn bare_off_does_not_consume_the_following_property() {
        let sheet = StyleSheet::parse("item-key {\n off\n font-weight: 450; opacity: 0.6; }");
        let style = sheet.item_key(false).unwrap();

        assert!(!style.enabled);
        assert_eq!(style.font_weight, 450);
        assert_eq!(style.opacity, 0.6);
    }

    #[test]
    fn return_indicator_overrides_the_base_indicator_style() {
        let sheet = StyleSheet::parse(
            "submenu-indicator { color: #ff0000; opacity: 0.4; width: 20px; } \
             submenu-indicator.return { color: #00ff00; opacity: 0.7; width: 30px; }",
        );
        let style = sheet
            .circle(&["submenu-indicator", "submenu-indicator.return"])
            .unwrap();
        assert_eq!(style.color, parse_color("#00ff00", "color").unwrap());
        assert_eq!(style.opacity, 0.7);
        assert_eq!(style.width, Some(30.0));
    }

    #[test]
    fn active_return_indicator_has_the_last_cascade_override() {
        let sheet = StyleSheet::parse(
            "submenu-indicator { opacity: 0.2; width: 20px; } \
             submenu-indicator.active { opacity: 0.4; width: 24px; } \
             submenu-indicator.return { opacity: 0.6; width: 28px; } \
             submenu-indicator.return.active { opacity: 0.9; width: 32px; }",
        );
        let style = sheet
            .circle(&[
                "submenu-indicator",
                "submenu-indicator.active",
                "submenu-indicator.return",
                "submenu-indicator.return.active",
            ])
            .unwrap();
        assert_eq!(style.opacity, 0.9);
        assert_eq!(style.width, Some(32.0));
    }

    #[test]
    fn rgb_and_rgba_are_compatible_with_python_styles() {
        assert_eq!(parse_color("rgb(255, 0, 128)", "color").unwrap().red, 1.0);
        assert_eq!(
            parse_color("rgba(255, 0, 128, 0.25)", "color")
                .unwrap()
                .alpha,
            0.25
        );
    }

    #[test]
    fn only_declared_font_families_are_requested() {
        let sheet = StyleSheet::parse(
            "circle { font-family: Inter; } circle.history { font-family: Serif; }",
        );
        assert_eq!(sheet.font_families(), vec!["Inter", "Serif"]);
        assert_eq!(
            StyleSheet::parse("circle { width: 70px; }").font_families(),
            vec!["Sans"]
        );
    }

    #[test]
    fn item_key_font_weights_are_requested_without_loading_every_weight() {
        let sheet = StyleSheet::parse(
            "circle { font-family: Inter; } item-key { font-family: Mono; font-weight: 500; } \
             item-key.active { font-weight: 700; }",
        );
        assert_eq!(
            sheet.font_requests(),
            vec![
                ("Inter".into(), 400),
                ("Mono".into(), 500),
                ("Mono".into(), 700),
            ]
        );
    }

    #[test]
    fn animation_settings_separate_timed_effects_from_springs() {
        let sheet = StyleSheet::parse(
            "animation { color-duration: 160ms; opacity-duration: 170ms; icon-duration: 180ms; item-delete-duration: 250ms; \
             close-duration: 220ms; connector-duration: 140ms; \
             action-scale: 1.4; menu-move-damping-ratio: 0.8; \
             menu-move-stiffness: 700; menu-move-epsilon: 0.001; \
             submenu-indicator-damping-ratio: 0.7; \
             submenu-indicator-stiffness: 600; submenu-indicator-epsilon: 0.002; }",
        );
        let animation = sheet.animation().unwrap();
        assert_eq!(animation.color_duration, Duration::from_millis(160));
        assert_eq!(animation.opacity_duration, Duration::from_millis(170));
        assert_eq!(animation.icon_duration, Duration::from_millis(180));
        assert_eq!(animation.item_delete_duration, Duration::from_millis(250));
        assert_eq!(animation.close_duration, Duration::from_millis(220));
        assert_eq!(animation.connector_duration, Duration::from_millis(140));
        assert_eq!(animation.action_scale, 1.4);
        assert_eq!(animation.menu_move_spring.damping_ratio, 0.8);
        assert_eq!(animation.menu_move_spring.stiffness, 700.0);
        assert_eq!(animation.submenu_indicator_spring.damping_ratio, 0.7);
        assert_eq!(animation.submenu_indicator_spring.stiffness, 600.0);
    }

    #[test]
    fn bare_off_flag_disables_all_timed_animation_values() {
        let sheet =
            StyleSheet::parse("animation {\n off\n color-duration: 5s; close-duration: 5s; }");
        let animation = sheet.animation().unwrap();

        assert!(animation.off);
        assert_eq!(animation.color_duration, Duration::ZERO);
        assert_eq!(animation.opacity_duration, Duration::ZERO);
        assert_eq!(animation.icon_duration, Duration::ZERO);
        assert_eq!(animation.item_delete_duration, Duration::ZERO);
        assert_eq!(animation.close_duration, Duration::ZERO);
        assert_eq!(animation.connector_duration, Duration::ZERO);
        assert_eq!(animation.action_scale, 1.0);
    }

    #[test]
    fn obsolete_spring_durations_do_not_control_physical_timing() {
        let sheet = StyleSheet::parse(
            "animation { hover-duration: 1ms; menu-duration: 2ms; \
             item-create-duration: 3ms; action-duration: 4ms; }",
        );
        let animation = sheet.animation().unwrap();
        assert_eq!(animation.hover_spring, Spring::default());
        assert_eq!(animation.menu_move_spring, Spring::default());
        assert_eq!(animation.item_create_spring, Spring::default());
        assert_eq!(animation.action_spring, Spring::default());
    }
}
