"""Render styles submitted by users, which may only fetch URLs from an allow list.

The application serves its own GeoJSON layers through the `layers://` scheme, and lets users
submit styles that draw them over the MapLibre demo tiles. Some of the submitted styles try to
reach data they should not: a local file, a cloud metadata endpoint, and a private layer of the
application. `mapli.restrict_resource_urls` denies these, so their renders fail instead.
"""

import json
from pathlib import Path
from typing import Any

import mapli

LAYERS: dict[str, dict[str, Any]] = {
    "layers://app/overlays/cities.geojson": {
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "properties": {"name": name},
                "geometry": {"type": "Point", "coordinates": coordinates},
            }
            for name, coordinates in {
                "Lisbon": [-9.14, 38.72],
                "Porto": [-8.61, 41.15],
                "Madrid": [-3.70, 40.42],
                "Paris": [2.35, 48.86],
            }.items()
        ],
    },
    # served by the same handler, but not meant for user styles
    "layers://app/private/customers.geojson": {
        "type": "FeatureCollection",
        "features": [],
    },
}


def serve_layer(request: mapli.ResourceRequest) -> mapli.Response:
    try:
        layer = LAYERS[request.url]
    except KeyError:
        return mapli.Response.error(mapli.ErrorReason.NOT_FOUND, "no such layer")
    return mapli.Response(json.dumps(layer).encode())


def build_user_style(overlay_url: str) -> dict[str, Any]:
    """A style a user could submit: the demo tiles' countries, with circles from `overlay_url`."""
    return {
        "version": 8,
        "sources": {
            "basemap": {
                "type": "vector",
                "url": "https://demotiles.maplibre.org/tiles/tiles.json",
            },
            "overlay": {"type": "geojson", "data": overlay_url},
        },
        "layers": [
            {
                "id": "background",
                "type": "background",
                "paint": {"background-color": "#d8e8f0"},
            },
            {
                "id": "countries",
                "type": "fill",
                "source": "basemap",
                "source-layer": "countries",
                "paint": {"fill-color": "#f4f1ea", "fill-outline-color": "#999"},
            },
            {
                "id": "overlay",
                "type": "circle",
                "source": "overlay",
                "paint": {"circle-color": "#c0392b", "circle-radius": 6},
            },
        ],
    }


SUBMITTED_STYLES = {
    "cities": build_user_style("layers://app/overlays/cities.geojson"),
    "local-file": build_user_style("file:///etc/passwd"),
    "cloud-metadata": build_user_style("http://169.254.169.254/latest/meta-data/"),
    "private-layer": build_user_style(
        "layers://app/overlays/../private/customers.geojson"
    ),
}


def main() -> None:
    mapli.register_scheme_handler("layers", serve_layer)
    # must happen before the first RenderPool is created
    mapli.restrict_resource_urls(
        allowed_url_prefixes=[
            "https://demotiles.maplibre.org/",
            "layers://app/overlays/",
        ]
    )

    pool = mapli.RenderPool()
    for name, style in SUBMITTED_STYLES.items():
        pool.register_style(name, json=style)
        try:
            png_bytes = pool.render(name, lon=-3, lat=43, zoom=4, width=512, height=512)
        except mapli.RenderError as error:
            print(f"{name}: refused: {error}")
            continue
        Path(f"{name}.png").write_bytes(png_bytes)
        print(f"{name}: wrote {name}.png ({len(png_bytes)} bytes)")


if __name__ == "__main__":
    main()
