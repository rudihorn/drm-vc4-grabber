use drm_ffi::result::SystemError;

use drm::control::Device as ControlDevice;
use drm::Device;
use libc::c_void;
use std::os::unix::io::AsRawFd;

pub trait DriverCard: Device + ControlDevice + AsRawFd {}

pub trait Driver {
    fn prepare(&self, handle: u32) -> Result<bool, SystemError>;
    fn mmap(&self, handle: u32, length: u64) -> Result<*mut c_void, SystemError>;
}
