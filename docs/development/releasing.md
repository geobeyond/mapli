# Wheels and releases

## Building distributable wheels

Distributable wheels are built inside a [manylinux] `manylinux_2_34` container. The wheel links
against the container's glibc (2.34), and [auditwheel] bundles the other libraries it needs.
The script `scripts/build-manylinux-wheel.sh` does this, and is also used by CI.

To run it locally:

```shell
docker run --rm -v "$PWD":/io -w /io \
    -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
    -e CARGO_TARGET_DIR=/build/target -v mapli-manylinux-build:/build \
    quay.io/pypa/manylinux_2_34_x86_64 scripts/build-manylinux-wheel.sh
```

The wheel and the sdist are written to `dist/`. The `mapli-manylinux-build` volume keeps the
build cache between runs, so only the first build compiles MapLibre Native from scratch.

In CI (`.github/workflows/ci.yml`):

- the `build_wheel` job builds the x86_64 and aarch64 wheels on pushes to `main` and on release
  tags. It can also be started from the Actions tab;
- the `test_wheel` job then runs the test suite against each wheel, on a plain Ubuntu runner that
  only has the [runtime dependencies](../user-guide/system-requirements.md) installed.

## Making a release

The version is set in one place: the `[workspace.package]` section of the root `Cargo.toml`.
Both Rust crates inherit it, and the Python package reads it from there.

1. Set `version` in the root `Cargo.toml` (e.g. `0.3.0`), and refresh the lockfiles with
   `cargo update --workspace` and `uv lock`.
2. Update `CHANGELOG.md`, and the version in the wheel URLs of the installation
   instructions, in `README.md` and `docs/index.md`.
3. Commit, then push an annotated tag with the same version, prefixed with `v`:

    ```shell
    git tag -a v0.3.0 -m "Release v0.3.0"
    git push origin v0.3.0
    ```

The tag triggers the release workflow (`.github/workflows/release.yml`). It runs CI, checks that
the version of the built wheels matches the tag, and creates a GitHub release with the wheels and
the sdist attached.

After the release, set the version to the next expected version with a `-dev.0` suffix
(e.g. `0.4.0-dev.0`), refresh the lockfiles and commit:

```shell
# after editing the version in Cargo.toml
cargo update --workspace
uv lock
git commit -am "Bump version to 0.4.0-dev.0"
```

maturin converts this to the Python version `0.4.0.dev0`, which marks the wheels built from
`main` in the meantime as development builds. For the next release, step 1 removes the suffix,
or sets a different version.


[manylinux]: https://github.com/pypa/manylinux
[auditwheel]: https://github.com/pypa/auditwheel
