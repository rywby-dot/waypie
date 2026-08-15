import tempfile
import unittest
from pathlib import Path

from waypie_common import (
    Item,
    animation_duration,
    animations_off,
    autogenerate_keys,
    centered_submenu_angles,
    colored_svg_source,
    computed_item_key_style,
    computed_style,
    fixed_text_geometry,
    geometric_return_angle,
    load_config,
    load_styles,
    menus_breadth_first,
    parse_color,
    parse_item,
    parse_key_sets,
    resolve_angles,
    scaled_icon_size,
    sort_icon_themes,
    spring_duration,
    spring_value,
    wrap_text_to_widths,
)


class AlternatePathTests(unittest.TestCase):
    def test_bundled_configs_are_valid_for_the_configurator(self):
        root = Path(__file__).resolve().parents[1]
        configs = sorted(
            path for path in (root / "config").glob("config*") if path.is_file()
        )
        self.assertTrue(configs)
        for path in configs:
            load_config(path)

    def test_loaders_use_the_explicit_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            config_path = directory / "alternate.toml"
            style_path = directory / "alternate.css"
            config_path.write_text(
                'center-hitbox-size = 100\n[menu]\nlabel = "Alternate"\n',
                encoding="utf-8",
            )
            style_path.write_text(
                "circle { width: 77px; }\n",
                encoding="utf-8",
            )

            settings = load_config(config_path)
            styles = load_styles(style_path)

            self.assertEqual(settings.root.label, "Alternate")
            self.assertEqual(styles["circle"]["width"], "77px")

    def test_unknown_config_fields_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "config"
            path.write_text(
                "typo-mode = true\n[menu]\nlabel = 'Root'\n",
                encoding="utf-8",
            )

            with self.assertRaises(SystemExit):
                load_config(path)


class FakeSvgPath:
    def __init__(self, source):
        self.source = source

    def read_text(self, encoding):
        self.encoding = encoding
        return self.source


class AnimationStyleTests(unittest.TestCase):
    def test_off_overrides_timed_and_spring_animations(self):
        rules = {
            "animation": {
                "off": "true",
                "color-duration": "999ms",
                "hover-stiffness": "1",
            }
        }

        self.assertTrue(animations_off(rules))
        self.assertEqual(animation_duration(rules, "color-duration"), 0)
        self.assertEqual(spring_duration(rules, "hover"), 0)
        self.assertEqual(spring_value(rules, "hover", 0.5), 1)


class StyleValidationTests(unittest.TestCase):
    def test_bundled_styles_are_valid_for_the_configurator(self):
        root = Path(__file__).resolve().parents[1]
        styles = sorted((root / "config").glob("style*.css"))
        self.assertTrue(styles)
        for path in styles:
            load_styles(path)

    def test_circle_font_weight_supports_names_and_numbers(self):
        rules = {
            "circle": {"font-weight": "normal"},
            "circle.active": {"font-weight": "750"},
        }

        self.assertEqual(computed_style(rules, ("circle",))["font-weight"], 400)
        self.assertEqual(
            computed_style(rules, ("circle", "circle.active"))["font-weight"],
            750,
        )

    def test_item_key_font_weight_is_validated(self):
        rules = {
            "item-key": {"font-weight": "500"},
            "item-key.active": {"font-weight": "bold"},
        }

        self.assertEqual(computed_item_key_style(rules)["font-weight"], 500)
        self.assertEqual(computed_item_key_style(rules, True)["font-weight"], 700)

    def test_bare_off_keeps_the_following_item_key_property(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "style.css"
            path.write_text(
                "circle { width: 70px; } item-key {\n off\n font-weight: 450; }",
                encoding="utf-8",
            )

            rules = load_styles(path)

        style = computed_item_key_style(rules)
        self.assertFalse(style["enabled"])
        self.assertEqual(style["font-weight"], 450)

    def test_non_finite_function_colors_are_rejected(self):
        with self.assertRaises(SystemExit):
            parse_color("rgb(nan, 0, 0)", "color")

    def test_scientific_css_numbers_match_the_runtime_parser(self):
        style = computed_style(
            {"circle": {"width": "7e1px", "text-fill": "1.25e2%"}},
            ("circle",),
        )

        self.assertEqual(style["width"], 70)
        self.assertEqual(style["text-fill"], 1.25)

    def test_invalid_active_rule_is_rejected_when_styles_are_loaded(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "style.css"
            path.write_text(
                "circle { width: 70px; } circle.active { opacity: 2; }",
                encoding="utf-8",
            )

            with self.assertRaises(SystemExit):
                load_styles(path)

    def test_malformed_style_syntax_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "style.css"
            path.write_text("circle { width 70px; }", encoding="utf-8")

            with self.assertRaises(SystemExit):
                load_styles(path)

    def test_unknown_style_property_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "style.css"
            path.write_text(
                "circle { width: 70px; opacitty: 1; }",
                encoding="utf-8",
            )

            with self.assertRaises(SystemExit):
                load_styles(path)


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
    def test_geometric_return_direction_does_not_depend_on_child_gaps(self):
        submenu = Item(
            "Submenu",
            angle=90,
            items=[
                Item("First", command="true", angle=10),
                Item("Second", command="true", angle=40),
            ],
        )

        self.assertEqual(geometric_return_angle(submenu), 270)
        self.assertEqual(centered_submenu_angles(submenu), (270, [30, 150]))

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


class QuickKeyGenerationTests(unittest.TestCase):
    def test_root_is_numbered_clockwise_from_zero(self):
        root = Item(
            "Root",
            items=[
                Item("Last", command="true", angle=300),
                Item("First", command="true", angle=0),
                Item("Second", command="true", angle=80),
            ],
        )

        autogenerate_keys(root, "aф rы sв")

        self.assertEqual(
            {item.label: item.keys for item in root.items},
            {"First": "aф", "Second": "rы", "Last": "sв"},
        )

    def test_submenu_is_numbered_clockwise_from_return_connector(self):
        submenu = Item(
            "Submenu",
            angle=90,
            items=[
                Item("Third", command="true", angle=180),
                Item("First", command="true", angle=280),
                Item("Second", command="true", angle=20),
            ],
        )
        root = Item("Root", items=[submenu])

        autogenerate_keys(root, "1 2 3")

        self.assertEqual(submenu.return_angle, 100)
        self.assertEqual(
            {item.label: item.keys for item in submenu.items},
            {"Third": "1", "First": "2", "Second": "3"},
        )

    def test_missing_submenu_angles_match_runtime_distribution(self):
        submenu = Item(
            "Submenu",
            angle=90,
            items=[
                Item("First", command="true"),
                Item("Second", command="true"),
                Item("Third", command="true"),
            ],
        )
        root = Item("Root", items=[submenu])

        resolve_angles(root, root=True)

        self.assertEqual([item.angle for item in submenu.items], [0, 120, 240])
        self.assertEqual(submenu.return_angle, 300)

    def test_missing_sets_clear_unassigned_circle_keys(self):
        root = Item(
            "Root",
            items=[
                Item("First", keys="old", command="true", angle=0),
                Item("Second", keys="stale", command="true", angle=180),
            ],
        )

        autogenerate_keys(root, "new")

        self.assertEqual([item.keys for item in root.items], ["new", ""])

    def test_key_sets_reject_conflicts_between_positions(self):
        with self.assertRaises(SystemExit):
            parse_key_sets("aф rФ")

    def test_every_nested_menu_is_generated(self):
        deepest = Item(
            "Deepest",
            angle=180,
            items=[Item("Deep action", command="true", angle=45)],
        )
        middle = Item("Middle", angle=90, items=[deepest])
        root = Item("Root", items=[middle])

        count = autogenerate_keys(root, "aф")

        self.assertEqual(count, 3)
        self.assertEqual(middle.keys, "aф")
        self.assertEqual(deepest.keys, "aф")
        self.assertEqual(deepest.items[0].keys, "aф")

    def test_menu_traversal_is_breadth_first(self):
        nested = Item("Nested", items=[])
        first = Item("First", items=[nested])
        second = Item("Second", items=[])
        root = Item("Root", items=[first, second])

        self.assertEqual(
            [menu.label for menu in menus_breadth_first(root)],
            ["Root", "First", "Second", "Nested"],
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
