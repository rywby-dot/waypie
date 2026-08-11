use crate::{
    config::Item,
    style::{CircleStyle, StyleSheet},
    visual::NodeRole,
};

pub fn node_style(styles: &StyleSheet, item: &Item, role: NodeRole, active: bool) -> CircleStyle {
    styles
        .circle(&node_selectors(item, role, active))
        .unwrap_or_default()
}

pub fn node_size(style: &CircleStyle) -> f64 {
    style.width.unwrap_or(0.0) * style.scale
}

fn node_selectors(item: &Item, role: NodeRole, active: bool) -> Vec<&'static str> {
    let mut selectors = vec!["circle"];
    match role {
        NodeRole::Item => {
            selectors.push("circle.item");
            if item.is_submenu() {
                selectors.push("circle.submenu");
            }
        }
        NodeRole::Center => selectors.push("circle.center"),
        NodeRole::History => selectors.push("circle.history"),
    }
    if active {
        selectors.push("circle.active");
        selectors.push(match (role, item.is_submenu()) {
            (NodeRole::Item, true) => "circle.submenu.active",
            (NodeRole::Item, false) => "circle.item.active",
            (NodeRole::Center, _) => "circle.center.active",
            (NodeRole::History, _) => "circle.history.active",
        });
    }
    selectors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Item;

    fn item(submenu: bool) -> Item {
        Item {
            label: String::new(),
            keys: String::new(),
            command: (!submenu).then(String::new),
            angle: None,
            icon_theme: None,
            icon: None,
            items: vec![],
            return_angle: None,
        }
    }

    #[test]
    fn submenu_active_selector_is_more_specific_than_generic_active() {
        assert_eq!(
            node_selectors(&item(true), NodeRole::Item, true),
            vec![
                "circle",
                "circle.item",
                "circle.submenu",
                "circle.active",
                "circle.submenu.active"
            ]
        );
    }

    #[test]
    fn action_uses_item_active_selector() {
        assert_eq!(
            node_selectors(&item(false), NodeRole::Item, true),
            vec![
                "circle",
                "circle.item",
                "circle.active",
                "circle.item.active"
            ]
        );
    }
}
