# Async rendering

Each render method has an `async` version for use with `asyncio`:

| Blocking | Async |
|---|---|
| `render` | `arender` |
| `render_from_bounds` | `arender_from_bounds` |
| `render_tile` | `arender_tile` |

The async methods take the same arguments and raise the same exceptions as the blocking ones.
While a worker renders, the event loop is free to run other tasks.

## Example

```python
import asyncio
from pathlib import Path

import mapli

CITIES = {
    "lisbon": (-9.14, 38.72),
    "porto": (-8.61, 41.15),
    "madrid": (-3.70, 40.42),
}


async def main() -> None:
    pool = mapli.RenderPool(workers=2)
    pool.register_style("demo", url="https://demotiles.maplibre.org/style.json")

    images = await asyncio.gather(
        *(
            pool.arender("demo", lon=lon, lat=lat, zoom=5, width=256, height=256)
            for lon, lat in CITIES.values()
        )
    )
    for name, png_bytes in zip(CITIES, images):
        Path(f"{name}.png").write_bytes(png_bytes)


asyncio.run(main())
```

With `workers=2`, two of the three images are rendered at the same time, and the third one
starts when a worker is free.
