use super::{driver::Driver, DriverCard};
use drm::control::Device as ControlDevice;
use drm::Device;
use drm_ffi::result::SystemError;
use std::os::unix::{io::AsRawFd, prelude::RawFd};

mod drmv3d {
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

    ioctl_readwrite!(
        drm_vc4_create_bo,
        DRM_IOCTL_BASE,
        DRM_COMMAND_BASE + 0x02,
        DrmCreateBo
    );
    ioctl_readwrite!(
        drm_vc4_mmap_bo,
        DRM_IOCTL_BASE,
        DRM_COMMAND_BASE + 0x03,
        DrmMmapBo
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
}

pub struct V3DDriver<Dev>
where
    Dev: Device + ControlDevice + AsRawFd,
{
    device: Dev,
}

impl<Dev> AsRawFd for V3DDriver<Dev>
where
    Dev: DriverCard,
{
    fn as_raw_fd(&self) -> RawFd {
        self.device.as_raw_fd()
    }
}

impl<Dev> V3DDriver<Dev>
where
    Dev: Device + ControlDevice + AsRawFd,
{
    pub fn dev(&self) -> &Dev {
        &self.device
    }

    pub fn of(device: Dev) -> V3DDriver<Dev> {
        V3DDriver { device }
    }
}

impl<Dev> Driver for V3DDriver<Dev>
where
    Dev: Device + ControlDevice + AsRawFd,
{
    fn prepare(&self, _handle: u32) -> Result<bool, SystemError> {
        Ok(true)
    }

    fn mmap(&self, handle: u32) -> Result<u64, SystemError> {
        drmv3d::mmap_bo(self.device.as_raw_fd(), handle)
    }
}
