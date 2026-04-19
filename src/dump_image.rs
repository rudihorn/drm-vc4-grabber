use std::{convert::TryFrom, mem::size_of, os::fd::AsRawFd};

use std::collections::HashSet;
use drm::control::framebuffer::Handle;
use drm::SystemError;
use drm_fourcc::{DrmFourcc, DrmModifier};
use image::{GenericImage, RgbImage};
use libc::close;
use nix::sys::mman;

use crate::{
    ffi::{self, gem_close},
    image_decoder::{
        decode_image_multichannel, decode_small_image_multichannel,
        decode_tiled_small_image, rgb565_to_rgb888, ToRgb, YUV420Pixel,
    },
    Card,
};

/// RAII guard for an mmap'd framebuffer. Ensures the mapping is unmapped and
/// the prime FD is closed even if the caller panics or returns early — the
/// previous code leaked both on certain failure paths.
struct MappedBuffer {
    addr: *mut libc::c_void,
    len: usize,
    fd: libc::c_int,
}

impl MappedBuffer {
    /// Map a DRM buffer object for read. `len` is in bytes.
    unsafe fn new(card: &Card, handle: u32, len: usize) -> Result<Self, SystemError> {
        let fd = ffi::prime_handle_to_fd(card.as_raw_fd(), handle)?;
        let addr = match mman::mmap(
            core::ptr::null_mut(),
            len as _,
            mman::ProtFlags::PROT_READ,
            // Drop MAP_POPULATE — it forces synchronous page faults over the
            // entire region, which stalls 4K captures. MADV_SEQUENTIAL below
            // gives the kernel enough hint for good readahead.
            mman::MapFlags::MAP_SHARED,
            fd,
            0,
        ) {
            Ok(m) => m,
            Err(e) => {
                libc::close(fd);
                // nix 0.20 returns `nix::Error` (not `Errno` directly) from
                // mmap. Extract the errno where possible, else fall back to
                // EIO as a generic I/O failure.
                let errno = match e {
                    nix::Error::Sys(errno) => errno,
                    _ => nix::errno::Errno::EIO,
                };
                return Err(SystemError::Unknown { errno });
            }
        };
        // Hint the kernel about our access pattern.
        libc::madvise(addr, len as _, libc::MADV_SEQUENTIAL);
        Ok(MappedBuffer { addr, len, fd })
    }

    /// Return the mapping as a read-only typed slice of `count` elements.
    fn as_slice<T: Copy>(&self, count: usize) -> &[T] {
        debug_assert!(count * size_of::<T>() <= self.len);
        unsafe { std::slice::from_raw_parts(self.addr as *const T, count) }
    }
}

impl Drop for MappedBuffer {
    fn drop(&mut self) {
        unsafe {
            let _ = mman::munmap(self.addr, self.len as _);
            if close(self.fd) == -1 {
                eprintln!(
                    "Warning: failed to close prime fd {} (errno: {})",
                    self.fd,
                    std::io::Error::last_os_error()
                );
            }
        }
    }
}

/// Copy an entire DRM buffer into a caller-owned slice. Retained for formats
/// that genuinely need a full copy (tiled/YUV/SAND128). For linear RGB, prefer
/// the direct sampling helpers below which skip the intermediate buffer.
fn copy_buffer<T: Sized + Copy>(
    card: &Card,
    handle: u32,
    to: &mut [T],
    _verbose: bool,
) -> Result<(), SystemError> {
    let length = to.len() * size_of::<T>();
    let map = unsafe { MappedBuffer::new(card, handle, length)? };
    to.copy_from_slice(map.as_slice::<T>(to.len()));
    Ok(())
}

fn decode_p030_image(
    card: &Card,
    size: (usize, usize),
    pitches: u32,
    handle: u32,
    modifier: u64,
    offset: usize,
    verbose: bool,
) -> Result<RgbImage, SystemError> {
    // We assume the DRM BROADCOM SAND128 format
    if u64::from(drm_fourcc::DrmModifier::Broadcom_sand128) != modifier & !(0xFFFF << 8) {
        return Err(SystemError::Unknown {
            errno: nix::errno::Errno::EOPNOTSUPP,
        });
    }

    let stride = 128 / 4; // each column is 128 bytes wide, we use 4 bytes per word
    let colpx = 96;

    let ylines = ((modifier >> 8) & 0xFFFFFFFF) as usize;
    let length = ylines * (size.0 / colpx) * stride;
    let crcboffset = offset / 4; // offset of the CrCb information in each column

    if verbose {
        println!(
            "P030, size: {:?}, lines: {}, pitches: {}, length: {}",
            size, ylines, pitches, length
        );
    }

    let mut yplane = vec![0u32; length as _];
    copy_buffer(card, handle, &mut yplane, verbose)?;

    let decim = 3;
    let mut img = RgbImage::new((size.0 / decim) as _, (size.1 / decim) as _);
    for y in 0..size.1 / decim {
        let ty = y * decim;
        for x in 0..size.0 / decim {
            let tx = x * decim;
            let col = tx / colpx;
            let col_offset = col * stride * ylines;
            let x_mod = (tx % colpx) / decim;

            let ypx = unsafe { yplane.get_unchecked(col_offset + ty * stride + x_mod) };
            let rx = x_mod / 2 * 2;
            let crcind = col_offset + crcboffset + ty / 2 * stride + rx;
            let crcbpx = unsafe { yplane.get_unchecked(crcind + 1) };

            let yuv = YUV420Pixel::new((ypx >> 2) as u8, (crcbpx >> 12) as u8, (crcbpx >> 2) as u8);

            unsafe {
                img.unsafe_put_pixel(x as _, y as _, yuv.rgb());
            }
        }
    }

    Ok(img)
}

fn decode_nv12_image(
    card: &Card,
    size: (usize, usize),
    pitches: u32,
    handle: u32,
    modifier: u64,
    offset: usize,
    verbose: bool,
) -> Result<RgbImage, SystemError> {
    // We assume the DRM BROADCOM SAND128 format
    if u64::from(drm_fourcc::DrmModifier::Broadcom_sand128) != modifier & !(0xFFFF << 8) {
        return Err(SystemError::Unknown {
            errno: nix::errno::Errno::EOPNOTSUPP,
        });
    }

    let stride = 128 / 4; // each column is 128 bytes wide, we use 4 bytes per word
    let colpx = 128; // 1 byte per pixel

    let ylines = ((modifier >> 8) & 0xFFFFFFFF) as usize;
    let length = ylines * (size.0 / colpx) * stride;
    let crcboffset = offset / 4; // offset of the CrCb information in each column

    if verbose {
        println!(
            "NV12, size: {:?}, lines: {}, pitches: {}, length: {}",
            size, ylines, pitches, length
        );
    }

    let mut yplane = vec![0u32; length as _];
    copy_buffer(card, handle, &mut yplane, verbose)?;

    let decim: usize = 4;
    let mut img = RgbImage::new((size.0 / decim) as _, (size.1 / decim) as _);
    for y in 0..size.1 / decim {
        let ty = y * decim;
        for x in 0..size.0 / decim {
            let tx = x * decim;
            let col = tx / colpx;
            let col_offset = col * stride * ylines;
            let x_mod = (tx % colpx) / decim;

            let ypx = unsafe { yplane.get_unchecked(col_offset + ty * stride + x_mod) };
            let rx = x_mod / 2 * 2;
            let crcind = col_offset + crcboffset + ty / 2 * stride + rx;
            let crcbpx = unsafe { yplane.get_unchecked(crcind + 1) };

            let yuv = YUV420Pixel::new((ypx >> 0) as u8, (crcbpx >> 0) as u8, (crcbpx >> 8) as u8);

            unsafe {
                img.unsafe_put_pixel(x as _, y as _, yuv.rgb());
            }
        }
    }

    Ok(img)
}

fn dump_linear_to_image(
    card: &Card,
    pitch: u32,
    size: (u32, u32),
    bpp: u32,
    handle: u32,
    verbose: bool,
) -> Result<RgbImage, SystemError> {
    // Decimate more aggressively for 4K — Hyperion averages per-LED anyway, so
    // a 480x270 sample gives the same LED colours as a full 3840x2160.
    let decim_factor: u32 = if size.0 >= 3840 || size.1 >= 2160 { 8 } else { 4 };
    let length = (pitch * size.1 / (bpp / 8)) as usize;

    if verbose {
        println!(
            "linear, size: {:?}, pitch: {}, bpp: {}, length: {}, decimation: {}",
            size, pitch, bpp, length, decim_factor
        );
    }

    // Map the framebuffer read-only and sample directly — no 33MB intermediate
    // copy for 4K frames. The map is walked once, row by row, picking every
    // `decim_factor` pixel. This slashes per-frame memory traffic by ~decim²×.
    let map = unsafe { MappedBuffer::new(card, handle, length * size_of::<u32>())? };
    let src: &[u32] = map.as_slice::<u32>(length);

    let bytepitch = (pitch / 4) as usize; // in u32 words
    let out_w = (size.0 / decim_factor) as u32;
    let out_h = (size.1 / decim_factor) as u32;
    let mut img = RgbImage::new(out_w, out_h);
    let step = decim_factor as usize;

    for y in 0..out_h {
        let src_y = (y as usize) * step;
        let row_off = src_y * bytepitch;
        for x in 0..out_w {
            let src_x = (x as usize) * step;
            // SAFETY: bounds checked by construction — src_y < size.1, src_x < size.0,
            // and row_off + src_x < length. Using get_unchecked avoids 2M bounds
            // checks per frame at 1080p.
            let v = unsafe { *src.get_unchecked(row_off + src_x) };
            let px = image::Rgb([(v >> 16) as u8, (v >> 8) as u8, v as u8]);
            unsafe { img.unsafe_put_pixel(x, y, px) };
        }
    }

    Ok(img)
}

fn dump_rgb565_to_image(
    card: &Card,
    pitch: u32,
    size: (u32, u32),
    bpp: u32,
    handle: u32,
    verbose: bool,
) -> Result<RgbImage, SystemError> {
    let length = pitch * size.1 / (bpp / 8);

    if verbose {
        println!(
            "rgb565, size: {:?}, pitch: {}, bpp: {}, length: {}",
            size, pitch, bpp, length
        );
    }
    let mut copy = vec![0u16; length as _];
    copy_buffer(card, handle, &mut copy, verbose)?;

    Ok(rgb565_to_rgb888(copy.as_mut_slice(), pitch, size))
}

fn dump_broadcom_tiled_to_image(
    card: &Card,
    size: (u32, u32),
    bpp: u32,
    handle: u32,
    verbose: bool,
) -> Result<RgbImage, SystemError> {
    let tilesize = 32;
    let tile_count = |n| (n + tilesize - 1) / tilesize;
    let tiles = (tile_count(size.0), tile_count(size.1));
    let total_tiles = tiles.0 * tiles.1;

    let length = total_tiles * tilesize * tilesize * (bpp / 8);

    let mut copy = vec![0; (length / 4) as _];
    copy_buffer(card, handle, &mut copy, verbose)?;

    Ok(decode_tiled_small_image(
        copy.as_mut_slice(),
        tilesize,
        tiles,
        size,
    ))
}

fn dump_yuv420_to_image(
    card: &Card,
    size: (u32, u32),
    pitches: [u32; 4],
    handles: [u32; 4],
    offsets: [u32; 4],
    verbose: bool,
) -> Result<RgbImage, SystemError> {
    // The length of the entire buffer is the length of the last buffer plus its
    // offset (assuming they are in order). The U and V buffers are grouped into
    // 2x2 tiles, hence the length is divided by 4.
    let length = offsets[2] + size.1 * pitches[2] / (pitches[0] / pitches[2]);
    //println!("  -> Mounting @{} +{}", offset, length);

    let mut copy = vec![0; length as _];
    copy_buffer(card, handles[0], &mut copy, verbose)?;

    let buffer_range = |i| {
        offsets[i] as usize..(offsets[i] + size.1 * pitches[i] / (pitches[0] / pitches[i])) as usize
    };

    let mappings = [
        &copy[buffer_range(0)],
        &copy[buffer_range(1)],
        &copy[buffer_range(2)],
    ];

    let mut pitches1 = [0; 3];
    pitches1.copy_from_slice(&pitches[0..3]);

    if size.0 > 640 {
        // If the image is large then just decode a smaller image
        Ok(decode_small_image_multichannel(mappings, size, pitches1))
    } else {
        Ok(decode_image_multichannel(mappings, size, pitches1))
    }
}

fn xr30_pixel_to_xrgb8888(v: u32) -> u32 {
    // XRGB2101010: [31:30]=X, [29:20]=R, [19:10]=G, [9:0]=B
    // Convert each 10-bit channel to 8-bit by shifting right 2
    let r = ((v >> 20) & 0x3FF) >> 2;
    let g = ((v >> 10) & 0x3FF) >> 2;
    let b = (v & 0x3FF) >> 2;
    (r << 16) | (g << 8) | b
}

fn dump_xrgb2101010_linear_to_image(
    card: &Card,
    pitch: u32,
    size: (u32, u32),
    handle: u32,
    verbose: bool,
) -> Result<RgbImage, SystemError> {
    // HDR content is almost always 4K these days; decimate 8x to match
    // dump_linear_to_image's behaviour and cut memory traffic accordingly.
    let decim_factor: u32 = if size.0 >= 3840 || size.1 >= 2160 { 8 } else { 4 };
    let length = (pitch * size.1 / 4) as usize;

    if verbose {
        println!(
            "xrgb2101010 linear, size: {:?}, pitch: {}, length: {}, decimation: {}",
            size, pitch, length, decim_factor
        );
    }

    // Direct-sample from mmap. Same 10-bit → 8-bit conversion as the image
    // decoder, just inlined so we skip the intermediate Vec allocation.
    let map = unsafe { MappedBuffer::new(card, handle, length * size_of::<u32>())? };
    let src: &[u32] = map.as_slice::<u32>(length);

    let bytepitch = (pitch / 4) as usize;
    let out_w = (size.0 / decim_factor) as u32;
    let out_h = (size.1 / decim_factor) as u32;
    let mut img = RgbImage::new(out_w, out_h);
    let step = decim_factor as usize;

    for y in 0..out_h {
        let row_off = (y as usize) * step * bytepitch;
        for x in 0..out_w {
            let src_x = (x as usize) * step;
            let v = unsafe { *src.get_unchecked(row_off + src_x) };
            // XRGB2101010 = [31:30]=X, [29:20]=R, [19:10]=G, [9:0]=B
            let r = (((v >> 20) & 0x3FF) >> 2) as u8;
            let g = (((v >> 10) & 0x3FF) >> 2) as u8;
            let b = ((v & 0x3FF) >> 2) as u8;
            unsafe { img.unsafe_put_pixel(x, y, image::Rgb([r, g, b])) };
        }
    }

    Ok(img)
}

fn dump_xrgb2101010_tiled_to_image(
    card: &Card,
    size: (u32, u32),
    handle: u32,
    verbose: bool,
) -> Result<RgbImage, SystemError> {
    let tilesize = 32;
    let tile_count = |n| (n + tilesize - 1) / tilesize;
    let tiles = (tile_count(size.0), tile_count(size.1));
    let total_tiles = tiles.0 * tiles.1;

    let length = total_tiles * tilesize * tilesize * 4;

    if verbose {
        println!(
            "xrgb2101010 tiled, size: {:?}, tiles: {:?}, length: {}",
            size, tiles, length
        );
    }

    let mut copy = vec![0u32; (length / 4) as _];
    copy_buffer(card, handle, &mut copy, verbose)?;

    // Convert XR30 pixels to XRGB8888 so the existing tiled decoder can process them
    for v in copy.iter_mut() {
        *v = xr30_pixel_to_xrgb8888(*v);
    }

    Ok(decode_tiled_small_image(
        copy.as_mut_slice(),
        tilesize,
        tiles,
        size,
    ))
}

/// Helper: construct a generic "unsupported format" error without panicking.
/// During HDR metadata transitions or plane switches the framebuffer can briefly
/// present a format we don't handle. Returning an error lets the capture loop
/// skip the frame and try again, instead of killing the process.
fn unsupported(kind: &str, detail: &dyn std::fmt::Debug) -> SystemError {
    eprintln!("Unsupported framebuffer {}: {:?}", kind, detail);
    SystemError::Unknown {
        errno: nix::errno::Errno::EOPNOTSUPP,
    }
}

pub fn dump_framebuffer_to_image(
    card: &Card,
    fb: Handle,
    verbose: bool,
) -> Result<RgbImage, SystemError> {
    let fbinfo2 = ffi::fb_cmd2(card.as_raw_fd(), fb.into())?;

    if verbose {
        println!("  -> FB Info 2: {:?}", fbinfo2);
    }

    let size = (fbinfo2.width, fbinfo2.height);

    // Helper function to clean up all GEM handles
    let cleanup_handles = || {
        let mut closed_handles = HashSet::new();
        for i in 0..4 {
            if fbinfo2.handles[i] != 0 && !closed_handles.contains(&fbinfo2.handles[i]) {
                if let Err(e) = gem_close(card.as_raw_fd(), fbinfo2.handles[i]) {
                    // Only log if it's not an "already closed" error
                    if !e.to_string().contains("invalid argument") {
                        eprintln!("Warning: Failed to close GEM handle {}: {}", fbinfo2.handles[i], e);
                    }
                } else if verbose {
                    println!("Closed GEM handle {}", fbinfo2.handles[i]);
                }
                closed_handles.insert(fbinfo2.handles[i]);
            }
        }
    };

    // Process the image with proper cleanup on both success and failure
    let image_result = if fbinfo2.pixel_format == 808661072 {
        decode_p030_image(
            card,
            (size.0 as _, size.1 as _),
            fbinfo2.pitches[0],
            fbinfo2.handles[0],
            fbinfo2.modifier[0],
            fbinfo2.offsets[1] as _,
            verbose,
        )
    } else {
        // Gracefully skip unknown formats rather than panicking. Kodi/HyperHDR can
        // briefly present formats the drm-fourcc crate hasn't mapped yet during
        // format transitions, and a panic here means systemd restart → rainbow.
        let fourcc = match drm_fourcc::DrmFourcc::try_from(fbinfo2.pixel_format) {
            Ok(f) => f,
            Err(_) => {
                cleanup_handles();
                return Err(unsupported("pixel format (unknown fourcc)", &fbinfo2.pixel_format));
            }
        };
        let modifier = match drm_fourcc::DrmModifier::try_from(fbinfo2.modifier[0]) {
            Ok(m) => m,
            Err(_) => {
                cleanup_handles();
                return Err(unsupported("modifier (unknown)", &fbinfo2.modifier[0]));
            }
        };

        match fourcc {
            DrmFourcc::Xrgb8888 => match modifier {
                DrmModifier::Broadcom_vc4_t_tiled => {
                    dump_broadcom_tiled_to_image(card, size, 32, fbinfo2.handles[0], verbose)
                }
                DrmModifier::Linear => dump_linear_to_image(
                    card,
                    fbinfo2.pitches[0],
                    size,
                    32,
                    fbinfo2.handles[0],
                    verbose,
                ),
                _ => Err(unsupported("Xrgb8888 modifier", &modifier)),
            },
            DrmFourcc::Argb8888 => match modifier {
                DrmModifier::Broadcom_vc4_t_tiled => {
                    dump_broadcom_tiled_to_image(card, size, 32, fbinfo2.handles[0], verbose)
                }
                DrmModifier::Linear => dump_linear_to_image(
                    card,
                    fbinfo2.pitches[0],
                    size,
                    32,
                    fbinfo2.handles[0],
                    verbose,
                ),
                _ => Err(unsupported("Argb8888 modifier", &modifier)),
            },
            DrmFourcc::Xrgb2101010 => match modifier {
                DrmModifier::Broadcom_vc4_t_tiled => {
                    dump_xrgb2101010_tiled_to_image(card, size, fbinfo2.handles[0], verbose)
                }
                DrmModifier::Linear => dump_xrgb2101010_linear_to_image(
                    card,
                    fbinfo2.pitches[0],
                    size,
                    fbinfo2.handles[0],
                    verbose,
                ),
                _ => Err(unsupported("Xrgb2101010 modifier", &modifier)),
            },
            DrmFourcc::Yuv420 => dump_yuv420_to_image(
                card,
                size,
                fbinfo2.pitches,
                fbinfo2.handles,
                fbinfo2.offsets,
                verbose,
            ),
            DrmFourcc::Rgb565 => dump_rgb565_to_image(
                card,
                fbinfo2.pitches[0],
                size,
                16,
                fbinfo2.handles[0],
                verbose,
            ),
            DrmFourcc::Nv12 => decode_nv12_image(
                card,
                (size.0 as _, size.1 as _),
                fbinfo2.pitches[0],
                fbinfo2.handles[0],
                fbinfo2.modifier[0],
                fbinfo2.offsets[1] as _,
                verbose,
            ),

            _ => Err(unsupported("pixel format", &fourcc)),
        }
    };

    // Always clean up handles, regardless of success or failure
    cleanup_handles();

    // Return the result (propagate any errors after cleanup)
    image_result
}
