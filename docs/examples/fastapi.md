# FastAPI web API

A [FastAPI] application that renders map images and XYZ tiles on request, with three styles.

- The render pool is created in the application's lifespan handler, so it is shared by all
  requests, and its workers stop when the application shuts down.
- The routes use the async render methods, so the event loop keeps serving other requests while
  an image is rendered.

```shell
uv run fastapi dev examples/webapi.py
```

The interactive API documentation is then available at <http://127.0.0.1:8000/docs>. The tile
route can be used as the tile URL of a web map client, e.g.
`http://127.0.0.1:8000/tiles/liberty/{z}/{x}/{y}.png?tile_size=256` for Leaflet.

````python title="examples/webapi.py"
--8<-- "examples/webapi.py"
````


[FastAPI]: https://fastapi.tiangolo.com/
