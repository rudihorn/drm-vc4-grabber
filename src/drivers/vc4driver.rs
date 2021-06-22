use super::driver::Driver;
use drm_ffi::result::SystemError;
use std::os::unix::{io::AsRawFd, prelude::RawFd};

use super::driver::DriverCard;

pub enum Madvise {
    WillNeed = 0,
    DontNeed = 1,
}

mod drmvc4 {
    use super::Madvise;
    use drm_ffi::result::SystemError;
    use drm_sys::*;
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

    pub fn mmap_bo(fd: RawFd, handle: u32) -> Result<u64, SystemError> {
        let mut mmap = DrmMmapBo {
            handle,
            flags: 0,
            offset: 0,
        };

        unsafe {
            drm_vc4_mmap_bo(fd, &mut mmap)?;
        }

        Ok(mmap.offset)
    }

    pub fn _create_bo(fd: RawFd, size: u32, flags: u32) -> Result<u32, SystemError> {
        let mut create = DrmCreateBo {
            size,
            flags,
            handle: 0,
            pad: 0,
        };

        unsafe {
            drm_vc4_create_bo(fd, &mut create)?;
        }

        Ok(create.handle)
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

    fn mmap(&self, handle: u32) -> Result<u64, SystemError> {
        drmvc4::mmap_bo(self.device.as_raw_fd(), handle)
    }
}
