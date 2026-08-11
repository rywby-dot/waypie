import unittest

from waypie_common import (
    colored_svg_source,
    computed_style,
    fixed_text_geometry,
    parse_item,
    resolve_angles,
    scaled_icon_size,
    sort_icon_themes,
    wrap_text_to_widths,
)


class FakeSvgPath:
    def __init__(self, source):
        self.source = source

    def read_text(self, encoding):
        self.encoding = encoding
        return self.source


class ScaledIconSizeTests(unittest.TestCase):
    def test_content_fill_can_exceed_one_hundred_percent(self):
        style = computed_style(
            {
                "circle": {
                    "icon-fill": "125%",
                    "text-fill": "150%",
                }
            },
            ("circle",),
        )

        self.assertEqual(style["icon-fill"], 1.25)
        self.assertEqual(style["text-fill"], 1.5)

    def test_icon_fill_uses_free_space_inside_border(self):
        style = {
            "icon-fill": 0.75,
            "icon-size": 10,
            "border-width": 4,
            "width": 80,
        }

        self.assertEqual(scaled_icon_size(style, 100), 69)

    def test_explicit_icon_size_follows_circle_scale(self):
        style = {"icon-size": 40, "width": 80}

        self.assertEqual(scaled_icon_size(style, 100), 50)

    def test_automatic_icon_size_uses_current_circle_size(self):
        style = {"icon-size": None, "width": 80}

        self.assertAlmostEqual(scaled_icon_size(style, 100), 55)

    def test_text_size_is_not_part_of_icon_scaling(self):
        style = {"icon-size": 40, "width": 80, "font-size": 14}

        scaled_icon_size(style, 120)

        self.assertEqual(style["font-size"], 14)


class IconThemeSortingTests(unittest.TestCase):
    def test_recently_selected_themes_come_first(self):
        themes = ["Zeta", "Alpha", "Beta"]
        history = {"Alpha": 10, "Beta": 20}

        self.assertEqual(
            sort_icon_themes(themes, history),
            ["Beta", "Alpha", "Zeta"],
        )

    def test_unused_themes_are_sorted_by_name(self):
        self.assertEqual(
            sort_icon_themes(["zeta", "Alpha", "beta"], {}),
            ["Alpha", "beta", "zeta"],
        )


class ColoredSvgSourceTests(unittest.TestCase):
    def test_current_color_uses_css_color(self):
        path = FakeSvgPath('<svg fill="currentColor"/>')

        source = colored_svg_source(path, (1.0, 1.0, 1.0, 1.0))

        self.assertEqual(source, '<svg fill="#ffffff"/>')

    def test_unstyled_monochrome_svg_gets_css_fill(self):
        path = FakeSvgPath('<svg><path d=""/></svg>')

        source = colored_svg_source(path, (1.0, 0.5, 0.0, 1.0))

        self.assertIn('fill="#ff8000"', source)

    def test_explicit_svg_color_is_preserved(self):
        source = '<svg><path fill="#123456"/></svg>'

        self.assertEqual(
            colored_svg_source(FakeSvgPath(source), (1.0, 1.0, 1.0, 1.0)),
            source,
        )


class EmptySubmenuTests(unittest.TestCase):
    def test_item_without_command_can_be_an_empty_submenu(self):
        root = parse_item(
            {
                "label": "Root",
                "items": [{"label": "Empty", "angle": 90}],
            },
            "menu",
            root=True,
        )

        resolve_angles(root, root=True)

        self.assertTrue(root.items[0].is_submenu)
        self.assertEqual(root.items[0].items, [])
        self.assertEqual(root.items[0].return_angle, 270)

    def test_action_command_cannot_be_empty(self):
        with self.assertRaises(SystemExit):
            parse_item(
                {"label": "Broken action", "command": ""},
                "menu.items[0]",
            )

    def test_unicode_quick_keys_are_preserved(self):
        item = parse_item(
            {"label": "Action", "command": "true", "keys": "фA7?"},
            "menu.items[0]",
        )

        self.assertEqual(item.keys, "фA7?")

    def test_sibling_quick_key_conflicts_ignore_case(self):
        with self.assertRaises(SystemExit):
            parse_item(
                {
                    "label": "Root",
                    "items": [
                        {"label": "One", "command": "true", "keys": "ж"},
                        {"label": "Two", "command": "true", "keys": "Ж"},
                    ],
                },
                "menu",
                root=True,
            )


class WrappedTextTests(unittest.TestCase):
    def test_words_are_kept_together_when_possible(self):
        self.assertEqual(
            wrap_text_to_widths("i like your dad", [6, 8], len),
            (["i like", "your dad"], True),
        )

    def test_long_word_is_split_across_lines(self):
        self.assertEqual(
            wrap_text_to_widths("LibreOffice", [8, 8], len),
            (["LibreOff", "ice"], True),
        )

    def test_overflow_uses_three_dots_and_preserves_last_three_characters(self):
        self.assertEqual(
            wrap_text_to_widths(
                "DJI_20260528185336_0271_D_RYW.MP4",
                [8, 8, 8],
                len,
            ),
            (["DJI_2026", "05281853", "36...MP4"], False),
        )

    def test_text_layout_does_not_grow_with_circle(self):
        style = {"width": 80}

        self.assertEqual(fixed_text_geometry(style, 120, 1), (80, 1))

    def test_smaller_circle_scales_fixed_text_layout(self):
        style = {"width": 80}

        self.assertEqual(fixed_text_geometry(style, 60, 1), (80, 0.75))


if __name__ == "__main__":
    unittest.main()
