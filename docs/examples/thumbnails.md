# Concurrent thumbnails

Renders a 256 x 256 pixel thumbnail of each of four cities, using `RenderPool.arender` and
`asyncio.gather`. The pool has two workers, so two thumbnails are rendered at a time. Each
thumbnail is written to `<city>.png` in the current directory.

```shell
uv run python examples/thumbnail-render.py
```

````python title="examples/thumbnail-render.py"
--8<-- "examples/thumbnail-render.py"
````
