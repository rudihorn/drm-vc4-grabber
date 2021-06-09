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
            let pitch = fbinfo.pitch();
            let length = size.1 * pitch + 8 * pitch;
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


            let tilesize = 32;

            let tile_count = |n| (n + tilesize - 1) / tilesize;

            let tiles = (tile_count(size.0), tile_count(size.1));
            let mut img = RgbImage::new(tiles.0 * tilesize, tiles.1 * tilesize);

            let total_tiles = tiles.0 * tiles.1;
            let xtiles = size.0 / tilesize;
            let mut i = 0;
            for t in 0..total_tiles {
                let mut tx = t % (xtiles * 2);
                let altrow = tx >= xtiles;
                if altrow {
                    tx = 2 * xtiles - tx - 1;
                }
                let ty = t / xtiles;

                for j in 0..tilesize * tilesize {
                    // each 32x32 tile is subdivided into small tiles of 16x16
                    // and then tiles of 4x4. The 4x4 tiles are in order, the
                    // 16x16 tiles go top left, bottom left, bottom right, top
                    // right. This order seems to be reversed on the second row

                    let not = |x| if x > 0 {0} else {1};

                    let col1 = j % 4;
                    let n = j / 4;
                    let row1 = n % 4;
                    let n = n / 4;
                    let col2 = n % 4;
                    let n = n / 4;
                    let row2 = n % 4;
                    let n = n / 4;
                    let row3 = if n == 1 || n == 2 { 1 } else {0};
                    let row3 = if altrow {not(row3)} else {row3};
                    let col3 = n / 2;
                    let col3 = if altrow {not(col3)} else {col3};

                    let color = Rgb([
                        mapping[(i + 2) as usize],
                        mapping[(i + 1) as usize],
                        mapping[(i + 0) as usize],
                    ]);

                    let xpos = col1 + col2 * 4 + col3 * 16 + tx * tilesize;
                    let ypos = row1 + row2 * 4 + row3 * 16 + ty * tilesize;

                    img.put_pixel(xpos, ypos, color);

                    i = i + 4;
                }
            }

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
