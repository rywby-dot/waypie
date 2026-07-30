import unittest

from waypie_hover import HoverGestureDetector, HoverSelection


class HoverGestureDetectorTests(unittest.TestCase):
    def detector(self):
        return HoverGestureDetector(
            activation_distance=15,
            min_stroke_length=150,
            min_stroke_angle=20,
            jitter_threshold=10,
            pause_timeout=0.1,
        )

    def test_short_motion_does_not_activate_hover_mode(self):
        detector = self.detector()
        detector.reset((0, 0))

        self.assertIsNone(detector.on_motion((15, 0), 0.0))
        self.assertFalse(detector.activated)
        self.assertIsNone(detector.pause_deadline)

    def test_pause_after_long_stroke_selects_pointer_position(self):
        detector = self.detector()
        detector.reset((0, 0))
        detector.on_motion((20, 0), 0.0)
        detector.on_motion((170, 0), 0.01)
        detector.on_motion((171, 0), 0.02)

        self.assertIsNone(detector.on_timeout(0.119))
        self.assertEqual(
            detector.on_timeout(0.121),
            HoverSelection(171, 0),
        )

    def test_sharp_turn_selects_previous_stroke_end(self):
        detector = self.detector()
        detector.reset((0, 0))
        detector.on_motion((20, 0), 0.0)
        detector.on_motion((170, 0), 0.01)

        self.assertEqual(
            detector.on_motion((170, 20), 0.02),
            HoverSelection(170, 0),
        )

    def test_jitter_does_not_move_the_stroke_tip(self):
        detector = self.detector()
        detector.reset((0, 0))
        detector.on_motion((20, 0), 0.0)
        detector.on_motion((170, 0), 0.01)
        detector.on_motion((175, 0), 0.02)

        self.assertEqual(detector.stroke_end, (170, 0))


if __name__ == "__main__":
    unittest.main()
