use super::driver::Driver;
use drm_ffi::result::SystemError;
use libc::c_void;
use std::os::unix::{io::AsRawFd, prelude::RawFd};

use super::driver::DriverCard;

pub enum Madvise {
    WillNeed = 0,
    DontNeed = 1,
}

mod drmvc4 {
    use crate::ffi::prime_handle_to_fd;

    use super::Madvise;
    use drm_ffi::result::SystemError;
    use drm_sys::*;
    use libc::c_void;
    use nix::sys::mman;
    use std::os::unix::prelude::RawFd;

    #[repr(C)]
    #[derive(Debug, Default, Copy, Clone, Hash, PartialEq, Eq)]
    pub struct DrmCreateBo {
        pub size: __u32,
        pub flags: __u32,
        pub handle: u32,
        pub pad: u32,
    }

    #[repr(C)]
    #[derive(Debug, Default, Copy, Clone, Hash, PartialEq, Eq)]
    pub struct DrmMmapBo {
        pub handle: __u32,
        pub flags: __u32,
        pub offset: __u64,
    }

    #[repr(C)]
    #[derive(Debug, Default, Copy, Clone, Hash, PartialEq, Eq)]
    pub struct DrmGemMadvise {
        pub handle: __u32,
        pub madv: __u32,
        pub retained: __u32,
        pub pad: __u32,
    }

    ioctl_readwrite!(
        drm_vc4_create_bo,
        DRM_IOCTL_BASE,
        DRM_COMMAND_BASE + 0x03,
        DrmCreateBo
    );
    ioctl_readwrite!(
        drm_vc4_mmap_bo,
        DRM_IOCTL_BASE,
        DRM_COMMAND_BASE + 0x04,
        DrmMmapBo
    );
    ioctl_readwrite!(
        drm_vc4_gem_madvise,
        DRM_IOCTL_BASE,
        DRM_COMMAND_BASE + 0x0b,
        DrmGemMadvise
    );

    pub fn mmap_bo(fd: RawFd, handle: u32, length: u64) -> Result<*mut c_void, SystemError> {

        let hfd = prime_handle_to_fd(fd, handle)?;
        println!("handle fd {}", hfd);

        let addr = core::ptr::null_mut();
        let prot = mman::ProtFlags::PROT_READ;
        let flags = mman::MapFlags::MAP_SHARED;
        unsafe {
            let map = mman::mmap(
                addr,
                length as _,
                prot,
                flags,
                hfd,
                0,
            ).unwrap();
            return Ok(map);
        }
    }

    pub fn gem_madvise(fd: RawFd, handle: u32, madv: Madvise) -> Result<bool, SystemError> {
        let madv = madv as i32;
        let mut madvise = DrmGemMadvise {
            handle,
            madv: madv as u32,
            retained: 0,
            pad: 0,
        };

        unsafe {
            drm_vc4_gem_madvise(fd, &mut madvise)?;
        }

        Ok(madvise.retained != 0)
    }
}

pub struct VC4Driver<Dev>
where
    Dev: DriverCard,
{
    device: Dev,
}

impl<Dev> AsRawFd for VC4Driver<Dev>
where
    Dev: DriverCard,
{
    fn as_raw_fd(&self) -> RawFd {
        self.device.as_raw_fd()
    }
}

impl<Dev> VC4Driver<Dev>
where
    Dev: DriverCard,
{
    pub fn dev(&self) -> &Dev {
        &self.device
    }

    pub fn of(device: Dev) -> VC4Driver<Dev> {
        VC4Driver { device }
    }

    pub fn gem_madvise(&self, handle: u32, madv: Madvise) -> Result<bool, SystemError> {
        drmvc4::gem_madvise(self.as_raw_fd(), handle, madv)
    }
}

impl<Dev> Driver for VC4Driver<Dev>
where
    Dev: DriverCard,
{
    fn prepare(&self, handle: u32) -> Result<bool, SystemError> {
        self.gem_madvise(handle, Madvise::WillNeed)
    }

    fn mmap(&self, handle: u32, length: u64) -> Result<*mut c_void, SystemError> {
        drmvc4::mmap_bo(self.device.as_raw_fd(), handle, length)
    }
}
