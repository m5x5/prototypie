//! CGImage → straight-alpha RGBA at an exact pixel size. CoreGraphics is thread safe for this, so
//! it runs on the worker thread, not the UI thread.

use std::ffi::c_void;

use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, CGInterpolationQuality};

/// Draws `image` scaled to `w` x `h` and returns straight-alpha sRGB RGBA, rows top first.
pub fn to_rgba(image: &CGImage, w: u32, h: u32) -> Option<Vec<u8>> {
    if w == 0 || h == 0 {
        return None;
    }
    let (w, h) = (w as usize, h as usize);
    let mut buf = vec![0u8; w * h * 4];
    let space: CFRetained<CGColorSpace> = CGColorSpace::new_device_rgb()?;
    // SAFETY: `buf` outlives the context (dropped at the end of this function) and holds
    // h rows of w*4 bytes; RGBA8 premultiplied-last is a supported bitmap format.
    let ctx = unsafe {
        CGBitmapContextCreate(buf.as_mut_ptr().cast::<c_void>(), w, h, 8, w * 4, Some(&space), CGImageAlphaInfo::PremultipliedLast.0)
    }?;
    CGContext::set_interpolation_quality(Some(&ctx), CGInterpolationQuality::High);
    let rect = CGRect { origin: CGPoint { x: 0.0, y: 0.0 }, size: CGSize { width: w as f64, height: h as f64 } };
    CGContext::draw_image(Some(&ctx), rect, Some(image));
    drop(ctx);
    unpremultiply(&mut buf);
    Some(buf)
}

/// Pixel size of a CGImage.
pub fn size(image: &CGImage) -> (u32, u32) {
    (CGImage::width(Some(image)) as u32, CGImage::height(Some(image)) as u32)
}

fn unpremultiply(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = px[3] as u32;
        if a == 0 || a == 255 {
            continue;
        }
        for c in &mut px[..3] {
            *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::unpremultiply;

    #[test]
    fn unpremultiplies_partial_alpha_only() {
        let mut px = [128, 64, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0];
        unpremultiply(&mut px);
        assert_eq!(px, [255, 128, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0]);
    }
}
