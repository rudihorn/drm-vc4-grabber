use drm_ffi::drm_mode_fb_cmd2;

pub struct YUV420Plane {
    pub size: (u32, u32),
    pub offset: u32,
}

impl YUV420Plane {
    pub fn length(&self) -> usize {
        (self.size.0 * self.size.1) as _
    }
}

pub struct YUV420 {
    pub size: (u32, u32),
    pub planes: [YUV420Plane; 3],
}

impl YUV420 {
    pub fn from(fbinfo: drm_mode_fb_cmd2) -> YUV420 {
        let plane = |i| YUV420Plane {
            size: (
                fbinfo.pitches[i],
                fbinfo.height * fbinfo.pitches[i] / fbinfo.pitches[0],
            ),
            offset: fbinfo.offsets[i],
        };
        YUV420 {
            size: (fbinfo.width, fbinfo.height),
            planes: [plane(0), plane(1), plane(2)],
        }
    }
}

pub struct Framebuffer {
    pub info2: drm_mode_fb_cmd2,
}

impl Framebuffer {
    pub fn length() -> usize {0}
}
