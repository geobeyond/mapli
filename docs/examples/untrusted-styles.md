# Untrusted styles

Renders styles submitted by users, with `mapli.restrict_resource_urls` limiting what they can
fetch, see [Restricting URLs](../user-guide/restricting-urls.md).

The application serves its own GeoJSON layers through a `layers://` scheme handler, and allows
only two URL prefixes: the MapLibre demo tiles, and the overlay layers that users may draw.
Four styles are then submitted:

| Style | Fetches | Result |
|---|---|---|
| `cities` | the demo tiles and `layers://app/overlays/cities.geojson` | written to `cities.png` |
| `local-file` | `file:///etc/passwd` | refused |
| `cloud-metadata` | `http://169.254.169.254/latest/meta-data/`, a cloud metadata endpoint | refused |
| `private-layer` | `layers://app/overlays/../private/customers.geojson`, which the handler would serve | refused |

The refused styles fail with a `RenderError` saying `access denied by URL rules`, and their URLs
are never fetched: the scheme handler never sees the request for the private layer.

```shell
uv run python examples/untrusted-styles.py
```

````python title="examples/untrusted-styles.py"
--8<-- "examples/untrusted-styles.py"
````
