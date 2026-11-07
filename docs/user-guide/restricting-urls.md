# Restricting URLs

MapLibre Native fetches every URL a style refers to: remote URLs, local files (`file://`,
`mbtiles://`, `pmtiles://file://`), and the URLs of [custom schemes](custom-url-schemes.md). A
style therefore has the same reach as the process that renders it. If your application renders
styles it does not fully control, e.g. styles fetched from a configurable URL, a malicious or
compromised style could:

- draw files from the server into the map image;
- make the server fetch internal URLs;
- read data that a scheme handler serves for other styles.

`mapli.restrict_resource_urls()` can be used to limit MapLibre Native to the URLs that start with one of a
list of prefixes:

```python
import mapli

mapli.restrict_resource_urls(
    allowed_url_prefixes=[
        "store://",
        "https://fonts.example.org/",
        "file:///srv/tiles/",
    ],
)

pool = mapli.RenderPool()
```

Requests for any other URL fail with the message `"<url>: access denied by URL rules"`. Rendering
then raises `RenderError`, or `StyleLoadError` if the denied URL is the style itself. The denied
URLs are never fetched.

Without a call to `restrict_resource_urls()`, every URL is allowed.

The [untrusted styles example](../examples/untrusted-styles.md) renders styles submitted by
users this way.

The list of URL prefixes is process-wide, and **can only be set once**, before the first `RenderPool` is created.
After that, `restrict_resource_urls()` raises `UrlRulesClosedError`, so that the rules cannot be
loosened while styles are being rendered.

## Prefixes

A prefix is either:

- a URL, e.g. `https://fonts.example.org/`, which allows the URLs that start with it;
- a scheme followed by `://`, e.g. `store://`, which allows every URL of that scheme.

URLs and prefixes are compared so that a URL which starts with an allowed prefix cannot reach
outside of it:

- Schemes are compared case-insensitively, and remote URLs are normalized first, e.g. host names
  are lowercased.
- A prefix only matches up to a path boundary: `https://example.org/fonts` allows
  `https://example.org/fonts/a.pbf`, but not `https://example.org/fontsevil` or
  `https://example.org.evil.com/`. Ending prefixes with `/` makes this explicit.
- `.` and `..` path segments are resolved before comparing remote URLs, so
  `https://example.org/fonts/../private` is denied by `https://example.org/fonts/`.
- Some URLs are always denied, as different URL parsers would read them differently: URLs with
  credentials (`https://user@host/`), whitespace, control characters or backslashes.

Custom schemes are not exempt: their URLs must be allowed like any other, even if the scheme has
a handler. Handlers receive the URL as the style wrote it, not its normalized form.

### Local files

Local files are denied unless a `file://` prefix allows them, e.g. `file:///srv/tiles/`. MapLibre
Native percent-decodes local file URLs before opening the file, so mapli compares the decoded
path. Local file URLs with `.` or `..` path segments, or with a malformed `%` sequence, are
always denied.

Styles registered with `path=` are loaded from a `file://` URL as well, so they are denied unless
a `file://` prefix allows them. To render a style file without allowing its directory, read it
yourself and register it with `json=`.

### MBTiles and PMTiles archives

Archive URLs are checked by what they read:

- `mbtiles:///srv/tiles/a.mbtiles` is checked as `file:///srv/tiles/a.mbtiles`;
- `pmtiles://https://tiles.example.org/a.pmtiles` is checked as
  `https://tiles.example.org/a.pmtiles`.

So `file:///srv/tiles/` allows MBTiles and PMTiles archives in `/srv/tiles` as well as plain
files there, and `https://tiles.example.org/` allows PMTiles archives on that host.

## Limitations

- HTTP redirects are followed by MapLibre Native's HTTP client, so their targets are not checked.
  Only allow servers you trust not to redirect elsewhere.
- For remote URLs, a prefix with a path, e.g. `https://example.org/fonts/`, is only as strong as
  the server's own handling of paths: a server that decodes `%2F` in paths may serve something
  outside of `/fonts/` for a URL inside it. Prefer allowing whole hosts.
- The rules apply to every style in the process, since MapLibre Native's requests do not say
  which style they come from.
- Data written into the style itself, e.g. inline GeoJSON, is not fetched, so the rules do not
  apply to it.
