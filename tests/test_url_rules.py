"""URL rules, set with `restrict_resource_urls`.

The rules are process-wide and can only be set before the first `RenderPool` is created, which
the other tests do in this process, so each test runs its code in a new Python process.
"""

import json
import subprocess
import sys
import textwrap
from pathlib import Path

BOX = {
    "type": "Polygon",
    "coordinates": [[[-10, -10], [10, -10], [10, 10], [-10, 10], [-10, -10]]],
}

PRELUDE = """
import json
import sys

import mapli

BOX = {
    "type": "Polygon",
    "coordinates": [[[-10, -10], [10, -10], [10, 10], [-10, 10], [-10, -10]]],
}
VIEW = {"lon": 0.0, "lat": 0.0, "zoom": 0.0, "width": 64, "height": 64}


def build_geojson_style(data):
    return {
        "version": 8,
        "sources": {"box": {"type": "geojson", "data": data}},
        "layers": [
            {"id": "bg", "type": "background", "paint": {"background-color": "#fff"}},
            {"id": "box", "type": "fill", "source": "box", "paint": {"fill-color": "#f00"}},
        ],
    }
"""


def run_in_new_process(code: str, *args: str) -> None:
    """Run `code`, after `PRELUDE`, in a new Python process, which must succeed."""
    script = PRELUDE + textwrap.dedent(code)
    result = subprocess.run(
        [sys.executable, "-c", script, *args],
        check=False,
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert result.returncode == 0, result.stderr


def test_urls_outside_the_allowed_prefixes_are_denied(tmp_path: Path) -> None:
    allowed_dir = tmp_path / "allowed"
    other_dir = tmp_path / "other"
    for directory in (allowed_dir, other_dir):
        directory.mkdir()
        (directory / "box.geojson").write_text(json.dumps(BOX))

    run_in_new_process(
        """
        allowed_dir, other_dir = sys.argv[1:]
        requested = []

        def serve_box(request):
            requested.append(request.url)
            return json.dumps(BOX).encode()

        mapli.register_scheme_handler("memory", serve_box)
        mapli.restrict_resource_urls(
            allowed_url_prefixes=["memory://data/allowed/", f"file://{allowed_dir}/"]
        )
        pool = mapli.RenderPool()
        pool.register_style("inline", json=build_geojson_style(BOX))
        expected = pool.render("inline", **VIEW)

        def assert_url_renders(data_url):
            pool.register_style("style", json=build_geojson_style(data_url))
            assert pool.render("style", **VIEW) == expected, data_url

        def assert_url_denied(data_url):
            pool.register_style("style", json=build_geojson_style(data_url))
            try:
                pool.render("style", **VIEW)
            except mapli.RenderError as error:
                assert "access denied by URL rules" in str(error), error
            else:
                raise AssertionError(f"{data_url} was not denied")

        assert_url_renders("memory://data/allowed/box.geojson")
        assert_url_denied("memory://data/denied/box.geojson")
        assert_url_denied("memory://data/allowed/../denied/box.geojson")
        assert requested == ["memory://data/allowed/box.geojson"], requested

        assert_url_renders(f"file://{allowed_dir}/box.geojson")
        assert_url_denied(f"file://{other_dir}/box.geojson")
        assert_url_denied(f"file://{allowed_dir}/../other/box.geojson")
        assert_url_denied("https://example.org/box.geojson")

        # the style itself is checked too
        pool.register_style("from-url", url="memory://data/denied/style.json")
        try:
            pool.render("from-url", **VIEW)
        except mapli.StyleLoadError as error:
            assert "access denied by URL rules" in str(error), error
        else:
            raise AssertionError("the style was not denied")
        """,
        str(allowed_dir),
        str(other_dir),
    )


def test_rules_are_closed_once_a_pool_exists() -> None:
    run_in_new_process(
        """
        mapli.RenderPool()
        try:
            mapli.restrict_resource_urls(["memory://"])
        except mapli.UrlRulesClosedError as error:
            assert "render pool" in str(error), error
        else:
            raise AssertionError("the rules were set after a pool was created")
        """
    )


def test_rules_can_only_be_set_once() -> None:
    run_in_new_process(
        """
        # refused prefixes do not set the rules
        for prefixes in (["memory://", "not a prefix"], "memory://"):
            try:
                mapli.restrict_resource_urls(prefixes)
            except (ValueError, TypeError):
                pass
            else:
                raise AssertionError(f"{prefixes!r} was accepted")

        mapli.restrict_resource_urls(["memory://"])
        try:
            mapli.restrict_resource_urls(["memory://", "https://"])
        except mapli.UrlRulesClosedError as error:
            assert "already been set" in str(error), error
        else:
            raise AssertionError("the rules were set twice")
        """
    )
