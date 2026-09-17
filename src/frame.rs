//! Coalesce scene changes while a committed frame awaits the compositor.
//!
//! Input and animation clocks are independent of this gate. A failed render
//! keeps its request pending; only a successful buffer commit consumes it.

pub(crate) struct FrameSchedule<S> {
    dirty: bool,
    pending_surface: Option<S>,
}

impl<S> Default for FrameSchedule<S> {
    fn default() -> Self {
        Self {
            dirty: false,
            pending_surface: None,
        }
    }
}

impl<S: PartialEq> FrameSchedule<S> {
    pub fn request(&mut self) {
        self.dirty = true;
    }

    pub fn ready(&self) -> bool {
        self.dirty && self.pending_surface.is_none()
    }

    pub fn submitted(&mut self, surface: S) {
        self.dirty = false;
        self.pending_surface = Some(surface);
    }

    pub fn frame_done(&mut self, surface: &S) {
        if self.pending_surface.as_ref() == Some(surface) {
            self.pending_surface = None;
        }
    }

    pub fn reset(&mut self) {
        self.dirty = false;
        self.pending_surface = None;
    }
}

#[cfg(test)]
mod tests {
    use super::FrameSchedule;

    #[test]
    fn first_frame_is_immediate_and_requests_are_coalesced() {
        let mut frames = FrameSchedule::<u32>::default();
        assert!(!frames.ready());
        frames.request();
        assert!(frames.ready());
        frames.submitted(1);
        for _ in 0..1000 {
            frames.request();
            assert!(!frames.ready());
        }
        frames.frame_done(&1);
        assert!(frames.ready());
        frames.submitted(1);
        frames.frame_done(&1);
        assert!(!frames.ready());
    }

    #[test]
    fn callback_before_input_does_not_lose_the_next_request() {
        let mut frames = FrameSchedule::<u32>::default();
        frames.request();
        frames.submitted(1);
        frames.frame_done(&1);
        frames.request();
        assert!(frames.ready());
    }

    #[test]
    fn failed_render_remains_ready_for_buffer_release() {
        let mut frames = FrameSchedule::<u32>::default();
        frames.request();
        assert!(frames.ready());
        // No submitted() on an unavailable SHM slot or failed attachment.
        assert!(frames.ready());
    }

    #[test]
    fn callbacks_from_other_surfaces_cannot_unlock_a_frame() {
        let mut frames = FrameSchedule::<u32>::default();
        frames.request();
        frames.submitted(1);
        frames.request();
        frames.frame_done(&2);
        assert!(!frames.ready());
        frames.reset();
        assert!(!frames.ready());
        frames.request();
        assert!(frames.ready());
        frames.submitted(2);
        frames.request();
        frames.frame_done(&1);
        assert!(!frames.ready());
        frames.frame_done(&2);
        assert!(frames.ready());
    }
}
