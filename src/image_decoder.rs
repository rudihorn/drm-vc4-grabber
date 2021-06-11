use image::{GenericImage, Rgb, RgbImage};

struct PixelAverage {
    avg_rb: u32,
    avg_g: u32,
}

impl PixelAverage {
    pub fn new() -> PixelAverage {
        PixelAverage {
            avg_rb: 0,
            avg_g: 0,
        }
    }

    pub fn add(&mut self, rgb: u32) {
        let rb = rgb & 0x00FF00FF;
        let g = rgb & 0x0000FF00;
        self.avg_rb += rb;
        self.avg_g += g;
    }

    pub fn rgb(self) -> Rgb<u8> {
        let rb = self.avg_rb / 16;
        let g = (self.avg_g / 16) >> 8;
        let r = rb;
        let b = rb >> 16;

        Rgb([r as _, g as _, b as _])
    }
}

pub fn decode_small_image(mapping: &[u8], size: (u32, u32)) -> RgbImage {
    let mut img = RgbImage::new(size.0, size.1);

    for y in 0..size.1 {
        for x in 0..size.0 {
            let offset: usize = (y * size.0 + x) as _;
            unsafe {
                img.unsafe_put_pixel(
                    x,
                    y,
                    Rgb([mapping[offset], mapping[offset ], mapping[offset ]]),
                )
            };
        }
    }

    img
}

pub fn decode_tiled_small_image(
    mapping: &[u32],
    tilesize: u32,
    tiles: (u32, u32),
    size: (u32, u32),
) -> RgbImage {
    let mut img = RgbImage::new(tiles.0 * tilesize / 4, tiles.1 * tilesize / 4);

    let mut i = 0;

    let mut avg_16 = |x, y| {
        let mut avg = PixelAverage::new();
        for n in 0..16 {
            avg.add(mapping[i + n]);
        }
        unsafe {
            img.unsafe_put_pixel(x, y, avg.rgb());
        }
        i = i + 16;
    };

    let mut copy_16x4_px = |x, y| {
        avg_16(x, y);
        avg_16(x + 1, y);
        avg_16(x + 2, y);
        avg_16(x + 3, y);
    };

    let mut copy_16x16_px = |x, y| {
        copy_16x4_px(x, y);
        copy_16x4_px(x, y + 1);
        copy_16x4_px(x, y + 2);
        copy_16x4_px(x, y + 3);
    };

    for ytile in 0..tiles.1 {
        if ytile % 2 == 0 {
            let mut copy_tile = |x, y| {
                copy_16x16_px(x, y);
                copy_16x16_px(x, y + 4);
                copy_16x16_px(x + 4, y + 4);
                copy_16x16_px(x + 4, y);
            };

            for xtile in 0..tiles.0 {
                copy_tile(xtile * tilesize / 4, ytile * tilesize / 4);
            }
        } else {
            let mut copy_tile = |x, y| {
                copy_16x16_px(x + 4, y + 4);
                copy_16x16_px(x + 4, y);
                copy_16x16_px(x, y);
                copy_16x16_px(x, y + 4);
            };

            for xtile in (0..tiles.0).rev() {
                copy_tile(xtile * tilesize / 4, ytile * tilesize / 4);
            }
        }
    }

    img.sub_image(0, 0, size.0 / 4, size.1 / 4).to_image()
}

pub fn to_image(mapping: &[u8], tilesize: u32, tiles: (u32, u32), size: (u32, u32)) -> RgbImage {
    let mut img = RgbImage::new(tiles.0 * tilesize, tiles.1 * tilesize);
    let mut i = 0;

    let mut copy_px = |x, y| {
        let color = Rgb([
            mapping[(i + 2) as usize],
            mapping[(i + 1) as usize],
            mapping[(i + 0) as usize],
        ]);
        unsafe {
            img.unsafe_put_pixel(x, y, color);
        }
        i = i + 4;
    };
    let mut copy_4_px = |x, y| {
        copy_px(x, y);
        copy_px(x + 1, y);
        copy_px(x + 2, y);
        copy_px(x + 3, y);
    };

    let mut copy_4x4_px = |x, y| {
        copy_4_px(x, y);
        copy_4_px(x, y + 1);
        copy_4_px(x, y + 2);
        copy_4_px(x, y + 3);
    };

    let mut copy_16x4_px = |x, y| {
        copy_4x4_px(x, y);
        copy_4x4_px(x + 4, y);
        copy_4x4_px(x + 8, y);
        copy_4x4_px(x + 12, y);
    };

    let mut copy_16x16_px = |x, y| {
        copy_16x4_px(x, y);
        copy_16x4_px(x, y + 4);
        copy_16x4_px(x, y + 8);
        copy_16x4_px(x, y + 12);
    };

    for ytile in 0..tiles.1 {
        if ytile % 2 == 0 {
            let mut copy_tile = |x, y| {
                copy_16x16_px(x, y);
                copy_16x16_px(x, y + 16);
                copy_16x16_px(x + 16, y + 16);
                copy_16x16_px(x + 16, y);
            };

            for xtile in 0..tiles.0 {
                copy_tile(xtile * tilesize, ytile * tilesize);
            }
        } else {
            let mut copy_tile = |x, y| {
                copy_16x16_px(x + 16, y + 16);
                copy_16x16_px(x + 16, y);
                copy_16x16_px(x, y);
                copy_16x16_px(x, y + 16);
            };

            for xtile in (0..tiles.0).rev() {
                copy_tile(xtile * tilesize, ytile * tilesize);
            }
        }
    }

    img.sub_image(0, 0, size.0, size.1).to_image()
}
