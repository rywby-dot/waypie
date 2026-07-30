import math
from dataclasses import dataclass

# Hover Mode tuning. These defaults match Kando's gesture detector.
HOVER_ACTIVATION_DISTANCE = 15.0
HOVER_MIN_STROKE_LENGTH = 150.0
HOVER_MIN_STROKE_ANGLE = 20.0
HOVER_JITTER_THRESHOLD = 10.0
HOVER_PAUSE_TIMEOUT = 0.100


@dataclass(frozen=True)
class HoverSelection:
    x: float
    y: float


class HoverGestureDetector:
    """Detect a pause or a turn at the end of a sufficiently long pointer stroke."""

    def __init__(
        self,
        activation_distance=HOVER_ACTIVATION_DISTANCE,
        min_stroke_length=HOVER_MIN_STROKE_LENGTH,
        min_stroke_angle=HOVER_MIN_STROKE_ANGLE,
        jitter_threshold=HOVER_JITTER_THRESHOLD,
        pause_timeout=HOVER_PAUSE_TIMEOUT,
    ):
        self.activation_distance = activation_distance
        self.min_stroke_length = min_stroke_length
        self.min_stroke_angle = min_stroke_angle
        self.jitter_threshold = jitter_threshold
        self.pause_timeout = pause_timeout
        self.reset()

    def reset(self, position=None):
        self.stroke_start = position
        self.stroke_end = position
        self.activated = False
        self.pause_deadline = None
        self.pause_position = None

    def on_motion(self, position, now):
        if self.stroke_start is None:
            self.reset(position)
            return None
        if not self.activated:
            if math.dist(position, self.stroke_start) <= self.activation_distance:
                return None
            self.activated = True

        stroke_x = self.stroke_end[0] - self.stroke_start[0]
        stroke_y = self.stroke_end[1] - self.stroke_start[1]
        stroke_length = math.hypot(stroke_x, stroke_y)

        if stroke_length <= self.min_stroke_length:
            self.stroke_end = position
            return None

        tip_x = position[0] - self.stroke_end[0]
        tip_y = position[1] - self.stroke_end[1]
        tip_length = math.hypot(tip_x, tip_y)
        if tip_length > self.jitter_threshold:
            self.pause_deadline = None
            self.pause_position = None
            cosine = (tip_x * stroke_x + tip_y * stroke_y) / (
                tip_length * stroke_length
            )
            turn_angle = math.degrees(math.acos(max(-1.0, min(1.0, cosine))))
            if turn_angle > self.min_stroke_angle:
                selection = HoverSelection(*self.stroke_end)
                self.reset(self.stroke_end)
                return selection
            self.stroke_end = position

        if self.pause_deadline is None:
            self.pause_deadline = now + self.pause_timeout
            self.pause_position = position
        return None

    def on_timeout(self, now):
        if self.pause_deadline is None or now < self.pause_deadline:
            return None
        selection = HoverSelection(*self.pause_position)
        self.reset(self.pause_position)
        return selection
