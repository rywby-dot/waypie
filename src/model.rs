use crate::{
    config::{Config, Item, item_at_path},
    geometry::{Point, angular_distance, clamp_center, direction_angle, radial_position},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Center,
    Parent(usize),
    Item(usize),
}

#[derive(Debug, Default)]
pub struct MenuState {
    path: Vec<usize>,
    centers: Vec<Point>,
    link_lengths: Vec<f64>,
    pointer: Option<Point>,
    active: Option<Target>,
}

impl MenuState {
    pub fn path(&self) -> &[usize] {
        &self.path
    }

    pub fn centers(&self) -> &[Point] {
        &self.centers
    }

    pub fn pointer(&self) -> Option<Point> {
        self.pointer
    }

    pub fn active(&self) -> Option<Target> {
        self.active
    }

    pub fn current<'a>(&self, config: &'a Config) -> &'a Item {
        item_at_path(&config.menu, &self.path)
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn place_root(&mut self, center: Point, config: &Config, width: u32, height: u32) {
        if !self.centers.is_empty() {
            return;
        }
        self.centers.push(clamp_center(
            center,
            width,
            height,
            config.minimum_edge_distance,
        ));
    }

    pub fn update_pointer(&mut self, position: Point, config: &Config, center_hitbox: f64) -> bool {
        self.pointer = Some(position);
        let next = self.target_at(position, config, center_hitbox);
        let changed = next != self.active;
        self.active = next;
        changed
    }

    pub fn target_at(
        &self,
        position: Point,
        config: &Config,
        center_hitbox: f64,
    ) -> Option<Target> {
        let center = *self.centers.last()?;
        if center_hitbox > 0.0 && center.distance(position) <= center_hitbox / 2.0 {
            return Some(Target::Center);
        }
        let pointer_angle = direction_angle(Point {
            x: position.x - center.x,
            y: position.y - center.y,
        });
        let current = self.current(config);
        let items = current
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| (Target::Item(index), item.angle.unwrap_or(0.0)));
        let parent = (!self.path.is_empty()).then(|| {
            (
                Target::Parent(self.path.len() - 1),
                current.return_angle.unwrap_or(180.0),
            )
        });
        items
            .chain(parent)
            .min_by(|left, right| {
                angular_distance(pointer_angle, left.1)
                    .total_cmp(&angular_distance(pointer_angle, right.1))
            })
            .map(|candidate| candidate.0)
    }

    pub fn open_submenu(
        &mut self,
        index: usize,
        at: Point,
        config: &Config,
        width: u32,
        height: u32,
    ) -> bool {
        let Some(item) = self.current(config).items.get(index) else {
            return false;
        };
        if !item.is_submenu() {
            return false;
        }
        let parent = *self.centers.last().expect("root must be placed");
        let child = clamp_center(at, width, height, config.minimum_edge_distance);
        self.link_lengths
            .push(parent.distance(child).max(config.menu_radius));
        self.path.push(index);
        self.centers.push(child);
        self.align_history(config);
        self.pointer = Some(at);
        self.active = None;
        true
    }

    pub fn return_to(
        &mut self,
        depth: usize,
        at: Point,
        config: &Config,
        width: u32,
        height: u32,
    ) -> bool {
        if depth >= self.path.len() {
            return false;
        }
        self.path.truncate(depth);
        self.centers.truncate(depth + 1);
        self.link_lengths.truncate(depth);
        *self.centers.last_mut().expect("root must be placed") =
            clamp_center(at, width, height, config.minimum_edge_distance);
        self.align_history(config);
        self.pointer = Some(at);
        self.active = None;
        true
    }

    fn align_history(&mut self, config: &Config) {
        for depth in (1..=self.path.len()).rev() {
            let child = item_at_path(&config.menu, &self.path[..depth]);
            self.centers[depth - 1] = radial_position(
                self.centers[depth],
                child.return_angle.unwrap_or(180.0),
                self.link_lengths[depth - 1],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, angle: f64) -> Item {
        Item {
            label: label.into(),
            command: Some("true".into()),
            angle: Some(angle),
            icon_theme: None,
            icon: None,
            items: vec![],
            return_angle: None,
        }
    }

    fn config() -> Config {
        Config {
            menu_radius: 100.0,
            center_hitbox_size: Some(40.0),
            minimum_edge_distance: 0.0,
            center_mode: false,
            hover_mode: false,
            turbo_mode: false,
            close_submenu_on_center_click: false,
            menu: Item {
                label: "Root".into(),
                command: None,
                angle: None,
                icon_theme: None,
                icon: None,
                items: vec![item("Up", 0.0), item("Right", 90.0)],
                return_angle: None,
            },
        }
    }

    #[test]
    fn active_target_switches_without_retaining_the_previous_item() {
        let config = config();
        let mut state = MenuState::default();
        state.place_root(Point { x: 100.0, y: 100.0 }, &config, 200, 200);
        state.update_pointer(Point { x: 100.0, y: 20.0 }, &config, 40.0);
        assert_eq!(state.active(), Some(Target::Item(0)));
        state.update_pointer(Point { x: 180.0, y: 100.0 }, &config, 40.0);
        assert_eq!(state.active(), Some(Target::Item(1)));
    }

    #[test]
    fn center_hitbox_has_priority_over_directions() {
        let config = config();
        let mut state = MenuState::default();
        state.place_root(Point { x: 100.0, y: 100.0 }, &config, 200, 200);
        state.update_pointer(Point { x: 105.0, y: 100.0 }, &config, 40.0);
        assert_eq!(state.active(), Some(Target::Center));
    }

    #[test]
    fn opening_a_submenu_clears_the_old_active_item() {
        let mut config = config();
        config.menu.items[0].command = None;
        config.menu.items[0].items = vec![item("Child", 0.0)];
        config.menu.items[0].return_angle = Some(180.0);
        let mut state = MenuState::default();
        state.place_root(Point { x: 100.0, y: 100.0 }, &config, 300, 300);
        let pointer = Point { x: 100.0, y: 20.0 };
        state.update_pointer(pointer, &config, 40.0);
        assert_eq!(state.active(), Some(Target::Item(0)));
        assert!(state.open_submenu(0, pointer, &config, 300, 300));
        assert_eq!(state.active(), None);
        assert_eq!(state.path(), &[0]);
    }
}
