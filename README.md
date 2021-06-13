# Hyperion DRM VC4 screen grabber 

This is an experimental attempt to capture a screenshot from a Raspberry Pi that
 is rendering using the [Direct Rendering
 Manager](https://en.wikipedia.org/wiki/Direct_Rendering_Manager). It currently
 works by opening the default card adapter, looping through all the planes and
 finding the underlying framebuffers. Using the framebuffer it is possible to
 determine the buffer handle for the underlying buffer object handle.

The buffer object handle can be mapped to memory using the VC4 specific DRM
API's (see `/usr/include/drm/vc4_drm.h`), specifically using the ioctl
`drm_vc4_mmap_bo`. The memory data is stored using XRGB8888 in little-endian 32
bit words, and is tiled in 32x32 bit squares
([reference](https://docs.mesa3d.org/drivers/vc4.html#tiled-rendering)). There
is also some other interlacing or similar I have not quite figured out yet.

The current implementation connects to Hyperion at `127.0.0.1:19400` and
directly uploads the images.

## Compiling

1. Ensure rust is installed (with rustup and cargo). 
2. Install the target toolchain for raspberry pi: `rustup target install armv7-unknown-linux-gnueabihf`.
3. Ensure the linker for this toolchain is installed, e.g. `sudo apt install binutils-arm-none-eabi`
4. Set the linker in your env var: `export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=/usr/bin/arm-linux-gnueabihf-gcc`
5. Compile: `cargo build --release --target armv7-unknown-linux-gnueabihf`
6. The built file will be at `target/armv7-unknown-linux-gnueabihf/release/drm-v4-capture`

## Example

The following is an example screen capture in the current codes state.

![Image capture](screenshot.png "Raspberry pi using latest OSMC devel branch and kodi 19")


## Debug Output

```
Driver: Driver { name: SmallOsString { data: [118, 99, 52, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], len: 3, as_ref(): "vc4" }, date: SmallOsString { data: [50, 48, 49, 52, 48, 54, 49, 54, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], len: 8, as_ref(): "20140616" }, desc: SmallOsString { data: [66, 114, 111, 97, 100, 99, 111, 109, 32, 86, 67, 52, 32, 103, 114, 97, 112, 104, 105, 99, 115, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], len: 21, as_ref(): "Broadcom VC4 graphics" } }
Plane Info: Info { handle: plane::Handle(84), crtc: Some(crtc::Handle(83)), fb: Some(framebuffer::Handle(207)), pos_crtcs: 15, formats: [0, 0, 0, 0, 0, 0, 0, 0], fmt_len: 8 }
  -> FB Info: Info { handle: framebuffer::Handle(207), size: (1920, 1080), pitch: 7680, bpp: 32, depth: 24, buffer: 1 }
  -> FB Info 2: drm_mode_fb_cmd2 { fb_id: 207, width: 1920, height: 1080, pixel_format: 875713112, flags: 2, handles: [2, 0, 0, 0], pitches: [7680, 0, 0, 0], offsets: [0, 0, 0, 0], modifier: [504403158265495553, 0, 0, 0] }
  -> Offset: 306663424
Plane Info: Info { handle: plane::Handle(90), crtc: None, fb: None, pos_crtcs: 15, formats: [0, 0, 0, 0, 0, 0, 0, 0], fmt_len: 8 }
Plane Info: Info { handle: plane::Handle(96), crtc: None, fb: None, pos_crtcs: 15, formats: [0, 0, 0, 0, 0, 0, 0, 0], fmt_len: 8 }
Plane Info: Info { handle: plane::Handle(102), crtc: None, fb: None, pos_crtcs: 15, formats: [0, 0, 0, 0, 0, 0, 0, 0], fmt_len: 8 }
Plane Info: Info { handle: plane::Handle(108), crtc: None, fb: None, pos_crtcs: 15, formats: [0, 0, 0, 0, 0, 0, 0, 0], fmt_len: 8 }
Plane Info: Info { handle: plane::Handle(114), crtc: None, fb: None, pos_crtcs: 15, formats: [0, 0, 0, 0, 0, 0, 0, 0], fmt_len: 8 }
Plane Info: Info { handle: plane::Handle(120), crtc: None, fb: None, pos_crtcs: 15, formats: [0, 0, 0, 0, 0, 0, 0, 0], fmt_len: 8 }
Plane Info: Info { handle: plane::Handle(126), crtc: None, fb: None, pos_crtcs: 15, formats: [0, 0, 0, 0, 0, 0, 0, 0], fmt_len: 8 }
Plane Info: Info { handle: plane::Handle(132), crtc: None, fb: None, pos_crtcs: 15, formats: [0, 0, 0, 0, 0, 0, 0, 0], fmt_len: 8 }
Plane Info: Info { handle: plane::Handle(138), crtc: None, fb: None, pos_crtcs: 15, formats: [0, 0, 0, 0, 0, 0, 0, 0], fmt_len: 8 }
Plane Info: Info { handle: plane::Handle(144), crtc: None, fb: None, pos_crtcs: 15, formats: [0, 0, 0, 0, 0, 0, 0, 0], fmt_len: 8 }
Plane Info: Info { handle: plane::Handle(150), crtc: None, fb: None, pos_crtcs: 15, formats: [0, 0, 0, 0, 0, 0, 0, 0], fmt_len: 8 }
Plane Info: Info { handle: plane::Handle(156), crtc: None, fb: None, pos_crtcs: 15, formats: [0, 0, 0, 0, 0, 0, 0, 0], fmt_len: 8 }
Plane Info: Info { handle: plane::Handle(162), crtc: None, fb: None, pos_crtcs: 15, formats: [0, 0, 0, 0, 0, 0, 0, 0], fmt_len: 8 }
Plane Info: Info { handle: plane::Handle(168), crtc: None, fb: None, pos_crtcs: 15, formats: [0, 0, 0, 0, 0, 0, 0, 0], fmt_len: 8 }
Plane Info: Info { handle: plane::Handle(174), crtc: None, fb: None, pos_crtcs: 15, formats: [0, 0, 0, 0, 0, 0, 0, 0], fmt_len: 8 }
CRTC Info: Info { handle: crtc::Handle(62), position: (0, 0), mode: None, fb: None, gamma_length: 256 }
CRTC Info: Info { handle: crtc::Handle(69), position: (0, 0), mode: None, fb: None, gamma_length: 256 }
CRTC Info: Info { handle: crtc::Handle(76), position: (0, 0), mode: None, fb: None, gamma_length: 256 }
CRTC Info: Info { handle: crtc::Handle(83), position: (0, 0), mode: Some(Mode { name: "1920x1080", clock: 148500, size: (1920, 1080), hsync: (2008, 2052, 2200), vsync: (1084, 1089, 1125), hskew: 0, vscan: 0, vrefresh: 60 }), fb: None, gamma_length: 256 }
```
