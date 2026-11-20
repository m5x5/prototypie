//! Area (box) downscaling of straight-alpha RGBA, for shrinking a cached snapshot without
//! re-rendering it through WebKit. Alpha-weighted, so transparent pixels don't darken edges.
//! Pure; runs on the snapshot worker thread.

/// Each output index's source indices and coverage weights (summing to 1).
fn weights(n_in: usize, n_out: usize) -> Vec<Vec<(usize, f32)>> {
    let scale = n_in as f64 / n_out as f64;
    (0..n_out)
        .map(|o| {
            let (start, end) = (o as f64 * scale, (o + 1) as f64 * scale);
            let mut w = Vec::with_capacity(scale.ceil() as usize + 1);
            let mut i = start.floor() as usize;
            while (i as f64) < end && i < n_in {
                let cover = end.min(i as f64 + 1.0) - start.max(i as f64);
                if cover > 0.0 {
                    w.push((i, (cover / scale) as f32));
                }
                i += 1;
            }
            w
        })
        .collect()
}

/// Output height for width `nw` with the aspect kept (at least 1).
pub fn scaled_height(w: u32, h: u32, nw: u32) -> u32 {
    ((h as f64 * nw as f64 / w.max(1) as f64).round() as u32).max(1)
}

/// Downscales `rgba` (`w` × `h`, rows top first) to `nw` wide, aspect kept. Returns the pixels
/// and the new height. `nw >= w` returns a copy.
pub fn downscale(rgba: &[u8], w: u32, h: u32, nw: u32) -> (Vec<u8>, u32) {
    let (w, h) = (w as usize, h as usize);
    assert_eq!(rgba.len(), w * h * 4, "rgba is w × h × 4");
    if nw as usize >= w || w == 0 || h == 0 {
        return (rgba.to_vec(), h as u32);
    }
    let nw = nw.max(1) as usize;
    let nh = scaled_height(w as u32, h as u32, nw as u32) as usize;
    let (wx, wy) = (weights(w, nw), weights(h, nh));
    // Horizontal pass into premultiplied f32 rows.
    let mut mid = vec![0f32; nw * h * 4];
    for y in 0..h {
        let row = &rgba[y * w * 4..(y + 1) * w * 4];
        for (ox, ws) in wx.iter().enumerate() {
            let mut acc = [0f32; 4];
            for &(ix, k) in ws {
                let p = &row[ix * 4..ix * 4 + 4];
                let a = p[3] as f32 / 255.0;
                acc[0] += p[0] as f32 * a * k;
                acc[1] += p[1] as f32 * a * k;
                acc[2] += p[2] as f32 * a * k;
                acc[3] += p[3] as f32 * k;
            }
            mid[(y * nw + ox) * 4..(y * nw + ox) * 4 + 4].copy_from_slice(&acc);
        }
    }
    // Vertical pass, then back to straight alpha.
    let mut out = vec![0u8; nw * nh * 4];
    for (oy, ws) in wy.iter().enumerate() {
        for x in 0..nw {
            let mut acc = [0f32; 4];
            for &(iy, k) in ws {
                let p = &mid[(iy * nw + x) * 4..(iy * nw + x) * 4 + 4];
                for c in 0..4 {
                    acc[c] += p[c] * k;
                }
            }
            let a = acc[3];
            let o = &mut out[(oy * nw + x) * 4..(oy * nw + x) * 4 + 4];
            if a > 0.0 {
                let unpre = 255.0 / a;
                for c in 0..3 {
                    o[c] = (acc[c] * unpre).round().clamp(0.0, 255.0) as u8;
                }
            }
            o[3] = a.round().clamp(0.0, 255.0) as u8;
        }
    }
    (out, nh as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halving_averages_2x2_blocks() {
        // 4×2: left 2×2 block red/blue mix, right block white.
        let px = |r: u8, g: u8, b: u8| [r, g, b, 255];
        let rows = [
            [px(255, 0, 0), px(0, 0, 255), px(255, 255, 255), px(255, 255, 255)],
            [px(255, 0, 0), px(0, 0, 255), px(255, 255, 255), px(255, 255, 255)],
        ];
        let rgba: Vec<u8> = rows.iter().flatten().flatten().copied().collect();
        let (out, nh) = downscale(&rgba, 4, 2, 2);
        assert_eq!(nh, 1);
        assert_eq!(out, [128, 0, 128, 255, 255, 255, 255, 255]);
    }

    #[test]
    fn transparent_pixels_do_not_darken_colour() {
        // Red next to fully transparent black: colour stays red, alpha halves.
        let rgba = [255, 0, 0, 255, 0, 0, 0, 0];
        let (out, _) = downscale(&rgba, 2, 1, 1);
        assert_eq!(out, [255, 0, 0, 128]);
    }

    #[test]
    fn fractional_ratios_keep_aspect_and_coverage() {
        // A uniform 768×576 grey downscales to 512×384 and stays that grey.
        let rgba = [90u8, 90, 90, 255].repeat(768 * 576);
        let (out, nh) = downscale(&rgba, 768, 576, 512);
        assert_eq!(nh, 384);
        assert_eq!(out.len(), 512 * 384 * 4);
        assert!(out.chunks(4).all(|p| p == [90, 90, 90, 255]));
        let ws = weights(3, 2);
        for w in &ws {
            assert!((w.iter().map(|(_, k)| k).sum::<f32>() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn not_smaller_is_a_copy() {
        let rgba = [1u8, 2, 3, 4].repeat(6);
        assert_eq!(downscale(&rgba, 3, 2, 3), (rgba.clone(), 2));
    }
}
