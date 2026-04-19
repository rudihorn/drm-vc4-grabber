#[macro_use]
extern crate nix;

use std::fs::{File, OpenOptions};
use std::net::TcpStream;
use std::os::fd::AsFd;

use clap::{App, Arg};
use drm::control::framebuffer::Handle;
use drm::control::Device as ControlDevice;
use drm::Device;
use drm_ffi::drm_set_client_cap;

use dump_image::dump_framebuffer_to_image;
use image::{ImageError, RgbImage};

use std::os::unix::io::{AsRawFd, RawFd};
use std::{thread, time::Duration};

use std::io::Result as StdResult;

pub mod ffi;
pub mod hyperion;
#[allow(mismatched_lifetime_syntaxes)]
pub mod hyperion_reply_generated;
#[allow(mismatched_lifetime_syntaxes)]
pub mod hyperion_request_generated;
pub mod image_decoder;
pub mod dump_image;

use hyperion::{read_reply, register_direct, send_image};

pub struct Card(File);

impl AsRawFd for Card {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl AsFd for Card {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl Device for Card {}
impl ControlDevice for Card {}

impl Card {
    pub fn open(path: &str) -> std::io::Result<Self> {
        let mut options = OpenOptions::new();
        options.read(true);
        options.write(false);
        Ok(Card(options.open(path)?))
    }
}

fn save_screenshot(img: &RgbImage) -> Result<(), ImageError> {
    img.save("screenshot.png")
}

/// Send an already-captured image. Assumes the socket is already registered.
/// Registration happens once per connection in `main`, not per-frame — doing it
/// per-frame would double the round-trips and spam Hyperion's priority registry.
fn send_dumped_image(socket: &mut TcpStream, img: &RgbImage, verbose: bool) -> StdResult<()> {
    send_image(socket, img, verbose)?;
    Ok(())
}

/// Capture the framebuffer and forward it to Hyperion. Decode failures are
/// surfaced as errors so the capture loop's backoff logic kicks in instead of
/// silently dropping frames (which eventually triggers Hyperion's rainbow).
fn dump_and_send_framebuffer(
    socket: &mut TcpStream,
    card: &Card,
    fb: Handle,
    verbose: bool,
) -> StdResult<()> {
    match dump_framebuffer_to_image(card, fb, verbose) {
        Ok(img) => send_dumped_image(socket, &img, verbose),
        Err(e) => {
            if verbose {
                eprintln!("Error dumping framebuffer to image: {:?}", e);
            }
            // Translate DRM error into io::Error so the main loop's match can
            // classify it (same as any other capture failure).
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("framebuffer decode failed: {:?}", e),
            ))
        }
    }
}

fn find_framebuffer(card: &Card, verbose: bool) -> Option<Handle> {
    // Resource handles can fail transiently during mode-set events — return None
    // rather than panicking so the main loop can retry.
    let resource_handles = match card.resource_handles() {
        Ok(h) => h,
        Err(e) => {
            if verbose {
                eprintln!("resource_handles failed: {}", e);
            }
            return None;
        }
    };

    for crtc in resource_handles.crtcs() {
        let info = match card.get_crtc(*crtc) {
            Ok(i) => i,
            Err(e) => {
                if verbose {
                    eprintln!("get_crtc({:?}) failed: {}", crtc, e);
                }
                continue;
            }
        };

        if verbose {
            println!("CRTC Info: {:?}", info);
        }

        if info.mode().is_some() {
            if let Some(fb) = info.framebuffer() {
                return Some(fb);
            }
        }
    }

    let plane_handles = match card.plane_handles() {
        Ok(h) => h,
        Err(e) => {
            if verbose {
                eprintln!("plane_handles failed: {}", e);
            }
            return None;
        }
    };

    for plane in plane_handles.planes() {
        let info = match card.get_plane(*plane) {
            Ok(i) => i,
            Err(e) => {
                if verbose {
                    eprintln!("get_plane({:?}) failed: {}", plane, e);
                }
                continue;
            }
        };

        if verbose {
            println!("Plane Info: {:?}", info);
        }

        // A plane may have a CRTC attached but no framebuffer during transitions
        // (HDR metadata changes, scene switches). Skip silently instead of panicking.
        if info.crtc().is_some() {
            if let Some(fb) = info.framebuffer() {
                return Some(fb);
            }
        }
    }

    None
}

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Target capture rate. 30 FPS is the practical upper bound for Hyperion — it
/// smooths LEDs internally and higher rates just burn CPU on the Pi.
const TARGET_FRAME_PERIOD: Duration = Duration::from_millis(33);

fn main() {
    let matches = App::new("DRM VC4 Screen Grabber for Hyperion")
        .version("0.1.3")
        .author("Rudi Horn <dyn-git@rudi-horn.de>")
        .about("Captures a screenshot and sends it to the Hyperion or HyperHDR server.")
        .arg(
            Arg::with_name("device")
                .short("d")
                .long("device")
                .default_value("/dev/dri/card1")
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
    let card = match Card::open(device_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to open DRM device '{}': {}", device_path, e);
            std::process::exit(1);
        }
    };
    let authenticated = card.authenticated().unwrap_or(false);

    if verbose {
        if let Ok(driver) = card.get_driver() {
            println!("Driver (auth={}): {:?}", authenticated, driver);
        }
    }

    // Enable universal planes so we can see overlay/cursor planes too. Without
    // this we only see the primary plane which is often not where video lands.
    unsafe {
        let set_cap = drm_set_client_cap {
            capability: drm_ffi::DRM_CLIENT_CAP_UNIVERSAL_PLANES as u64,
            value: 1,
        };
        if let Err(e) = drm_ffi::ioctl::set_cap(card.as_raw_fd(), &set_cap) {
            eprintln!("Warning: could not enable UNIVERSAL_PLANES: {}", e);
        }
    }

    let address = matches.value_of("address").unwrap();
    
    if screenshot {
        if let Some(fb) = find_framebuffer(&card, verbose) {
            let img = dump_framebuffer_to_image(&card, fb, verbose).unwrap();
            save_screenshot(&img).unwrap();
        } else {
            println!("No framebuffer found!");
        }
    } else {
        // Establish the Hyperion/HyperHDR connection and register just once.
        // Re-registering on every frame (as the original code did) doubles
        // round-trips per frame and spams the server's priority registry.
        let mut socket = TcpStream::connect(address).unwrap();
        register_direct(&mut socket).unwrap();
        read_reply(&mut socket, verbose).unwrap();

        if verbose {
            println!("Connected to Hyperion, starting capture loop");
        }

        let consecutive_errors = Arc::new(AtomicU32::new(0));
        let mut no_fb_count: u32 = 0;

        loop {
            let frame_start = Instant::now();

            if let Some(fb) = find_framebuffer(&card, verbose) {
                no_fb_count = 0;
                match dump_and_send_framebuffer(&mut socket, &card, fb, verbose) {
                    Ok(_) => {
                        consecutive_errors.store(0, Ordering::Relaxed);
                        // Delta-time pacing: sleep only the remainder of the
                        // target period, so slow captures don't compound into
                        // an even lower effective FPS.
                        let elapsed = frame_start.elapsed();
                        if elapsed < TARGET_FRAME_PERIOD {
                            thread::sleep(TARGET_FRAME_PERIOD - elapsed);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                        eprintln!("Hyperion disconnected. Reconnecting...");
                        consecutive_errors.store(0, Ordering::Relaxed);
                        thread::sleep(Duration::from_secs(2));

                        match TcpStream::connect(address) {
                            Ok(new_socket) => {
                                socket = new_socket;
                                // Re-register on the fresh connection.
                                if let Err(e) = register_direct(&mut socket) {
                                    eprintln!("Re-register failed: {}", e);
                                    continue;
                                }
                                if let Err(e) = read_reply(&mut socket, verbose) {
                                    eprintln!("Re-register read_reply failed: {}", e);
                                    continue;
                                }
                                eprintln!("Reconnected to Hyperion");
                            }
                            Err(e) => {
                                eprintln!("Reconnection failed: {}. Will retry...", e);
                            }
                        }
                    }
                    Err(e) => {
                        let errors = consecutive_errors.fetch_add(1, Ordering::Relaxed) + 1;

                        if verbose {
                            eprintln!("Capture error #{}: {}", errors, e);
                        }

                        let backoff_ms = match errors {
                            1..=2 => 100,
                            3..=5 => 500,
                            _ => 2000,
                        };
                        thread::sleep(Duration::from_millis(backoff_ms));
                    }
                }
            } else {
                no_fb_count += 1;

                if verbose {
                    eprintln!("No framebuffer found (count: {}), waiting...", no_fb_count);
                }

                // No framebuffer usually means the compositor is transitioning.
                // Sleeping silently preserves the last LED state rather than
                // forcing a colour that would jank the wall.
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}
