use anyhow::{Context, Result};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::{OcrEngine, OcrResult};
use windows::Security::Cryptography::CryptographicBuffer;
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
use windows::core::HSTRING;

use crate::model::{Rect, RgbaFrame};
use crate::ocr::{OcrDocument, OcrLanguage, OcrLine, OcrWord};

#[allow(dead_code)]
pub fn available_languages() -> Result<Vec<OcrLanguage>> {
    let _apartment = Apartment::initialize()?;
    let languages = OcrEngine::AvailableRecognizerLanguages()
        .context("failed to enumerate Windows OCR languages")?;
    let mut result = Vec::with_capacity(languages.Size()? as usize);
    for index in 0..languages.Size()? {
        result.push(map_language(&languages.GetAt(index)?)?);
    }
    Ok(result)
}

pub fn recognize(image: &RgbaFrame, language_tag: Option<&str>) -> Result<OcrDocument> {
    let _apartment = Apartment::initialize()?;
    validate_dimensions(image)?;

    let engine = match language_tag {
        Some(tag) => engine_for_language(tag)?,
        None => OcrEngine::TryCreateFromUserProfileLanguages()
            .context("no installed Windows OCR language matches the user profile")?,
    };
    let language = map_language(&engine.RecognizerLanguage()?)?;
    let bitmap = software_bitmap(image)?;
    let result = engine
        .RecognizeAsync(&bitmap)
        .context("failed to start Windows OCR")?
        .join()
        .context("Windows OCR failed")?;
    map_result(&result, language)
}

fn validate_dimensions(image: &RgbaFrame) -> Result<()> {
    let maximum = OcrEngine::MaxImageDimension().context("failed to query OCR image limit")?;
    anyhow::ensure!(
        image.width() <= maximum && image.height() <= maximum,
        "OCR image {}x{} exceeds Windows OCR limit {maximum}",
        image.width(),
        image.height()
    );
    Ok(())
}

fn engine_for_language(tag: &str) -> Result<OcrEngine> {
    let language = Language::CreateLanguage(&HSTRING::from(tag))
        .with_context(|| format!("invalid OCR language tag {tag:?}"))?;
    anyhow::ensure!(
        OcrEngine::IsLanguageSupported(&language)?,
        "Windows OCR language {tag:?} is not installed"
    );
    OcrEngine::TryCreateFromLanguage(&language)
        .with_context(|| format!("failed to create Windows OCR engine for {tag:?}"))
}

fn software_bitmap(image: &RgbaFrame) -> Result<SoftwareBitmap> {
    let width = i32::try_from(image.width()).context("OCR image width exceeds i32")?;
    let height = i32::try_from(image.height()).context("OCR image height exceeds i32")?;
    let buffer = CryptographicBuffer::CreateFromByteArray(image.pixels())
        .context("failed to create OCR pixel buffer")?;
    let rgba = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Rgba8,
        width,
        height,
        BitmapAlphaMode::Straight,
    )
    .context("failed to create OCR bitmap")?;
    SoftwareBitmap::Convert(&rgba, BitmapPixelFormat::Gray8)
        .context("failed to convert OCR bitmap to Gray8")
}

fn map_result(result: &OcrResult, language: OcrLanguage) -> Result<OcrDocument> {
    let native_lines = result.Lines().context("failed to read OCR lines")?;
    let mut lines = Vec::with_capacity(native_lines.Size()? as usize);
    for line_index in 0..native_lines.Size()? {
        let native_line = native_lines.GetAt(line_index)?;
        let native_words = native_line.Words()?;
        let mut words = Vec::with_capacity(native_words.Size()? as usize);
        for word_index in 0..native_words.Size()? {
            let native_word = native_words.GetAt(word_index)?;
            let bounds = native_word.BoundingRect()?;
            words.push(OcrWord {
                text: native_word.Text()?.to_string_lossy(),
                bounds: Rect::new(
                    bounds.X,
                    bounds.Y,
                    bounds.X + bounds.Width,
                    bounds.Y + bounds.Height,
                ),
            });
        }
        lines.push(OcrLine::from_words(
            native_line.Text()?.to_string_lossy(),
            words,
        ));
    }

    let text_angle_degrees = result.TextAngle().ok().and_then(|angle| angle.Value().ok());
    Ok(OcrDocument {
        language,
        text_angle_degrees,
        lines,
    })
}

fn map_language(language: &Language) -> Result<OcrLanguage> {
    Ok(OcrLanguage {
        tag: language.LanguageTag()?.to_string_lossy(),
        display_name: language.DisplayName()?.to_string_lossy(),
        native_name: language.NativeName()?.to_string_lossy(),
    })
}

struct Apartment {
    uninitialize: bool,
}

impl Apartment {
    fn initialize() -> Result<Self> {
        match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
            Ok(()) => Ok(Self { uninitialize: true }),
            Err(error) if error.code() == RPC_E_CHANGED_MODE => Ok(Self {
                uninitialize: false,
            }),
            Err(error) => Err(error).context("failed to initialize Windows Runtime for OCR"),
        }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.uninitialize {
            unsafe { RoUninitialize() };
        }
    }
}
