pub mod driver;
pub mod vc4driver;
pub mod v3ddriver;

use std::os::unix::prelude::{AsRawFd, RawFd};

pub use driver::{Driver, DriverCard};
pub use drm::Device;
pub use vc4driver::VC4Driver;
pub use v3ddriver::V3DDriver;
use drm_ffi::result::SystemError;

#[derive(Debug)]
pub enum DriverError {
    SystemError(SystemError),
    UnknownDriver(String)
}

impl From<SystemError> for DriverError {
    fn from(err: SystemError) -> Self {
        DriverError::SystemError(err)
    }
}

pub enum AnyDriver<Dev> where Dev : DriverCard {
    VC4(VC4Driver<Dev>),
    V3D(V3DDriver<Dev>)
}

impl<Dev> AsRawFd for AnyDriver<Dev> where Dev : DriverCard {
    fn as_raw_fd(&self) -> RawFd {
        match self {
            AnyDriver::VC4(vc4) => vc4.as_raw_fd(),
            AnyDriver::V3D(v3d) => v3d.as_raw_fd()
        }
    }
}

impl<Dev> AnyDriver<Dev> where Dev : DriverCard {
    pub fn of(dev : Dev) -> Result<AnyDriver<Dev>, DriverError> {
        let driver = dev.get_driver()?;
        let driver_name = driver.name().to_str().unwrap();
        let driver = match driver_name {
            "v3d" => Ok(AnyDriver::V3D(V3DDriver::of(dev))),
            "vc4" => Ok(AnyDriver::VC4(VC4Driver::of(dev))),
            _ => Err(DriverError::UnknownDriver(String::from(driver_name)))
        };
        driver
    }

    pub fn dev(&self) -> &Dev {
        match self {
            AnyDriver::VC4(vc4) => vc4.dev(),
            AnyDriver::V3D(v3d) => v3d.dev()
        }
    }
}

impl<Dev> Driver for AnyDriver<Dev> where Dev : DriverCard {
    fn mmap(&self, handle : u32) -> Result<u64, SystemError> {
        match self {
            AnyDriver::VC4(vc4) => vc4.mmap(handle),
            AnyDriver::V3D(v3d) => v3d.mmap(handle),
        }
    }
}
