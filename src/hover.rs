use std::time::{Duration, Instant};

use crate::geometry::Point;

// Hover Mode tuning. These values match the detector used by Kando.
const ACTIVATION_DISTANCE: f64 = 15.0;
const MIN_STROKE_LENGTH: f64 = 150.0;
const MIN_STROKE_ANGLE: f64 = 20.0;
const JITTER_THRESHOLD: f64 = 10.0;
const PAUSE_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Default)]
pub struct HoverDetector {
    stroke_start: Option<Point>,
    stroke_end: Option<Point>,
    activated: bool,
    pause_deadline: Option<Instant>,
    pause_position: Option<Point>,
}

impl HoverDetector {
    pub fn reset(&mut self, position: Option<Point>) {
        self.stroke_start = position;
        self.stroke_end = position;
        self.activated = false;
        self.pause_deadline = None;
        self.pause_position = None;
    }

    pub fn on_motion(&mut self, position: Point, now: Instant) -> Option<Point> {
        let Some(start) = self.stroke_start else {
            self.reset(Some(position));
            return None;
        };
        if !self.activated {
            if start.distance(position) <= ACTIVATION_DISTANCE {
                return None;
            }
            self.activated = true;
        }
        let end = self.stroke_end.unwrap_or(start);
        let stroke = Point {
            x: end.x - start.x,
            y: end.y - start.y,
        };
        let stroke_length = stroke.x.hypot(stroke.y);
        if stroke_length <= MIN_STROKE_LENGTH {
            self.stroke_end = Some(position);
            return None;
        }
        let tip = Point {
            x: position.x - end.x,
            y: position.y - end.y,
        };
        let tip_length = tip.x.hypot(tip.y);
        if tip_length > JITTER_THRESHOLD {
            self.pause_deadline = None;
            self.pause_position = None;
            let cosine = ((tip.x * stroke.x + tip.y * stroke.y) / (tip_length * stroke_length))
                .clamp(-1.0, 1.0);
            if cosine.acos().to_degrees() > MIN_STROKE_ANGLE {
                self.reset(Some(end));
                return Some(end);
            }
            self.stroke_end = Some(position);
        }
        if self.pause_deadline.is_none() {
            self.pause_deadline = Some(now + PAUSE_TIMEOUT);
            self.pause_position = Some(position);
        }
        None
    }

    pub fn on_timeout(&mut self, now: Instant) -> Option<Point> {
        if self.pause_deadline.is_none_or(|deadline| now < deadline) {
            return None;
        }
        let position = self.pause_position;
        self.reset(position);
        position
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_selects_end_of_long_stroke() {
        let start = Instant::now();
        let mut detector = HoverDetector::default();
        detector.reset(Some(Point { x: 0.0, y: 0.0 }));
        assert_eq!(detector.on_motion(Point { x: 20.0, y: 0.0 }, start), None);
        assert_eq!(detector.on_motion(Point { x: 180.0, y: 0.0 }, start), None);
        assert_eq!(detector.on_motion(Point { x: 181.0, y: 0.0 }, start), None);
        assert_eq!(
            detector.on_timeout(start + PAUSE_TIMEOUT),
            Some(Point { x: 181.0, y: 0.0 })
        );
    }

    #[test]
    fn turn_selects_previous_tip() {
        let now = Instant::now();
        let mut detector = HoverDetector::default();
        detector.reset(Some(Point { x: 0.0, y: 0.0 }));
        detector.on_motion(Point { x: 20.0, y: 0.0 }, now);
        detector.on_motion(Point { x: 180.0, y: 0.0 }, now);
        assert_eq!(
            detector.on_motion(Point { x: 140.0, y: 0.0 }, now),
            Some(Point { x: 180.0, y: 0.0 })
        );
    }
}
