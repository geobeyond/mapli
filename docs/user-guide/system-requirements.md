# System requirements

## Platforms

mapli currently runs on Linux only. Prebuilt wheels are available for x86_64 and ARM64 (aarch64),
and need glibc 2.34 or newer (e.g. Ubuntu 22.04+, Debian 12+, RHEL 9+).

macOS on Apple Silicon may become possible through MapLibre Native's Metal backend, but is not
built or tested yet. Windows support depends on the upstream [maplibre-native-rs] project, which
does not currently support it.

## Runtime dependencies

mapli renders with MapLibre Native's OpenGL backend. On Linux, this creates an EGL context in the
process. EGL is the interface between OpenGL and the platform's windowing system.

The wheels bundle most of the libraries MapLibre Native needs (ICU, libpng, libjpeg, libwebp,
libuv). The following must be installed on the system:

**An EGL driver.** A GPU is not required: Mesa's `llvmpipe` software renderer works on servers
and in containers. On Debian/Ubuntu:

```shell
sudo apt-get install libegl1 libegl-mesa0 libgl1-mesa-dri
```

**libcurl.** Used to fetch remote styles, tiles, fonts and sprites. It is not bundled, so that it
uses the system's CA certificates. On Debian/Ubuntu:

```shell
sudo apt-get install libcurl4t64  # libcurl4 on Ubuntu 22.04 and Debian 12
```

**A display.** On a desktop machine, the existing display is used. On a machine without a
display, such as a server or a container, run an X virtual framebuffer (Xvfb):

```shell
sudo apt-get install xvfb
xvfb-run -a python your_app.py
```

For long-running services, Xvfb can instead run as its own service, with the `DISPLAY`
environment variable of the process that uses mapli set to Xvfb's display number:

```shell
Xvfb :99 -screen 0 1280x1024x24 -nolisten tcp &
DISPLAY=:99 python your_app.py
```

## Checking for graphics support

If the EGL driver or the display is missing, creating a `RenderPool` raises
`GraphicsUnavailableError`. Its message says which EGL step failed:

```python
import mapli

try:
    pool = mapli.RenderPool(workers=2)
except mapli.GraphicsUnavailableError as exc:
    print(f"cannot render on this machine: {exc}")
```

!!! tip "Machines that expose a GPU device"

    On some hosts, Mesa tries a hardware rendering path that Xvfb does not support, and
    `GraphicsUnavailableError` is raised. Setting the environment variable
    `LIBGL_ALWAYS_SOFTWARE=1` makes Mesa use the `llvmpipe` software renderer.


[maplibre-native-rs]: https://github.com/maplibre/maplibre-native-rs#platform-support
