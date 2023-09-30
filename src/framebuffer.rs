use std::cmp::max;

use drm_ffi::drm_mode_fb_cmd2;
use image::{GenericImage, RgbImage};

use crate::image_decoder::{ToRgb, YUV420Pixel};

pub struct YUV420Plane {
    pub pitch: u32,
    pub size: (u32, u32),
    pub offset: u32,
}

impl YUV420Plane {
    pub fn len(&self) -> usize {
        (self.pitch * self.size.1) as _
    }

    pub fn end(&self) -> usize {
        (self.offset as usize) + self.len()
    }

    pub fn offset(&self, x: usize, y: usize) -> usize {
        self.pitch as usize * y + x
    }
}

pub struct YUV420 {
    pub size: (u32, u32),
    pub planes: [YUV420Plane; 3],
}

impl YUV420 {
    pub fn from(fbinfo: drm_mode_fb_cmd2) -> YUV420 {
        let plane = |i| YUV420Plane {
            pitch: fbinfo.pitches[i],
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

pub struct FramebufferYUV420 {
    pub info2: drm_mode_fb_cmd2,
    pub planes: [YUV420Plane; 3],
}

impl FramebufferYUV420 {
    pub fn len(&self) -> usize {
        let mut res = 0;
        for plane in self.planes.iter() {
            res = max(plane.end(), res);
        }
        res as _
    }
}

pub trait Framebuffer<P> {
    fn info<'a>(&'a self) -> &'a drm_mode_fb_cmd2;
    fn get(&self, mappings: [&[u8]; 3], x: usize, y: usize) -> P;
}

impl Framebuffer<YUV420Pixel> for FramebufferYUV420 {
    fn info<'a>(&'a self) -> &'a drm_mode_fb_cmd2 {
        &self.info2
    }

    fn get(&self, mappings: [&[u8]; 3], x: usize, y: usize) -> YUV420Pixel {
        let offset: usize = self.planes[0].offset(x, y);
        let offset1: usize = self.planes[1].offset(x / 2, y / 2);
        let offset2: usize = self.planes[2].offset(x / 2, y / 2);

        YUV420Pixel::new(
            mappings[0][offset],
            mappings[1][offset1],
            mappings[2][offset2],
        )
    }
}

pub struct FramebufferCopy<'a, P> {
    fb: &'a dyn Framebuffer<P>,
}

impl<'a, P> FramebufferCopy<'a, P>
where
    P: ToRgb,
{
    pub fn decode_image(&self, mappings: [&[u8]; 3]) {
        let mut img = RgbImage::new(self.fb.info().width, self.fb.info().height);
        for y in 0..self.fb.info().height {
            for x in 0..self.fb.info().width {
                unsafe {
                    img.unsafe_put_pixel(x, y, self.fb.get(mappings, x as _, y as _).rgb());
                }
            }
        }
    }
}
