#[macro_use]
extern crate nix;

use std::fs::{File, OpenOptions};
use std::usize;

use drm::control::Device as ControlDevice;
use drm::Device;

use image::{RgbImage, Rgb};

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
            let length = size.1 * pitch + 8*pitch;
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

            let mapping : & mut [u8] = unsafe { std::slice::from_raw_parts_mut(map as *mut _, length as _) };

            let mut img = RgbImage::new(size.0, size.1);

            let tilesize = 32;

            let tiles = size.0 * size.1 / (tilesize * tilesize);
            let xtiles = size.0 / tilesize;
            let mut i = 0;
            for t in 0..tiles {
                let mut tx = t % (xtiles * 2);
                if tx >= xtiles {
                    tx = 2 * xtiles - tx - 1;
                }
                let ty = t / xtiles;

                for y in 0..tilesize {
                    for x in 0..tilesize{
                        let col = Rgb([
                            mapping[(i+2) as usize],
                            mapping[(i+1) as usize],
                            mapping[(i+0) as usize]
                        ]);
                        if y + ty*tilesize < size.1 {
                            img.put_pixel(x + tx*tilesize, y + ty*tilesize, col);
                        }
                        i = i + 4;
                    }
                }
            }

            img.save("screenshot.png").unwrap();
        }
    }

    let resource_handles = card.resource_handles().unwrap();

    for crtc in resource_handles.crtcs() {
        let info = card.get_crtc(*crtc).unwrap();
        println!("CRTC Info: {:?}", info);

        if info.mode().is_some() {}
    }
}
