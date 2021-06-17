use drm_ffi::result::SystemError;

use drm::control::Device as ControlDevice;
use drm::Device;
use std::os::unix::io::AsRawFd;

pub trait DriverCard: Device + ControlDevice + AsRawFd {}

pub trait Driver {
    fn mmap(&self, handle: u32) -> Result<u64, SystemError>;
}
