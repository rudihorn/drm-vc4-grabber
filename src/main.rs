#[macro_use]
extern crate nix;

use std::fs::{File, OpenOptions};
use std::net::TcpStream;

use drm::control::framebuffer::Handle;
use drm::control::Device as ControlDevice;
use drm::Device;

use image::RgbImage;
use nix::sys::mman;
use nix::unistd::sleep;

use std::os::unix::io::{AsRawFd, RawFd};

use std::io::Result as StdResult;

pub mod ffi;
pub mod hyperion;
pub mod hyperion_reply_generated;
pub mod hyperion_request_generated;
pub mod image_decoder;

pub use hyperion_request_generated::hyperionnet::{Clear, Color, Command, Image, Register};

use hyperion::{read_reply, register_direct, send_color_red, send_image};
use image_decoder::decode_tiled_small_image;

use crate::image_decoder::decode_small_image;

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

fn dump_buffer_to_image(
    card: &mut Card,
    tiled: bool,
    size: (u32, u32),
    bpp: u32,
    handle: u32,
) -> RgbImage {
    let offset = ffi::mmap_bo(card.as_raw_fd(), handle).unwrap();

    let tilesize = 32;
    let tile_count = |n| (n + tilesize - 1) / tilesize;
    let tiles = (tile_count(size.0), tile_count(size.1));
    let total_tiles = tiles.0 * tiles.1;

    let length = if tiled {
        total_tiles * tilesize * tilesize * (bpp / 8)
    } else {
        size.0 * size.1 // * (bpp / 8)
    };
    println!(
        "  -> Offset: {}, Tiles {:?}, Length {}",
        offset, tiles, length
    );

    let map = {
        let addr = core::ptr::null_mut();
        //let length = length;
        let prot = mman::ProtFlags::PROT_READ | mman::ProtFlags::PROT_WRITE;
        let flags = mman::MapFlags::MAP_SHARED;
        let offset = offset as _;
        unsafe { mman::mmap(addr, length as _, prot, flags, card.as_raw_fd(), offset).unwrap() }
    };

    let img = if tiled {
        let mut copy = vec![0; (length / 4) as _];
        let mapping: &mut [u32] =
            unsafe { std::slice::from_raw_parts_mut(map as *mut _, (length / 4) as _) };
        copy.copy_from_slice(mapping);
        decode_tiled_small_image(copy.as_mut_slice(), tilesize, tiles, size)
    } else {
        let mut copy = vec![0; length as _];
        let mapping: &mut [u8] =
            unsafe { std::slice::from_raw_parts_mut(map as *mut _, length as _) };
        copy.copy_from_slice(mapping);

        decode_small_image(copy.as_mut_slice(), size)
    };

    unsafe { mman::munmap(map, length as _).unwrap() };

    img
}

fn dump_framebuffer_to_image(card: &mut Card, fb: Handle) -> RgbImage {
    let fbinfo2 = ffi::fb_cmd2(card.as_raw_fd(), fb.into()).unwrap();
    println!("  -> FB Info 2: {:?}", fbinfo2);
    //let fbinfo = card.get_framebuffer(fb).unwrap();

    let size = (fbinfo2.width, fbinfo2.height);
    let tiled = fbinfo2.modifier[0] > 0;
    let bpp = match fbinfo2.pixel_format {
        0x32315559 => 16,
        _ => 32,
    };

    dump_buffer_to_image(card, tiled, size, bpp, fbinfo2.handles[0])
}

fn send_dumped_image(socket: &mut TcpStream, img: &RgbImage) -> StdResult<()> {
    //img.save("screenshot.png").unwrap();

    register_direct(socket)?;
    read_reply(socket)?;

    send_image(socket, img)?;

    Ok(())
}

fn dump_and_send_framebuffer(socket: &mut TcpStream, card: &mut Card, fb: Handle) -> StdResult<()> {
    let img = dump_framebuffer_to_image(card, fb);
    send_dumped_image(socket, &img)?;

    Ok(())
}

fn main() {
    let mut card = Card::open_global();
    let driver = card.get_driver().unwrap();
    println!("Driver: {:?}", driver);

    let mut socket = TcpStream::connect("127.0.0.1:19400").unwrap();
    register_direct(&mut socket).unwrap();
    read_reply(&mut socket).unwrap();

    send_color_red(&mut socket).unwrap();
    sleep(1);

    let plane_handles = card.plane_handles().unwrap();

    loop {
        let resource_handles = card.resource_handles().unwrap();

        let mut already_sent = false;

        for crtc in resource_handles.crtcs() {
            let info = card.get_crtc(*crtc).unwrap();
            println!("CRTC Info: {:?}", info);

            if info.mode().is_some() {
                if let Some(fb) = info.framebuffer() {
                    dump_and_send_framebuffer(&mut socket, &mut card, fb).unwrap();
                    /*
                    let img = dump_buffer_to_image(&mut card, (1920, 1080), 32, fb.into());
                    send_dumped_image(&mut socket, &img).unwrap();
                    */
                    already_sent = true;
                }
            }
        }

        if !already_sent {
            for plane in plane_handles.planes() {
                let info = card.get_plane(*plane).unwrap();
                println!("Plane Info: {:?}", info);

                if info.crtc().is_some() {
                    let fb = info.framebuffer().unwrap();

                    dump_and_send_framebuffer(&mut socket, &mut card, fb).unwrap();
                }
            }
        }
    }
}
