//! Stretching part of a render to the output size.

use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::RgbaImage;

use crate::bbox::PixelRect;
use crate::error::{MapliError, Result};

/// Resample `crop`, a fractional rectangle of `src`, to a `width × height` image.
///
/// The crop edges are passed to the filter unrounded, so they land on sub-pixel positions and
/// adjacent bboxes line up exactly. Output pixels at the edges are interpolated with the source
/// pixels just outside the crop, as neighbouring bboxes would be, so they blend across seams. Filtering is bilinear, as a WMS stretch usually is, and
/// done on premultiplied alpha, so transparent maps do not get dark fringes around features.
pub(crate) fn stretch_crop(
    src: &RgbaImage,
    crop: PixelRect,
    width: u32,
    height: u32,
) -> Result<RgbaImage> {
    let fail = |e: &dyn std::fmt::Display| MapliError::RenderFailed(format!("resampling: {e}"));

    // Rounding errors must not push the crop outside the source, which the resizer rejects
    let (src_w, src_h) = (f64::from(src.width()), f64::from(src.height()));
    let left = crop.left.clamp(0.0, src_w);
    let top = crop.top.clamp(0.0, src_h);
    let crop_w = crop.width.min(src_w - left);
    let crop_h = crop.height.min(src_h - top);

    let src_view = ImageRef::new(src.width(), src.height(), src.as_raw(), PixelType::U8x4)
        .map_err(|e| fail(&e))?;
    let mut dst = Image::new(width, height, PixelType::U8x4);
    let options = ResizeOptions::new()
        .resize_alg(ResizeAlg::Convolution(FilterType::Bilinear))
        .crop(left, top, crop_w, crop_h);
    Resizer::new()
        .resize(&src_view, &mut dst, &options)
        .map_err(|e| fail(&e))?;

    RgbaImage::from_raw(width, height, dst.into_vec())
        .ok_or_else(|| MapliError::RenderFailed("resampling: wrong buffer size".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn rect(left: f64, top: f64, width: f64, height: f64) -> PixelRect {
        PixelRect {
            left,
            top,
            width,
            height,
        }
    }

    #[test]
    fn stretches_crop_to_output_size() {
        // left half red, right half blue
        let src = RgbaImage::from_fn(8, 8, |x, _| {
            if x < 4 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 0, 255, 255])
            }
        });
        // the filter also reads the pixels just outside the crop, so keep clear of the blue
        let out = stretch_crop(&src, rect(1.0, 0.0, 2.0, 8.0), 16, 4).unwrap();
        assert_eq!(out.dimensions(), (16, 4));
        assert!(out.pixels().all(|p| p.0 == [255, 0, 0, 255]));
    }

    #[test]
    fn crop_slightly_outside_source_is_clamped() {
        let src = RgbaImage::from_pixel(4, 4, Rgba([0, 255, 0, 255]));
        let out = stretch_crop(&src, rect(-1e-9, -1e-9, 4.0 + 1e-9, 4.0 + 1e-9), 2, 2).unwrap();
        assert!(out.pixels().all(|p| p.0 == [0, 255, 0, 255]));
    }

    #[test]
    fn transparent_edges_get_no_dark_fringe() {
        // opaque red on the left, fully transparent (black) on the right
        let src = RgbaImage::from_fn(4, 1, |x, _| {
            if x < 2 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });
        let out = stretch_crop(&src, rect(0.0, 0.0, 4.0, 1.0), 16, 1).unwrap();
        for p in out.pixels().filter(|p| p.0[3] > 0) {
            assert_eq!(p.0[..3], [255, 0, 0], "{p:?} has a dark fringe");
        }
    }
}
