#[macro_use]
extern crate nix;

use std::convert::TryFrom;
use std::fs::{File, OpenOptions};
use std::marker::PhantomData;
use std::net::TcpStream;

use clap::{App, Arg};
use drivers::DriverCard;
use drm::control::framebuffer::Handle;
use drm::control::Device as ControlDevice;
use drm::Device;
use drm_fourcc::{DrmFourcc, DrmModifier};

use framebuffer::{Framebuffer, YUV420};
use image::{ImageError, RgbImage};
use nix::unistd::sleep;

use std::os::unix::io::{AsRawFd, RawFd};

use std::io::Result as StdResult;

pub mod drivers;

pub mod ffi;
pub mod framebuffer;
pub mod hyperion;
pub mod hyperion_reply_generated;
pub mod hyperion_request_generated;
pub mod image_decoder;

pub use hyperion_request_generated::hyperionnet::{Clear, Color, Command, Image, Register};

use hyperion::{read_reply, register_direct, send_color_red, send_image};
use image_decoder::{decode_small_image_multichannel, decode_tiled_small_image};

use crate::drivers::AnyDriver;
use crate::image_decoder::{decode_image, decode_image_multichannel};

struct Card(File);

impl AsRawFd for Card {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl Device for Card {}
impl ControlDevice for Card {}
impl DriverCard for Card {}

impl Card {
    pub fn open(path: &str) -> Self {
        let mut options = OpenOptions::new();
        options.read(true);
        options.write(true);
        Card(options.open(path).unwrap())
    }
}

fn dump_linear_to_image(
    driver: &mut AnyDriver<Card>,
    pitch: u32,
    size: (u32, u32),
    bpp: u32,
    handle: u32,
    verbose: bool,
) -> RgbImage {
    let size = (size.0, size.1/64);

    let length = pitch * size.1 / (bpp / 8);

    println!("size: {:?}, pitch: {}, bpp: {}, length: {}", size, pitch, bpp, length);
    let mut copy = vec![0u32; length as _];
    driver.copy(handle, &mut copy, verbose).unwrap();

    decode_image(copy.as_mut_slice(), pitch, size)
}

fn dump_broadcom_tiled_to_image(
    driver: &mut AnyDriver<Card>,
    size: (u32, u32),
    bpp: u32,
    handle: u32,
    verbose: bool,
) -> RgbImage {
    let tilesize = 32;
    let tile_count = |n| (n + tilesize - 1) / tilesize;
    let tiles = (tile_count(size.0), tile_count(size.1));
    let total_tiles = tiles.0 * tiles.1;

    let length = total_tiles * tilesize * tilesize * (bpp / 8);

    let mut copy = vec![0; (length / 4) as _];
    driver.copy(handle, &mut copy, verbose).unwrap();

    decode_tiled_small_image(copy.as_mut_slice(), tilesize, tiles, size)
}

fn dump_yuv420_to_image(
    driver: &mut AnyDriver<Card>,
    size: (u32, u32),
    pitches: [u32; 4],
    handles: [u32; 4],
    offsets: [u32; 4],
    verbose: bool,
) -> RgbImage {
    // The length of the entire buffer is the length of the last buffer plus its
    // offset (assuming they are in order). The U and V buffers are grouped into
    // 2x2 tiles, hence the length is divided by 4.
    let length = offsets[2] + size.1 * pitches[2] * pitches[2] / pitches[0];
    //println!("  -> Mounting @{} +{}", offset, length);

    let mut copy = vec![0; length as _];
    driver.copy(handles[0], &mut copy, verbose).unwrap();

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
        decode_small_image_multichannel(mappings, size, pitches1)
    } else {
        decode_image_multichannel(mappings, size, pitches1)
    }
}

fn dump_framebuffer_to_image(driver: &mut AnyDriver<Card>, fb: Handle, verbose: bool) -> RgbImage {
    let fbinfo2 = ffi::fb_cmd2(driver.dev().as_raw_fd(), fb.into()).unwrap();

    if verbose {
        println!("  -> FB Info 2: {:?}", fbinfo2);
    }

    let size = (fbinfo2.width, fbinfo2.height);
    let tiled = fbinfo2.modifier[0] > 0;

    let fourcc = drm_fourcc::DrmFourcc::try_from(fbinfo2.pixel_format).unwrap();
    let modifier = drm_fourcc::DrmModifier::try_from(fbinfo2.modifier[0]).unwrap();

    match fourcc {
        DrmFourcc::Xrgb8888 => match modifier {
            DrmModifier::Broadcom_vc4_t_tiled => {
                dump_broadcom_tiled_to_image(driver, size, 32, fbinfo2.handles[0], verbose)
            }
            DrmModifier::Linear => dump_linear_to_image(
                driver,
                fbinfo2.pitches[0],
                size,
                32,
                fbinfo2.handles[0],
                verbose,
            ),
            _ => panic!("Unknown framebuffer modifier: {:?}", modifier),
        },
        DrmFourcc::Yuv420 => dump_yuv420_to_image(
            driver,
            size,
            fbinfo2.pitches,
            fbinfo2.handles,
            fbinfo2.offsets,
            verbose,
        ),
        _ => panic!("Unknown framebuffer pixel format: {}", fourcc),
    }
}

fn save_screenshot(img: &RgbImage) -> Result<(), ImageError> {
    img.save("screenshot.png")
}

fn send_dumped_image(socket: &mut TcpStream, img: &RgbImage) -> StdResult<()> {
    register_direct(socket)?;
    read_reply(socket)?;

    send_image(socket, img)?;

    Ok(())
}

fn dump_and_send_framebuffer(
    socket: &mut TcpStream,
    driver: &mut AnyDriver<Card>,
    fb: Handle,
    verbose: bool,
) -> StdResult<()> {
    let img = dump_framebuffer_to_image(driver, fb, verbose);
    send_dumped_image(socket, &img)?;

    Ok(())
}

fn find_framebuffer(driver: &mut AnyDriver<Card>, verbose: bool) -> Option<Handle> {
    let resource_handles = driver.dev().resource_handles().unwrap();

    for crtc in resource_handles.crtcs() {
        let info = driver.dev().get_crtc(*crtc).unwrap();

        if verbose {
            println!("CRTC Info: {:?}", info);
        }

        if info.mode().is_some() {
            if let Some(fb) = info.framebuffer() {
                return Some(fb);
            }
        }
    }

    let plane_handles = driver.dev().plane_handles().unwrap();

    for plane in plane_handles.planes() {
        let info = driver.dev().get_plane(*plane).unwrap();

        if verbose {
            println!("Plane Info: {:?}", info);
        }

        if info.crtc().is_some() {
            let fb = info.framebuffer().unwrap();

            return Some(fb);
        }
    }

    None
}

fn main() {
    let matches = App::new("DRM VC4 Screen Grabber for Hyperion")
        .version("0.1.0")
        .author("Rudi Horn <dyn-git@rudi-horn.de>")
        .about("Captures a screenshot and sends it to the Hyperion server.")
        .arg(
            Arg::with_name("device")
                .short("d")
                .long("device")
                .default_value("/dev/dri/card0")
                .takes_value(true)
                .help("The device path of the DRM device to capture the image from."),
        )
        .arg(
            Arg::with_name("address")
                .short("a")
                .long("address")
                .default_value("127.0.0.1:19400")
                .takes_value(true)
                .help("The Hyperion TCP socket address to send the captured screenshots to."),
        )
        .arg(
            Arg::with_name("screenshot")
                .long("screenshot")
                .takes_value(false)
                .help("Capture a screenshot and save it to screenshot.png"),
        )
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .help("Print verbose debugging information."),
        )
        .get_matches();

    let verbose = matches.is_present("verbose");
    let screenshot = matches.is_present("screenshot");
    let device_path = matches.value_of("device").unwrap();
    let card = Card::open(device_path);
    let authenticated = card.authenticated().unwrap();

    if verbose {
        let driver = card.get_driver().unwrap();
        println!("Driver (auth={}): {:?}", authenticated, driver);
    }

    let mut driver = AnyDriver::of(card).unwrap();

    if !authenticated {
        let auth_token = driver.dev().generate_auth_token().unwrap();
        driver.dev().authenticate_auth_token(auth_token).unwrap();
    }

    let adress = matches.value_of("address").unwrap();
    if screenshot {
        if let Some(fb) = find_framebuffer(&mut driver, verbose) {
            let img = dump_framebuffer_to_image(&mut driver, fb, verbose);
            save_screenshot(&img).unwrap();
        } else {
            println!("No framebuffer found!");
        }
    } else {
        let mut socket = TcpStream::connect(adress).unwrap();
        register_direct(&mut socket).unwrap();
        read_reply(&mut socket).unwrap();

        send_color_red(&mut socket).unwrap();
        sleep(1);

        loop {
            if let Some(fb) = find_framebuffer(&mut driver, verbose) {
                dump_and_send_framebuffer(&mut socket, &mut driver, fb, verbose).unwrap();
            } else {
                sleep(1);
            }
        }
    }
}
