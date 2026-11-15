/// A texture on the GPU, ready to draw with. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Texture {
    pub(crate) bind_group: wgpu::BindGroup,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl Texture {
    /// Size in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Builds the mip chain for an RGBA8 image: each level halves the previous one
/// with a 2x2 box filter, down to 1x1. Level 0 is the input itself.
pub(crate) fn mip_chain(width: u32, height: u32, rgba: &[u8]) -> Vec<(u32, u32, Vec<u8>)> {
    let mut levels = vec![(width, height, rgba.to_vec())];
    loop {
        let (w, h, src) = levels.last().unwrap();
        let (w, h) = (*w, *h);
        if w == 1 && h == 1 {
            break;
        }
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut dst = Vec::with_capacity((nw * nh * 4) as usize);
        for y in 0..nh {
            for x in 0..nw {
                // Clamp so odd sizes and 1-pixel-wide levels stay in bounds.
                let (x0, y0) = ((x * 2).min(w - 1), (y * 2).min(h - 1));
                let (x1, y1) = ((x * 2 + 1).min(w - 1), (y * 2 + 1).min(h - 1));
                for c in 0..4 {
                    let at = |px: u32, py: u32| src[((py * w + px) * 4 + c) as usize] as u32;
                    let sum = at(x0, y0) + at(x1, y0) + at(x0, y1) + at(x1, y1);
                    dst.push(((sum + 2) / 4) as u8);
                }
            }
        }
        levels.push((nw, nh, dst));
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mip_chain_halves_down_to_one_pixel() {
        let levels = mip_chain(8, 2, &[255; 8 * 2 * 4]);
        let sizes: Vec<_> = levels.iter().map(|(w, h, _)| (*w, *h)).collect();
        assert_eq!(sizes, [(8, 2), (4, 1), (2, 1), (1, 1)]);
        for (w, h, data) in &levels {
            assert_eq!(data.len() as u32, w * h * 4);
            assert!(data.iter().all(|&b| b == 255));
        }
    }

    #[test]
    fn mip_averages_neighbours() {
        // 2x2: two black and two white pixels average to mid grey.
        let mut px = vec![0u8; 16];
        px[0..4].fill(255);
        px[12..16].fill(255);
        let levels = mip_chain(2, 2, &px);
        assert_eq!(levels[1].2, [128, 128, 128, 128]);
    }
}
