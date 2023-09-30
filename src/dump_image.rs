use std::{mem::size_of, os::fd::AsRawFd, convert::TryFrom};

use drm::SystemError;
use drm::control::framebuffer::Handle;
use drm_fourcc::{DrmFourcc, DrmModifier};
use image::RgbImage;
use libc::close;
use nix::sys::mman;

use crate::{Card, ffi::{self, gem_close}, image_decoder::{decode_image, rgb565_to_rgb888, decode_tiled_small_image, decode_small_image_multichannel, decode_image_multichannel}};


fn copy_buffer<T: Sized + Copy>(
    card: &Card,
    handle : u32,
    to: &mut [T],
    verbose: bool) -> Result<(), SystemError> {

    let length = to.len() * size_of::<T>();

    let hfd = ffi::prime_handle_to_fd(card.as_raw_fd(), handle)?;

    if verbose {
        println!("handle fd {}", hfd);
    }

    let addr = core::ptr::null_mut();
    let prot = mman::ProtFlags::PROT_READ;
    let flags = mman::MapFlags::MAP_SHARED;
    unsafe {
        let map = mman::mmap(
            addr,
            length as _,
            prot,
            flags,
            hfd,
            0,
        ).unwrap();

        let mapping: &mut [T] = std::slice::from_raw_parts_mut(map as *mut _, to.len());
        to.copy_from_slice(mapping);
        mman::munmap(map, length as _).unwrap();

        if close(hfd) == -1 {
            panic!("Failed to close prime fd.");
        };
    }

    Ok(())
}

fn dump_linear_to_image(
    card: &Card,
    pitch: u32,
    size: (u32, u32),
    bpp: u32,
    handle: u32,
    verbose: bool,
) -> Result<RgbImage, SystemError> {
    let size = (size.0, size.1);

    let length = pitch * size.1 / (bpp / 8);

    println!(
        "size: {:?}, pitch: {}, bpp: {}, length: {}",
        size, pitch, bpp, length
    );
    let mut copy = vec![0u32; length as _];
    copy_buffer(card, handle, &mut copy, verbose)?;

    Ok(decode_image(copy.as_mut_slice(), pitch, size))
}

fn dump_rgb565_to_image(
    card: &Card,
    pitch: u32,
    size: (u32, u32),
    bpp: u32,
    handle: u32,
    verbose: bool,
) -> Result<RgbImage, SystemError> {
    // let size = (size.0, size.1 / 64);

    let length = pitch * size.1 / (bpp / 8);

    println!(
        "size: {:?}, pitch: {}, bpp: {}, length: {}",
        size, pitch, bpp, length
    );
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

    Ok(decode_tiled_small_image(copy.as_mut_slice(), tilesize, tiles, size))
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
    let length = offsets[2] + size.1 * pitches[2] * pitches[2] / pitches[0];
    //println!("  -> Mounting @{} +{}", offset, length);

    let mut copy = vec![0; length as _];
    copy_buffer(card, handles[0], &mut copy, verbose)?;

    let buffer_range = |i| {
        offsets[i] as usize..(offsets[i] + size.1 * pitches[i] * pitches[i] / pitches[0]) as usize
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

pub fn dump_framebuffer_to_image(card: &Card, fb: Handle, verbose: bool) -> Result<RgbImage, SystemError> {
    let fbinfo2 = ffi::fb_cmd2(card.as_raw_fd(), fb.into())?;

    if verbose {
        println!("  -> FB Info 2: {:?}", fbinfo2);
    }

    let size = (fbinfo2.width, fbinfo2.height);

    let fourcc = drm_fourcc::DrmFourcc::try_from(fbinfo2.pixel_format).unwrap();
    let modifier = drm_fourcc::DrmModifier::try_from(fbinfo2.modifier[0]).unwrap();

    let image_result = match fourcc {
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
            _ => panic!("Unsupported framebuffer modifier: {:?}", modifier),
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
            _ => panic!("Unsupported framebuffer modifier: {:?}", modifier),
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

        _ => panic!(
            "Unsupported framebuffer pixel format: {} {:x}",
            fourcc, fbinfo2.pixel_format
        ),
    };

    gem_close(card.as_raw_fd(), fbinfo2.handles[0]).unwrap();

    let image = image_result?;

    Ok(image)
}
