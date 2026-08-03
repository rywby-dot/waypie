#[derive(Clone, Copy, Debug)]
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
    pub fn sample(self, progress: f64, duration: f64) -> f64 {
        let progress = progress.clamp(0.0, 1.0);
        if progress == 0.0 || progress == 1.0 {
            return progress;
        }
        let elapsed = progress * duration;
        let damping = self.damping_ratio;
        let frequency = self.stiffness.sqrt();
        let (value, velocity) = if damping < 1.0 {
            let root = (1.0 - damping * damping).sqrt();
            let damped = frequency * root;
            let decay = (-damping * frequency * elapsed).exp();
            (
                1.0 - decay
                    * ((damped * elapsed).cos() + damping / root * (damped * elapsed).sin()),
                frequency / root * decay * (damped * elapsed).sin(),
            )
        } else if damping == 1.0 {
            let decay = (-frequency * elapsed).exp();
            (
                1.0 - (1.0 + frequency * elapsed) * decay,
                frequency * frequency * elapsed * decay,
            )
        } else {
            let root = (damping * damping - 1.0).sqrt();
            let first = -frequency * (damping - root);
            let second = -frequency * (damping + root);
            let first_weight = -second / (second - first);
            let second_weight = first / (second - first);
            let first_decay = (first * elapsed).exp();
            let second_decay = (second * elapsed).exp();
            (
                1.0 - first_weight * first_decay - second_weight * second_decay,
                -first_weight * first * first_decay - second_weight * second * second_decay,
            )
        };
        if (1.0 - value).abs() <= self.epsilon && (velocity / frequency).abs() <= self.epsilon {
            1.0
        } else {
            value
        }
    }
}

pub fn smoothstep(progress: f64) -> f64 {
    let progress = progress.clamp(0.0, 1.0);
    progress * progress * (3.0 - 2.0 * progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn underdamped_spring_overshoots() {
        let spring = Spring {
            damping_ratio: 0.5,
            stiffness: 100.0,
            epsilon: 0.000001,
        };
        assert!((0..100).any(|step| spring.sample(step as f64 / 100.0, 1.0) > 1.0));
    }
}
