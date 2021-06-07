
use drm_sys::*;
use drm_sys::drm_mode_fb_cmd2;
use std::os::unix::prelude::RawFd;
use drm_ffi::result::SystemError;


#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Hash, PartialEq, Eq)]
pub struct drm_create_bo {
    pub size: __u32,
    pub flags: __u32,
    pub handle : u32,
    pub pad : u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Hash, PartialEq, Eq)]
pub struct drm_mmap_bo {
    pub handle: __u32,
    pub flags: __u32,
    pub offset: __u64,
}


ioctl_readwrite!(drm_vc4_create_bo, DRM_IOCTL_BASE, DRM_COMMAND_BASE + 0x03, drm_create_bo);
ioctl_readwrite!(drm_vc4_mmap_bo, DRM_IOCTL_BASE, DRM_COMMAND_BASE + 0x04, drm_mmap_bo);
ioctl_readwrite!(drm_mode_getfb2, DRM_IOCTL_BASE, 0xCE, drm_mode_fb_cmd2);

pub fn mmap_bo(fd : RawFd, handle : u32) -> Result<u64, SystemError> {
    let mut mmap = drm_mmap_bo {
        handle,
        flags : 0,
        offset : 0
    };

    unsafe {
        drm_vc4_mmap_bo(fd, &mut mmap)?;
    }

    Ok(mmap.offset)
}

pub fn create_bo(fd : RawFd, size: u32, flags : u32) -> Result<u32, SystemError>
{
    let mut create = drm_create_bo {
        size,
        flags,
        handle : 0,
        pad : 0
    };

    unsafe {
        drm_vc4_create_bo(fd, &mut create)?;
    }

    Ok(create.handle)
}

pub fn fb_cmd2(fd : RawFd, handle : u32) -> Result<drm_mode_fb_cmd2, SystemError>
{
    let mut fb = drm_mode_fb_cmd2 {
        fb_id : handle,
        width : 0,
        height : 0,
        pixel_format : 0,
        flags : 0,
        handles : [0;4],
        pitches : [0;4],
        offsets : [0;4],
        modifier : [0;4],
    };

    unsafe {
        drm_mode_getfb2(fd, &mut fb)?;
    }

    Ok(fb)
}
