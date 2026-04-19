# Hyperion & HyperHDR DRM VC4 screen grabber

Captures the Raspberry Pi's framebuffer directly from the DRM (Direct Rendering
Manager) subsystem and streams it to [Hyperion] / [HyperHDR] for ambient LED
backlighting. It does not rely on a Kodi add-on, an X server, or a separate
grabber binary: it reads the framebuffer that Kodi (or any KMS-native
application) is already rendering, decodes it, and ships the image over TCP.

[Hyperion]: https://github.com/hyperion-project/hyperion.ng
[HyperHDR]: https://github.com/awawa-dev/HyperHDR

## Supported platforms

- **Raspberry Pi 5** (LibreELEC, driver `vc4` exposed as `card1`)
- **Raspberry Pi 4** (LibreELEC / Raspberry Pi OS, driver `vc4` at `card0`)
- Earlier Pi models that expose the VC4 KMS driver

Pi 5 and Pi 4 have the display on different card nodes. Use the `-d` flag to
override the default if needed (see [Usage](#usage)).

## Supported framebuffer formats

The grabber dispatches on the framebuffer's DRM FourCC and modifier:

| FourCC        | Modifier                 | Common use                        |
|---------------|--------------------------|-----------------------------------|
| `XRGB8888`    | Linear / VC4 T-tiled     | Kodi UI, SDR video                |
| `ARGB8888`    | Linear / VC4 T-tiled     | Kodi UI with alpha                |
| `XRGB2101010` | Linear / VC4 T-tiled     | 10-bit HDR content                |
| `YUV420`      | Linear                   | SD/HD video (8-bit)               |
| `NV12`        | Broadcom SAND128         | Hardware-decoded HD video         |
| `P030`        | Broadcom SAND128         | Hardware-decoded 4K HDR video     |
| `RGB565`      | Linear                   | Legacy low-bpp surfaces           |

Unknown formats are skipped with a log message, not a crash, so a transient
format switch (common during HDR metadata transitions) will not take the
service down.

## Usage

1. Download the latest release from the [Releases] page and extract it:
   ```
   tar xf drm-vc4-grabber-vX.Y.Z-aarch64-linux.tar.xz
   ```
2. Run it directly, pointing at your Hyperion server:
   ```
   ./drm-vc4-grabber-vX.Y.Z-aarch64-linux/drm-vc4-grabber
   ```
3. For a permanent setup, copy the supplied `drm-capture.service` unit to your
   systemd config path. On LibreELEC:
   ```
   cp drm-capture.service /storage/.config/system.d/
   systemctl enable --now drm-capture.service
   ```

[Releases]: https://github.com/rudihorn/drm-vc4-grabber/releases

### Command-line flags

| Flag | Default | Description |
|------|---------|-------------|
| `-d`, `--device` | `/dev/dri/card1` | DRM card node. Use `card0` for Pi 4 and earlier. |
| `-a`, `--address` | `127.0.0.1:19400` | Hyperion / HyperHDR TCP endpoint. |
| `--screenshot` | off | Capture a single frame to `screenshot.png` and exit. |
| `-v`, `--verbose` | off | Log DRM, framebuffer, and send details. |

## Building from source

The easiest cross-compilation path uses `cargo-zigbuild`, which bundles its own
toolchain via `pip install ziglang`. No system cross-compiler or sudo required.

```bash
# one-time setup
rustup target add aarch64-unknown-linux-musl
cargo install cargo-zigbuild
pip install --user ziglang

# build
cargo zigbuild --release --target aarch64-unknown-linux-musl
```

The resulting binary is at
`target/aarch64-unknown-linux-musl/release/drm-vc4-grabber`.

Native builds on x86_64 (for compile-checks only, not runnable) work with
`cargo build --release`.

## How it works

At startup the grabber opens the DRM device, enables the `UNIVERSAL_PLANES`
capability, then enters a capture loop:

1. Walk the CRTCs and planes to find the currently-scanned-out framebuffer.
2. Call `DRM_IOCTL_MODE_GETFB2` to pull the framebuffer metadata (size, FourCC,
   modifier, GEM handles, pitches, offsets).
3. Convert the GEM handles to PRIME file descriptors, `mmap` them read-only.
4. Decode the pixel data into an 8-bit RGB image, decimating 4x or 8x to keep
   the image small (Hyperion averages across LED regions anyway).
5. Flatbuffer-encode the image and send it over TCP to Hyperion.
6. Close the PRIME FDs and GEM handles, then sleep the remainder of the
   ~33 ms frame budget.

Linear RGB formats are sampled directly from the mmap without an intermediate
copy. Broadcom SAND128-tiled formats (NV12, P030) are copied once and decoded.

## Compatibility with ambient-light servers

The grabber uses Hyperion's native flatbuffer protocol on TCP port 19400. Both
**Hyperion.NG** and **HyperHDR** speak this protocol, so either works as the
receiver without any configuration change in the grabber.

## License

MIT. See [LICENSE](LICENSE).

## Donations

Not required and not solicited, but if you would like to support Rudi Horn
(the original author):

| PayPal | Bitcoin |
|--------|---------|
| [![paypal](https://www.paypalobjects.com/en_US/i/btn/btn_donateCC_LG.gif)](https://www.paypal.me/rudihppal) | [bitcoin:bc1qjantllys0pg3zvsr97krxzz9dxzlmxmgy5qk4v](bitcoin:bc1qjantllys0pg3zvsr97krxzz9dxzlmxmgy5qk4v) |
