pub mod driver;
pub mod v3ddriver;
pub mod vc4driver;

use std::{
    mem::size_of,
    os::unix::prelude::{AsRawFd, RawFd},
};

pub use driver::{Driver, DriverCard};
pub use drm::Device;
use drm_ffi::result::SystemError;
use libc::c_void;
use nix::sys::mman;
pub use v3ddriver::V3DDriver;
pub use vc4driver::VC4Driver;

#[derive(Debug)]
pub enum DriverError {
    SystemError(SystemError),
    UnknownDriver(String),
}

impl From<SystemError> for DriverError {
    fn from(err: SystemError) -> Self {
        DriverError::SystemError(err)
    }
}

pub enum AnyDriver<Dev>
where
    Dev: DriverCard,
{
    VC4(VC4Driver<Dev>),
    V3D(V3DDriver<Dev>),
}

impl<Dev> AsRawFd for AnyDriver<Dev>
where
    Dev: DriverCard,
{
    fn as_raw_fd(&self) -> RawFd {
        match self {
            AnyDriver::VC4(vc4) => vc4.as_raw_fd(),
            AnyDriver::V3D(v3d) => v3d.as_raw_fd(),
        }
    }
}

impl<Dev> AnyDriver<Dev>
where
    Dev: DriverCard,
{
    pub fn of(dev: Dev) -> Result<AnyDriver<Dev>, DriverError> {
        let driver = dev.get_driver()?;
        let driver_name = driver.name().to_str().unwrap();
        let driver = match driver_name {
            "v3d" => Ok(AnyDriver::V3D(V3DDriver::of(dev))),
            "vc4" => Ok(AnyDriver::VC4(VC4Driver::of(dev))),
            _ => Err(DriverError::UnknownDriver(String::from(driver_name))),
        };
        driver
    }

    pub fn dev(&self) -> &Dev {
        match self {
            AnyDriver::VC4(vc4) => vc4.dev(),
            AnyDriver::V3D(v3d) => v3d.dev(),
        }
    }

    /// Map the specified framebuffer handle to memory using the length of the
    /// given byte slice, copy the contents of the memory buffer to the slice.
    /// Copying the buffer is a good idea in any case since random access on the
    /// original buffer seems slow.
    pub fn copy<T: Sized + Copy>(
        &self,
        handle: u32,
        to: &mut [T],
        verbose: bool,
    ) -> Result<(), SystemError> {
        if !self.prepare(handle).unwrap_or(true) && verbose {
            println!("Could not prepare buffer for mmaping, the buffer may be purged.");
        }
        println!("Using handle {}", handle);

        let length = to.len() * size_of::<T>();
        let map = self.mmap(handle, length as _)?;

        unsafe {
            let mapping: &mut [T] = std::slice::from_raw_parts_mut(map as *mut _, to.len());
            to.copy_from_slice(mapping);
            mman::munmap(map, length as _).unwrap();
        };

        Ok(())
    }
}

impl<Dev> Driver for AnyDriver<Dev>
where
    Dev: DriverCard,
{
    fn prepare(&self, handle: u32) -> Result<bool, SystemError> {
        match self {
            AnyDriver::VC4(vc4) => vc4.prepare(handle),
            AnyDriver::V3D(v3d) => v3d.prepare(handle),
        }
    }

    fn mmap(&self, handle: u32, length: u64) -> Result<*mut c_void, SystemError> {
        match self {
            AnyDriver::VC4(vc4) => vc4.mmap(handle, length),
            AnyDriver::V3D(v3d) => v3d.mmap(handle, length),
        }
    }
}
