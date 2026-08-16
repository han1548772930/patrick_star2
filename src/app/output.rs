use std::fs::File;
use std::io::{BufWriter, Write};

use anyhow::{Context, Result};
use jpeg_encoder::{ColorType as JpegColorType, Encoder as JpegEncoder};
use png::{BitDepth, ColorType as PngColorType, Encoder as PngEncoder};

use crate::model::RgbaFrame;
use crate::platform::{ImageFileFormat, ImageSaveTarget};

const JPEG_QUALITY: u8 = 92;

pub fn save_image(image: &RgbaFrame, target: &ImageSaveTarget) -> Result<()> {
    let file = File::create(&target.path)
        .with_context(|| format!("create image file {}", target.path.display()))?;
    let mut writer = BufWriter::new(file);
    match target.format {
        ImageFileFormat::Png => encode_png(image, &mut writer)?,
        ImageFileFormat::Jpeg => encode_jpeg(image, &mut writer)?,
    }
    writer
        .flush()
        .with_context(|| format!("flush image file {}", target.path.display()))?;
    Ok(())
}

fn encode_png(image: &RgbaFrame, output: impl Write) -> Result<()> {
    let mut encoder = PngEncoder::new(output, image.width(), image.height());
    encoder.set_color(PngColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    let mut writer = encoder.write_header().context("write PNG header")?;
    writer
        .write_image_data(image.pixels())
        .context("encode PNG pixels")?;
    writer.finish().context("finish PNG image")?;
    Ok(())
}

fn encode_jpeg(image: &RgbaFrame, output: impl Write) -> Result<()> {
    let width = u16::try_from(image.width()).context("JPEG width exceeds 65535 pixels")?;
    let height = u16::try_from(image.height()).context("JPEG height exceeds 65535 pixels")?;
    JpegEncoder::new(output, JPEG_QUALITY)
        .encode(image.pixels(), width, height, JpegColorType::Rgba)
        .context("encode JPEG pixels")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RectI;

    fn image() -> RgbaFrame {
        RgbaFrame::new(
            RectI::new(-10, 20, 2, 2),
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
        )
        .unwrap()
    }

    #[test]
    fn png_encoder_writes_a_complete_png_stream() {
        let mut encoded = Vec::new();
        encode_png(&image(), &mut encoded).unwrap();
        assert!(encoded.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(encoded.windows(4).any(|chunk| chunk == b"IEND"));
    }

    #[test]
    fn jpeg_encoder_writes_a_complete_jfif_stream() {
        let mut encoded = Vec::new();
        encode_jpeg(&image(), &mut encoded).unwrap();
        assert!(encoded.starts_with(&[0xff, 0xd8]));
        assert!(encoded.ends_with(&[0xff, 0xd9]));
    }
}
