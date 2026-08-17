use std::collections::VecDeque;

use super::PreviewRegion;

#[derive(Clone)]
pub(crate) struct TiledImage {
    width: u32,
    height: u32,
    strips: VecDeque<Strip>,
}

#[derive(Clone)]
struct Strip {
    height: u32,
    pixels: Vec<u8>,
}

impl TiledImage {
    pub(crate) fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        Self {
            width,
            height,
            strips: VecDeque::from([Strip { height, pixels }]),
        }
    }

    pub(crate) const fn width(&self) -> u32 {
        self.width
    }

    pub(crate) const fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn append_overlapping(
        &mut self,
        mut strip: Vec<u8>,
        strip_height: u32,
        grow: u32,
    ) -> PreviewRegion {
        let grow = grow.min(strip_height);
        let old_height = self.height;
        let new_height = old_height + grow;
        let overlap = strip_height
            .saturating_sub(grow)
            .min(self.height.saturating_sub(1));
        self.trim_bottom(overlap);
        let drawn = new_height - self.height;
        if drawn < strip_height {
            let drop_rows = strip_height - drawn;
            strip.drain(..self.row_bytes() * drop_rows as usize);
        }
        self.height += drawn;
        self.strips.push_back(Strip {
            height: drawn,
            pixels: strip,
        });
        PreviewRegion {
            top: new_height - drawn,
            height: drawn,
        }
    }

    pub(crate) fn prepend_overlapping(
        &mut self,
        mut strip: Vec<u8>,
        strip_height: u32,
        grow: u32,
    ) -> PreviewRegion {
        let grow = grow.min(strip_height);
        let new_height = self.height + grow;
        let overlap = strip_height
            .saturating_sub(grow)
            .min(self.height.saturating_sub(1));
        self.trim_top(overlap);
        let drawn = new_height - self.height;
        if drawn < strip_height {
            strip.truncate(self.row_bytes() * drawn as usize);
        }
        self.height += drawn;
        self.strips.push_front(Strip {
            height: drawn,
            pixels: strip,
        });
        PreviewRegion {
            top: 0,
            height: new_height,
        }
    }

    pub(crate) fn crop_rows(&self, start: u32, height: u32) -> Vec<u8> {
        let row_bytes = self.row_bytes();
        let mut output = vec![0; row_bytes * height as usize];
        let end = start.saturating_add(height);
        let mut strip_top = 0;
        for strip in &self.strips {
            let strip_bottom = strip_top + strip.height;
            let copy_top = start.max(strip_top);
            let copy_bottom = end.min(strip_bottom);
            if copy_top < copy_bottom {
                let source = (copy_top - strip_top) as usize * row_bytes;
                let destination = (copy_top - start) as usize * row_bytes;
                let bytes = (copy_bottom - copy_top) as usize * row_bytes;
                output[destination..destination + bytes]
                    .copy_from_slice(&strip.pixels[source..source + bytes]);
            }
            strip_top = strip_bottom;
            if strip_top >= end {
                break;
            }
        }
        output
    }

    pub(crate) fn into_pixels(self) -> Vec<u8> {
        let mut pixels = Vec::with_capacity(self.row_bytes() * self.height as usize);
        for strip in self.strips {
            pixels.extend_from_slice(&strip.pixels);
        }
        pixels
    }

    fn trim_top(&mut self, rows: u32) {
        let mut rows = rows.min(self.height.saturating_sub(1));
        self.height -= rows;
        let row_bytes = self.row_bytes();
        while rows > 0 {
            let mut strip = self.strips.pop_front().expect("tiled image has strips");
            if rows < strip.height {
                strip.pixels.drain(..row_bytes * rows as usize);
                strip.height -= rows;
                self.strips.push_front(strip);
                break;
            }
            rows -= strip.height;
        }
    }

    fn trim_bottom(&mut self, rows: u32) {
        let mut rows = rows.min(self.height.saturating_sub(1));
        self.height -= rows;
        let row_bytes = self.row_bytes();
        while rows > 0 {
            let mut strip = self.strips.pop_back().expect("tiled image has strips");
            if rows < strip.height {
                strip.height -= rows;
                strip.pixels.truncate(row_bytes * strip.height as usize);
                self.strips.push_back(strip);
                break;
            }
            rows -= strip.height;
        }
    }

    const fn row_bytes(&self) -> usize {
        self.width as usize * 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(values: &[u8]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| [*value, *value, *value, 255])
            .collect()
    }

    #[test]
    fn overlapping_append_and_prepend_keep_row_order() {
        let mut image = TiledImage::new(1, 4, rows(&[3, 4, 5, 6]));
        image.append_overlapping(rows(&[5, 6, 7, 8]), 4, 2);
        image.prepend_overlapping(rows(&[1, 2, 3, 4]), 4, 2);
        let values = image
            .into_pixels()
            .chunks_exact(4)
            .map(|pixel| pixel[0])
            .collect::<Vec<_>>();
        assert_eq!(values, [1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
