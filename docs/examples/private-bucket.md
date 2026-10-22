# Private bucket with obstore

Renders a [PMTiles] archive kept in a private object storage bucket (e.g. AWS S3, an
S3-compatible store or Google Cloud Storage). The archive is read with [obstore], through a
[custom URL scheme](../user-guide/custom-url-schemes.md) handler.

- The handler serves `store://<key>` URLs. The style refers to the archive as
  `pmtiles://store://<key>`, a URL that contains no credentials; obstore reads the credentials
  from the environment.
- The style is generated from the archive itself: its header gives the bounds and the tile type,
  and, for vector tiles, its metadata lists the layers, which are all drawn.
- Each request served by the handler is printed, with its byte range, followed by the read from
  the store and its duration.
- With `--cache-size N`, the last `N` reads are cached in memory. Requests served from the cache
  print no read.

```shell
# AWS S3, with credentials from AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_REGION, ...
uv run python examples/private-bucket.py s3://my-bucket tiles/roads.pmtiles

# an S3-compatible store served over plain HTTP, e.g. a local MinIO
AWS_ENDPOINT_URL=http://localhost:9000 AWS_ALLOW_HTTP=true \
    AWS_ACCESS_KEY_ID=minioadmin AWS_SECRET_ACCESS_KEY=minioadmin \
    uv run python examples/private-bucket.py s3://my-bucket roads.pmtiles

# Google Cloud Storage, with credentials from GOOGLE_APPLICATION_CREDENTIALS
uv run python examples/private-bucket.py gs://my-bucket tiles/roads.pmtiles
```

````python title="examples/private-bucket.py"
--8<-- "examples/private-bucket.py"
````


[PMTiles]: https://docs.protomaps.com/pmtiles/
[obstore]: https://developmentseed.org/obstore/
