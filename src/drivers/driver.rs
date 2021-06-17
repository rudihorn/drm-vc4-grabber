use drm_ffi::result::SystemError;

use drm::control::Device as ControlDevice;
use std::os::unix::io::AsRawFd;
use drm::Device;

pub trait DriverCard : Device + ControlDevice + AsRawFd {}

pub trait Driver {
    fn mmap(&self, handle : u32) -> Result<u64, SystemError>;
}

