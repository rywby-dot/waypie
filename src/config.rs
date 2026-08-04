use std::{fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    #[serde(default = "default_menu_radius")]
    pub menu_radius: f64,
    pub center_hitbox_size: Option<f64>,
    #[serde(default)]
    pub minimum_edge_distance: f64,
    #[serde(default)]
    pub center_mode: bool,
    #[serde(default)]
    pub hover_mode: bool,
    #[serde(default)]
    pub turbo_mode: bool,
    #[serde(default)]
    pub travel_item_animation: bool,
    #[serde(default)]
    pub close_submenu_on_center_click: bool,
    pub menu: Item,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Item {
    #[serde(default)]
    pub label: String,
    pub command: Option<String>,
    pub angle: Option<f64>,
    #[serde(rename = "icon-theme")]
    pub icon_theme: Option<String>,
    pub icon: Option<String>,
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(skip)]
    pub return_angle: Option<f64>,
}

impl Item {
    pub fn is_submenu(&self) -> bool {
        self.command.is_none()
    }
}

pub fn item_at_path<'a>(root: &'a Item, path: &[usize]) -> &'a Item {
    path.iter().fold(root, |item, index| &item.items[*index])
}

fn default_menu_radius() -> f64 {
    170.0
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let source =
            fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
        let mut config: Self = toml::from_str(&source)
            .with_context(|| format!("invalid config {}", path.display()))?;
        config.validate()?;
        resolve_angles(&mut config.menu, true);
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if !self.menu_radius.is_finite() || self.menu_radius <= 0.0 {
            bail!("menu-radius must be positive");
        }
        if self
            .center_hitbox_size
            .is_some_and(|v| !v.is_finite() || v < 0.0)
        {
            bail!("center-hitbox-size cannot be negative");
        }
        if !self.minimum_edge_distance.is_finite() || self.minimum_edge_distance < 0.0 {
            bail!("minimum-edge-distance cannot be negative");
        }
        validate_item(&self.menu, "menu", true)
    }
}

fn validate_item(item: &Item, path: &str, root: bool) -> Result<()> {
    if item.command.as_deref() == Some("") {
        bail!("{path}.command cannot be empty");
    }
    if item.command.is_some() && !item.items.is_empty() {
        bail!("{path} cannot have command and items");
    }
    if !root && item.angle.is_some_and(|v| !v.is_finite()) {
        bail!("{path}.angle must be finite");
    }
    if item.icon_theme.is_some() != item.icon.is_some() {
        bail!("{path}.icon-theme and .icon must be used together");
    }
    for (index, child) in item.items.iter().enumerate() {
        validate_item(child, &format!("{path}.items[{index}]"), false)?;
    }
    Ok(())
}

pub fn resolve_angles(item: &mut Item, root: bool) {
    let count = item.items.len();
    for (index, child) in item.items.iter_mut().enumerate() {
        child.angle = Some(
            child
                .angle
                .map(|value| value.round().rem_euclid(360.0))
                .unwrap_or_else(|| index as f64 * 360.0 / count.max(1) as f64),
        );
        resolve_angles(child, false);
    }
    if !root {
        item.return_angle = Some(largest_gap_angle(
            &item
                .items
                .iter()
                .filter_map(|child| child.angle)
                .collect::<Vec<_>>(),
            item.angle.map(|angle| (angle + 180.0).rem_euclid(360.0)),
        ));
    }
}

pub fn largest_gap_angle(angles: &[f64], preferred: Option<f64>) -> f64 {
    if angles.is_empty() {
        return preferred.unwrap_or(180.0).round().rem_euclid(360.0);
    }
    let mut ordered = angles
        .iter()
        .map(|angle| angle.rem_euclid(360.0))
        .collect::<Vec<_>>();
    ordered.sort_by(f64::total_cmp);
    let mut gaps = ordered
        .iter()
        .enumerate()
        .map(|(index, start)| {
            let end = ordered[(index + 1) % ordered.len()]
                + if index + 1 == ordered.len() {
                    360.0
                } else {
                    0.0
                };
            (end - start, (start + (end - start) / 2.0).rem_euclid(360.0))
        })
        .collect::<Vec<_>>();
    gaps.sort_by(|left, right| right.0.total_cmp(&left.0));
    let best_size = gaps[0].0;
    let candidates = gaps
        .into_iter()
        .take_while(|(size, _)| (size - best_size).abs() < 1e-9)
        .map(|(_, angle)| angle)
        .collect::<Vec<_>>();
    preferred
        .map(|target| {
            candidates
                .iter()
                .copied()
                .min_by(|a, b| {
                    angular_distance(*a, target).total_cmp(&angular_distance(*b, target))
                })
                .unwrap()
        })
        .unwrap_or(candidates[0])
        .round()
        .rem_euclid(360.0)
}

fn angular_distance(a: f64, b: f64) -> f64 {
    ((a - b + 180.0).rem_euclid(360.0) - 180.0).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_submenu_gets_opposite_return_angle() {
        let mut root = Item {
            label: "Root".into(),
            command: None,
            angle: None,
            icon_theme: None,
            icon: None,
            items: vec![Item {
                label: "Empty".into(),
                command: None,
                angle: Some(90.0),
                icon_theme: None,
                icon: None,
                items: vec![],
                return_angle: None,
            }],
            return_angle: None,
        };
        resolve_angles(&mut root, true);
        assert_eq!(root.items[0].return_angle, Some(270.0));
    }

    #[test]
    fn return_uses_largest_gap() {
        assert_eq!(largest_gap_angle(&[0.0, 90.0, 180.0], None), 270.0);
    }

    #[test]
    fn travel_item_animation_is_optional_and_can_be_enabled() {
        let disabled: Config = toml::from_str("[menu]\nlabel = 'Root'").unwrap();
        let enabled: Config =
            toml::from_str("travel-item-animation = true\n[menu]\nlabel = 'Root'").unwrap();
        assert!(!disabled.travel_item_animation);
        assert!(enabled.travel_item_animation);
    }
}
