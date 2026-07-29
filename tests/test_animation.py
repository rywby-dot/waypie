import unittest
from itertools import pairwise

from waypie_animation import (
    CloseAnimation,
    NavigationAnimation,
    ScalarAnimation,
    Timeline,
    spring,
)


class SpringTests(unittest.TestCase):
    def test_spring_is_normalized_and_monotonic(self):
        values = [spring(index / 100) for index in range(101)]

        self.assertEqual(values[0], 0)
        self.assertEqual(values[-1], 1)
        self.assertTrue(all(left <= right for left, right in pairwise(values)))


class NavigationAnimationTests(unittest.TestCase):
    def test_updated_target_is_followed_without_restarting_progress(self):
        animation = NavigationAnimation(
            Timeline(started=10, duration=2),
            reveal_from=0,
            reveal_to=1,
            start_centers=((0, 0),),
        )

        first = animation.frame(11, ((100, 0),))
        moved_target = animation.frame(11, ((200, 0),))

        self.assertEqual(first.progress, moved_target.progress)
        self.assertEqual(first.eased, moved_target.eased)
        self.assertAlmostEqual(
            moved_target.centers[0][0],
            first.centers[0][0] * 2,
        )

    def test_navigation_animation_finishes_at_current_target(self):
        animation = NavigationAnimation(
            Timeline(started=0, duration=1),
            reveal_from=0,
            reveal_to=1,
            start_centers=((25, 25),),
        )

        frame = animation.frame(1, ((300, 400),))

        self.assertTrue(frame.done)
        self.assertEqual(frame.reveal, 1)
        self.assertEqual(frame.centers, ((300, 400),))


class CloseAnimationTests(unittest.TestCase):
    def test_close_and_menu_timelines_advance_independently(self):
        navigation = NavigationAnimation(
            Timeline(started=0, duration=2),
            reveal_from=0,
            reveal_to=1,
            start_centers=((0, 0),),
        )
        close = CloseAnimation(Timeline(started=0.5, duration=1), has_action=False)

        navigation_before_close = navigation.frame(0.5, ((100, 0),))
        navigation_during_close = navigation.frame(1, ((200, 0),))
        close_frame = close.frame(1)

        self.assertGreater(
            navigation_during_close.progress,
            navigation_before_close.progress,
        )
        self.assertFalse(navigation_during_close.done)
        self.assertFalse(close_frame.done)

    def test_finished_close_does_not_finish_longer_navigation(self):
        navigation = NavigationAnimation(
            Timeline(started=0, duration=2),
            reveal_from=0,
            reveal_to=1,
            start_centers=((0, 0),),
        )
        close = CloseAnimation(Timeline(started=0.5, duration=0.5), has_action=False)

        navigation_frame = navigation.frame(1, ((200, 0),))
        close_frame = close.frame(1)

        self.assertFalse(navigation_frame.done)
        self.assertTrue(close_frame.done)

    def test_action_fades_only_after_reaching_pointer(self):
        animation = CloseAnimation(Timeline(started=0, duration=3), has_action=True)

        before_arrival = animation.frame(1)
        at_arrival = animation.frame(2)
        finished = animation.frame(3)

        self.assertLess(before_arrival.action_position, 1)
        self.assertEqual(before_arrival.action_opacity, 1)
        self.assertEqual(at_arrival.action_position, 1)
        self.assertEqual(at_arrival.action_opacity, 1)
        self.assertEqual(finished.action_opacity, 0)
        self.assertEqual(finished.action_scale, 1)

    def test_scene_opacity_finishes_before_scene_scale(self):
        animation = CloseAnimation(Timeline(started=0, duration=1), has_action=False)

        frame = animation.frame(0.8)

        self.assertEqual(frame.opacity, 0)
        self.assertGreater(frame.scale, 0)


class ScalarAnimationTests(unittest.TestCase):
    def test_retargeting_can_continue_from_current_value(self):
        first = ScalarAnimation(0, 1, Timeline(started=0, duration=1))
        current, _done = first.frame(0.4)
        retargeted = ScalarAnimation(current, 0, Timeline(started=0.4, duration=1))

        value, _done = retargeted.frame(0.4)

        self.assertEqual(value, current)


if __name__ == "__main__":
    unittest.main()
