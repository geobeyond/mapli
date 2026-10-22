# Examples

The [`examples`](https://github.com/geobeyond/mapli/tree/main/examples) directory of the
repository contains complete programs that use mapli:

| Example | Shows |
|---|---|
| [Concurrent thumbnails](thumbnails.md) | Rendering several images at the same time with the async API |
| [FastAPI web API](fastapi.md) | Serving map images and XYZ tiles over HTTP |
| [Private bucket with obstore](private-bucket.md) | Rendering a PMTiles archive from a private object storage bucket, through a custom URL scheme |

The examples use dependencies from the `dev` dependency group. To run them from a clone of the
repository:

```shell
uv sync --group dev
```

On a machine without a display, prefix the commands with `xvfb-run -a`, see
[System requirements](../user-guide/system-requirements.md).
