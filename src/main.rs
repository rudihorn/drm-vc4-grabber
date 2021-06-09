#[macro_use]
extern crate nix;

use std::fs::{File, OpenOptions};
use std::usize;

use drm::control::Device as ControlDevice;
use drm::Device;

use image::{GenericImage, Rgb, RgbImage};

use std::os::unix::io::{AsRawFd, RawFd};

pub mod ffi;

struct Card(File);

impl AsRawFd for Card {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl Device for Card {}
impl ControlDevice for Card {}

impl Card {
    pub fn open(path: &str) -> Self {
        let mut options = OpenOptions::new();
        options.read(true);
        options.write(true);
        Card(options.open(path).unwrap())
    }

    pub fn open_global() -> Self {
        Self::open("/dev/dri/card0")
    }
}

fn main() {
    let card = Card::open_global();
    let driver = card.get_driver().unwrap();
    println!("Driver: {:?}", driver);

    let plane_handles = card.plane_handles().unwrap();

    for plane in plane_handles.planes() {
        let info = card.get_plane(*plane).unwrap();
        println!("Plane Info: {:?}", info);

        if info.crtc().is_some() {
            let fb = info.framebuffer().unwrap();
            let fbinfo = card.get_framebuffer(fb).unwrap();
            let fbinfo2 = ffi::fb_cmd2(card.as_raw_fd(), fb.into()).unwrap();
            println!("  -> FB Info: {:?}", fbinfo);
            println!("  -> FB Info 2: {:?}", fbinfo2);

            let offset = ffi::mmap_bo(card.as_raw_fd(), fbinfo2.handles[0]).unwrap();
            println!("  -> Offset: {}", offset);

            let size = fbinfo.size();

            let tilesize = 32;
            let tile_count = |n| (n + tilesize - 1) / tilesize;
            let tiles = (tile_count(size.0), tile_count(size.1));
            let total_tiles = tiles.0 * tiles.1;
            let mut img = RgbImage::new(tiles.0 * tilesize, tiles.1 * tilesize);

            let length = total_tiles * tilesize * tilesize * (fbinfo.bpp() / 8);
            let map = {
                use nix::sys::mman;
                let addr = core::ptr::null_mut();
                //let length = length;
                let prot = mman::ProtFlags::PROT_READ | mman::ProtFlags::PROT_WRITE;
                let flags = mman::MapFlags::MAP_SHARED;
                let offset = offset as _;
                unsafe {
                    mman::mmap(addr, length as _, prot, flags, card.as_raw_fd(), offset).unwrap()
                }
            };

            let mapping: &mut [u8] =
                unsafe { std::slice::from_raw_parts_mut(map as *mut _, length as _) };

            let mut i = 0;

            let mut copy_px = |x, y| {
                let color = Rgb([
                    mapping[(i + 2) as usize],
                    mapping[(i + 1) as usize],
                    mapping[(i + 0) as usize],
                ]);
                unsafe {
                    img.unsafe_put_pixel(x, y, color);
                }
                i = i + 4;
            };
            let mut copy_4_px = |x, y| {
                copy_px(x, y);
                copy_px(x + 1, y);
                copy_px(x + 2, y);
                copy_px(x + 3, y);
            };

            let mut copy_4x4_px = |x, y| {
                copy_4_px(x, y);
                copy_4_px(x, y + 1);
                copy_4_px(x, y + 2);
                copy_4_px(x, y + 3);
            };

            let mut copy_16x4_px = |x, y| {
                copy_4x4_px(x, y);
                copy_4x4_px(x + 4, y);
                copy_4x4_px(x + 8, y);
                copy_4x4_px(x + 12, y);
            };

            let mut copy_16x16_px = |x, y| {
                copy_16x4_px(x, y);
                copy_16x4_px(x, y + 4);
                copy_16x4_px(x, y + 8);
                copy_16x4_px(x, y + 12);
            };

            for ytile in 0..tiles.1 {
                if ytile % 2 == 0 {
                    let mut copy_tile =
                        |x, y| {
                            copy_16x16_px(x, y);
                            copy_16x16_px(x, y + 16);
                            copy_16x16_px(x + 16, y + 16);
                            copy_16x16_px(x + 16, y);
                        };

                    for xtile in 0..tiles.0 {
                        copy_tile(xtile * tilesize, ytile * tilesize);
                    }
                } else {
                    let mut copy_tile =|x, y| {
                        copy_16x16_px(x + 16, y + 16);
                        copy_16x16_px(x + 16, y);
                        copy_16x16_px(x, y);
                        copy_16x16_px(x, y + 16);
                    };

                    for xtile in (0..tiles.0).rev() {
                        copy_tile(xtile * tilesize, ytile * tilesize);
                    }
                }
            };

            let cropped = img.sub_image(0, 0, size.0, size.1);
            cropped.to_image().save("screenshot.png").unwrap();
        }
    }

    let resource_handles = card.resource_handles().unwrap();

    for crtc in resource_handles.crtcs() {
        let info = card.get_crtc(*crtc).unwrap();
        println!("CRTC Info: {:?}", info);

        if info.mode().is_some() {}
    }
}
