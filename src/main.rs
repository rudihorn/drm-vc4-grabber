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

pub mod ffi;
pub mod hyperion;
#[allow(mismatched_lifetime_syntaxes)]
pub mod hyperion_reply_generated;
#[allow(mismatched_lifetime_syntaxes)]
pub mod hyperion_request_generated;
pub mod image_decoder;
pub mod dump_image;

use hyperion::{drain_replies, read_reply, register_direct, send_image};

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

/// Per-frame timing breakdown, emitted when --profile is on.
#[derive(Default, Clone, Copy)]
pub struct FrameStats {
    pub find_fb_us: u64,
    pub decode_us: u64,
    pub send_us: u64,
    pub img_w: u32,
    pub img_h: u32,
}

/// Running aggregator so we can print rolling averages rather than spamming
/// one line per frame.
struct ProfileAggregator {
    frames: u32,
    find_fb_sum: u64,
    decode_sum: u64,
    send_sum: u64,
    decode_max: u64,
    last_img_size: (u32, u32),
    last_print: std::time::Instant,
    format_hint: Option<String>,
}

impl ProfileAggregator {
    fn new() -> Self {
        Self {
            frames: 0,
            find_fb_sum: 0,
            decode_sum: 0,
            send_sum: 0,
            decode_max: 0,
            last_img_size: (0, 0),
            last_print: std::time::Instant::now(),
            format_hint: None,
        }
    }

    fn record(&mut self, s: FrameStats) {
        self.frames += 1;
        self.find_fb_sum += s.find_fb_us;
        self.decode_sum += s.decode_us;
        self.send_sum += s.send_us;
        if s.decode_us > self.decode_max {
            self.decode_max = s.decode_us;
        }
        self.last_img_size = (s.img_w, s.img_h);
    }

    fn set_format(&mut self, fmt: String) {
        self.format_hint = Some(fmt);
    }

    /// Flush an averaged report if enough time has passed. Returns whether
    /// it printed so the caller knows to reset counters.
    fn maybe_flush(&mut self) {
        let elapsed = self.last_print.elapsed();
        if elapsed < std::time::Duration::from_secs(2) {
            return;
        }
        if self.frames == 0 {
            self.last_print = std::time::Instant::now();
            return;
        }
        let n = self.frames as u64;
        let elapsed_s = elapsed.as_secs_f64();
        let actual_fps = (self.frames as f64) / elapsed_s;
        let total_avg_us = (self.find_fb_sum + self.decode_sum + self.send_sum) / n;
        let headroom_fps = if total_avg_us > 0 {
            1_000_000.0 / total_avg_us as f64
        } else {
            f64::INFINITY
        };
        let fmt = self.format_hint.as_deref().unwrap_or("?");
        println!(
            "[profile] fmt={} out={}x{} | actual_fps={:.1} headroom_fps={:.1} | find_fb={}us decode_avg={}us decode_max={}us send={}us | total_avg={}us frames={}",
            fmt,
            self.last_img_size.0,
            self.last_img_size.1,
            actual_fps,
            headroom_fps,
            self.find_fb_sum / n,
            self.decode_sum / n,
            self.decode_max,
            self.send_sum / n,
            total_avg_us,
            self.frames,
        );
        self.frames = 0;
        self.find_fb_sum = 0;
        self.decode_sum = 0;
        self.send_sum = 0;
        self.decode_max = 0;
        self.last_print = std::time::Instant::now();
    }
}

/// Caches per-card DRM topology so we don't re-enumerate CRTCs and planes
/// every frame (those ioctls get expensive during active video playback
/// because they contend with Kodi's continuous framebuffer updates).
///
/// The CRTC and plane handle lists only change on monitor hotplug, so we
/// read them once at startup. We also remember which plane last had a live
/// framebuffer so subsequent frames can short-circuit to that plane with
/// a single ioctl.
struct FramebufferFinder {
    crtcs: Vec<drm::control::crtc::Handle>,
    planes: Vec<drm::control::plane::Handle>,
    last_good_plane: Option<drm::control::plane::Handle>,
    /// Counts consecutive failures of the cached plane; after a few we drop
    /// the cache and fall back to a full scan (handles resolution changes
    /// and other reconfigurations).
    cache_misses: u32,
}

impl FramebufferFinder {
    fn new(card: &Card) -> Option<Self> {
        let resource_handles = card.resource_handles().ok()?;
        let plane_handles = card.plane_handles().ok()?;
        Some(FramebufferFinder {
            crtcs: resource_handles.crtcs().to_vec(),
            planes: plane_handles.planes().to_vec(),
            last_good_plane: None,
            cache_misses: 0,
        })
    }

    /// Find the currently-scanned-out framebuffer. Prefers the plane that
    /// worked on the previous call (fast path, one ioctl), then falls back
    /// to a full scan.
    fn find(&mut self, card: &Card, verbose: bool) -> Option<Handle> {
        // Fast path: try the previously-winning plane first.
        if let Some(plane) = self.last_good_plane {
            if let Ok(info) = card.get_plane(plane) {
                if info.crtc().is_some() {
                    if let Some(fb) = info.framebuffer() {
                        return Some(fb);
                    }
                }
            }
            // Fast path missed. After a few consecutive misses, rebuild the
            // cached plane list in case the topology changed (hotplug, etc.).
            self.cache_misses = self.cache_misses.saturating_add(1);
            if self.cache_misses >= 30 {
                if let Ok(plane_handles) = card.plane_handles() {
                    self.planes = plane_handles.planes().to_vec();
                }
                self.cache_misses = 0;
            }
        }

        // Slow path: walk CRTCs (often none match during plane-only overlays)
        // then planes. Update the cache for next time.
        for crtc in &self.crtcs {
            let info = match card.get_crtc(*crtc) {
                Ok(i) => i,
                Err(e) => {
                    if verbose {
                        eprintln!("get_crtc({:?}) failed: {}", crtc, e);
                    }
                    continue;
                }
            };

            if info.mode().is_some() {
                if let Some(fb) = info.framebuffer() {
                    // CRTCs don't have a plane handle; clear plane cache so
                    // next frame also takes the slow path (CRTC path is rare).
                    self.last_good_plane = None;
                    return Some(fb);
                }
            }
        }

        for plane in &self.planes {
            let info = match card.get_plane(*plane) {
                Ok(i) => i,
                Err(e) => {
                    if verbose {
                        eprintln!("get_plane({:?}) failed: {}", plane, e);
                    }
                    continue;
                }
            };

            if info.crtc().is_some() {
                if let Some(fb) = info.framebuffer() {
                    self.last_good_plane = Some(*plane);
                    self.cache_misses = 0;
                    return Some(fb);
                }
            }
        }

        None
    }
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
        .arg(
            Arg::with_name("profile")
                .long("profile")
                .help("Emit periodic per-frame timing stats (for CPU investigation)."),
        )
        .arg(
            Arg::with_name("unthrottled")
                .long("unthrottled")
                .help("Run the capture loop without the 33ms frame cap. Combine with --profile to see the maximum pipeline throughput; useful for CPU investigation. Do not use in production."),
        )
        .get_matches();

    let verbose = matches.is_present("verbose");
    let profile = matches.is_present("profile");
    let unthrottled = matches.is_present("unthrottled");
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
        let mut finder = match FramebufferFinder::new(&card) {
            Some(f) => f,
            None => {
                eprintln!("Failed to enumerate DRM resources");
                std::process::exit(1);
            }
        };
        if let Some(fb) = finder.find(&card, verbose) {
            let frame = dump_framebuffer_to_image(&card, fb, verbose).unwrap();
            save_screenshot(&frame.image).unwrap();
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
        if profile {
            println!("[profile] profiling enabled, reports every 2s");
        }
        if unthrottled {
            println!("[profile] UNTHROTTLED: frame pacing disabled, loop runs as fast as possible");
        }

        let consecutive_errors = Arc::new(AtomicU32::new(0));
        let mut no_fb_count: u32 = 0;
        let mut prof = ProfileAggregator::new();
        let mut last_drain = Instant::now();
        let mut finder = match FramebufferFinder::new(&card) {
            Some(f) => f,
            None => {
                eprintln!("Failed to enumerate DRM resources at startup");
                std::process::exit(1);
            }
        };

        loop {
            let frame_start = Instant::now();

            // Every ~1s, drain any replies HyperHDR has sent us so its acks
            // don't slowly fill our TCP receive buffer. Non-blocking, cheap.
            if last_drain.elapsed() >= Duration::from_secs(1) {
                if let Err(e) = drain_replies(&mut socket) {
                    if verbose {
                        eprintln!("drain_replies warning: {}", e);
                    }
                }
                last_drain = Instant::now();
            }

            // --- stage: find the active framebuffer handle
            let t0 = Instant::now();
            let fb_opt = finder.find(&card, verbose);
            let find_fb_us = t0.elapsed().as_micros() as u64;

            if let Some(fb) = fb_opt {
                no_fb_count = 0;

                // --- stage: decode framebuffer to RGB image
                let t1 = Instant::now();
                let decode_result = dump_framebuffer_to_image(&card, fb, verbose);
                let decode_us = t1.elapsed().as_micros() as u64;

                match decode_result {
                    Ok(frame) => {
                        let format_label = frame.format_label;
                        let img = frame.image;

                        // --- stage: encode + send to Hyperion
                        let t2 = Instant::now();
                        let send_result = send_image(&mut socket, &img, verbose);
                        let send_us = t2.elapsed().as_micros() as u64;

                        match send_result {
                            Ok(_) => {
                                consecutive_errors.store(0, Ordering::Relaxed);
                                if profile {
                                    prof.set_format(format_label.to_string());
                                    prof.record(FrameStats {
                                        find_fb_us,
                                        decode_us,
                                        send_us,
                                        img_w: img.width(),
                                        img_h: img.height(),
                                    });
                                    prof.maybe_flush();
                                }
                                let elapsed = frame_start.elapsed();
                                if !unthrottled && elapsed < TARGET_FRAME_PERIOD {
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
                                    eprintln!("Send error #{}: {}", errors, e);
                                }
                                let backoff_ms = match errors {
                                    1..=2 => 100,
                                    3..=5 => 500,
                                    _ => 2000,
                                };
                                thread::sleep(Duration::from_millis(backoff_ms));
                            }
                        }
                    }
                    Err(e) => {
                        // Decode failed (e.g. transient unknown format during
                        // HDR metadata switch). Count it and back off, but
                        // don't bail out — the next frame may succeed.
                        let errors = consecutive_errors.fetch_add(1, Ordering::Relaxed) + 1;
                        if verbose || profile {
                            eprintln!("Decode error #{}: {:?}", errors, e);
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
