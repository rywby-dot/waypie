use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub damping_ratio: f64,
    pub stiffness: f64,
    pub epsilon: f64,
}

impl Default for Spring {
    fn default() -> Self {
        Self {
            damping_ratio: 1.0,
            stiffness: 1000.0,
            epsilon: 0.0001,
        }
    }
}

impl Spring {
    pub fn duration(self) -> Duration {
        self.checked_duration()
            .expect("spring parameters must produce a finite duration")
    }

    pub fn checked_duration(self) -> Option<Duration> {
        let seconds = settling_window(self.damping_ratio, self.epsilon) / self.stiffness.sqrt();
        Duration::try_from_secs_f64(seconds).ok()
    }

    pub fn sample(self, progress: f64) -> f64 {
        let progress = progress.clamp(0.0, 1.0);
        if progress == 0.0 || progress == 1.0 {
            return progress;
        }
        let damping = self.damping_ratio;
        let elapsed = progress * settling_window(damping, self.epsilon);
        let value = if damping < 1.0 {
            let root = (1.0 - damping * damping).sqrt();
            let damped = root;
            let decay = (-damping * elapsed).exp();
            1.0 - decay * ((damped * elapsed).cos() + damping / root * (damped * elapsed).sin())
        } else if (damping - 1.0).abs() < f64::EPSILON {
            let decay = (-elapsed).exp();
            1.0 - (1.0 + elapsed) * decay
        } else {
            let root = (damping * damping - 1.0).sqrt();
            let first = -(damping - root);
            let second = -(damping + root);
            let first_weight = -second / (second - first);
            let second_weight = first / (second - first);
            1.0 - first_weight * (first * elapsed).exp() - second_weight * (second * elapsed).exp()
        };
        if (1.0 - value).abs() <= self.epsilon {
            1.0
        } else {
            value
        }
    }
}

fn settling_window(damping: f64, epsilon: f64) -> f64 {
    let error = |time: f64| {
        if damping < 1.0 {
            (-damping * time).exp() / (1.0 - damping * damping).sqrt()
        } else if (damping - 1.0).abs() < f64::EPSILON {
            (1.0 + time) * (-time).exp()
        } else {
            let root = (damping * damping - 1.0).sqrt();
            let first = -(damping - root);
            let second = -(damping + root);
            let first_weight = -second / (second - first);
            let second_weight = first / (second - first);
            (first_weight * (first * time).exp() + second_weight * (second * time).exp()).abs()
        }
    };
    let mut low = 0.0;
    let mut high = 1.0;
    while error(high) > epsilon && high < 1_000_000.0 {
        high *= 2.0;
    }
    for _ in 0..64 {
        let middle = (low + high) / 2.0;
        if error(middle) > epsilon {
            low = middle;
        } else {
            high = middle;
        }
    }
    high
}

pub fn smoothstep(progress: f64) -> f64 {
    let progress = progress.clamp(0.0, 1.0);
    progress * progress * (3.0 - 2.0 * progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn underdamped_spring_can_overshoot() {
        let spring = Spring {
            damping_ratio: 0.5,
            stiffness: 100.0,
            epsilon: 0.000001,
        };
        assert!((0..100).any(|step| spring.sample(step as f64 / 100.0) > 1.0));
    }

    #[test]
    fn default_spring_uses_the_complete_normalized_timeline() {
        let spring = Spring::default();
        assert!(spring.sample(0.25) < 0.9);
        assert!(spring.sample(0.9) > 0.99);
    }

    #[test]
    fn spring_duration_is_derived_from_its_physical_parameters() {
        let normal = Spring::default();
        let stiff = Spring {
            stiffness: normal.stiffness * 4.0,
            ..normal
        };
        let ratio = normal.duration().as_secs_f64() / stiff.duration().as_secs_f64();
        assert!((ratio - 2.0).abs() < 0.001);
    }

    #[test]
    fn extremely_small_stiffness_is_detected_without_panicking() {
        let spring = Spring {
            stiffness: f64::MIN_POSITIVE,
            ..Spring::default()
        };

        assert_eq!(spring.checked_duration(), None);
    }
}
