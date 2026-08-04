use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn distance(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }

    pub fn lerp(self, other: Self, progress: f64) -> Self {
        Self {
            x: self.x + (other.x - self.x) * progress,
            y: self.y + (other.y - self.y) * progress,
        }
    }
}

pub fn radial_position(center: Point, angle: f64, distance: f64) -> Point {
    let radians = angle.to_radians();
    Point {
        x: center.x + distance * radians.sin(),
        y: center.y - distance * radians.cos(),
    }
}

pub fn direction_angle(delta: Point) -> f64 {
    delta.x.atan2(-delta.y).rem_euclid(TAU).to_degrees()
}

pub fn angular_distance(first: f64, second: f64) -> f64 {
    ((first - second + 180.0).rem_euclid(360.0) - 180.0).abs()
}

pub fn clamp_center(point: Point, width: u32, height: u32, edge: f64) -> Point {
    let clamp = |value: f64, extent: u32| {
        if edge * 2.0 > extent as f64 {
            extent as f64 / 2.0
        } else {
            value.clamp(edge, extent as f64 - edge)
        }
    };
    Point {
        x: clamp(point.x, width),
        y: clamp(point.y, height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_are_clockwise_from_up() {
        assert_eq!(direction_angle(Point { x: 0.0, y: -1.0 }), 0.0);
        assert_eq!(direction_angle(Point { x: 1.0, y: 0.0 }), 90.0);
    }
}
