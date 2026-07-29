import math
from dataclasses import dataclass


def spring(progress):
    """Critically damped spring progress normalized to exactly 0..1."""
    progress = min(1.0, max(0.0, progress))
    if progress in {0.0, 1.0}:
        return progress
    stiffness = 9
    value = 1 - (1 + stiffness * progress) * math.exp(-stiffness * progress)
    end = 1 - (1 + stiffness) * math.exp(-stiffness)
    return value / end


@dataclass(frozen=True)
class TimelineFrame:
    progress: float
    eased: float
    done: bool


@dataclass(frozen=True)
class Timeline:
    started: float
    duration: float

    def frame(self, now):
        progress = min(1.0, max(0.0, (now - self.started) / self.duration))
        return TimelineFrame(progress, spring(progress), progress >= 1.0)


@dataclass(frozen=True)
class NavigationFrame:
    progress: float
    eased: float
    reveal: float
    centers: tuple
    done: bool


@dataclass(frozen=True)
class NavigationAnimation:
    timeline: Timeline
    reveal_from: float
    reveal_to: float
    start_centers: tuple

    def frame(self, now, target_centers):
        timeline = self.timeline.frame(now)
        reveal = self.reveal_from + (self.reveal_to - self.reveal_from) * timeline.eased
        centers = tuple(
            (
                start_x + (target_x - start_x) * timeline.eased,
                start_y + (target_y - start_y) * timeline.eased,
            )
            for (start_x, start_y), (target_x, target_y) in zip(
                self.start_centers,
                target_centers,
                strict=True,
            )
        )
        return NavigationFrame(
            timeline.progress,
            timeline.eased,
            reveal,
            centers,
            timeline.done,
        )


@dataclass(frozen=True)
class CloseFrame:
    scale: float
    opacity: float
    action_position: float
    action_scale: float
    action_opacity: float
    done: bool


@dataclass(frozen=True)
class CloseAnimation:
    timeline: Timeline
    has_action: bool

    OPACITY_END = 0.8
    ACTION_FLIGHT_END = 2 / 3

    def frame(self, now):
        timeline = self.timeline.frame(now)
        scale = max(0.0, 1 - timeline.eased)
        opacity = max(
            0.0,
            1 - spring(min(1.0, timeline.progress / self.OPACITY_END)),
        )
        if not self.has_action:
            return CloseFrame(scale, opacity, 1.0, 1.0, 1.0, timeline.done)

        action_position = spring(min(1.0, timeline.progress / self.ACTION_FLIGHT_END))
        action_fade = max(
            0.0,
            min(
                1.0,
                (timeline.progress - self.ACTION_FLIGHT_END)
                / (1 - self.ACTION_FLIGHT_END),
            ),
        )
        return CloseFrame(
            scale,
            opacity,
            action_position,
            timeline.eased,
            max(0.0, 1 - spring(action_fade)),
            timeline.done,
        )


@dataclass(frozen=True)
class ScalarAnimation:
    start: float
    target: float
    timeline: Timeline

    def frame(self, now):
        timeline = self.timeline.frame(now)
        value = self.start + (self.target - self.start) * timeline.eased
        return value, timeline.done
