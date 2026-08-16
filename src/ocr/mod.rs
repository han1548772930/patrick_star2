//! OCR task execution and platform-neutral result models.

use crate::model::{Rect, RgbaFrame};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrLanguage {
    pub tag: String,
    pub display_name: String,
    pub native_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrWord {
    pub text: String,
    pub bounds: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrLine {
    pub text: String,
    pub bounds: Option<Rect>,
    pub words: Vec<OcrWord>,
}

impl OcrLine {
    pub fn from_words(text: String, words: Vec<OcrWord>) -> Self {
        let bounds = words.iter().map(|word| word.bounds).reduce(Rect::union);
        Self {
            text,
            bounds,
            words,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrDocument {
    pub language: OcrLanguage,
    pub text_angle_degrees: Option<f64>,
    pub lines: Vec<OcrLine>,
}

impl OcrDocument {
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub trait TextRecognizer {
    #[allow(dead_code)]
    fn available_languages(&self) -> anyhow::Result<Vec<OcrLanguage>>;

    fn recognize_text(
        &self,
        image: &RgbaFrame,
        language_tag: Option<&str>,
    ) -> anyhow::Result<OcrDocument>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_text_preserves_line_boundaries_without_trailing_newline() {
        let document = OcrDocument {
            language: OcrLanguage {
                tag: "zh-Hans".to_owned(),
                display_name: "Chinese (Simplified)".to_owned(),
                native_name: "Chinese".to_owned(),
            },
            text_angle_degrees: None,
            lines: vec![
                OcrLine {
                    text: "first".to_owned(),
                    bounds: None,
                    words: Vec::new(),
                },
                OcrLine {
                    text: "second".to_owned(),
                    bounds: None,
                    words: Vec::new(),
                },
            ],
        };

        assert_eq!(document.text(), "first\nsecond");
    }

    #[test]
    fn empty_document_produces_empty_text() {
        let document = OcrDocument {
            language: OcrLanguage {
                tag: "en-US".to_owned(),
                display_name: "English".to_owned(),
                native_name: "English".to_owned(),
            },
            text_angle_degrees: None,
            lines: Vec::new(),
        };
        assert!(document.text().is_empty());
    }

    #[test]
    fn line_bounds_are_the_union_of_word_bounds() {
        let line = OcrLine::from_words(
            "two words".to_owned(),
            vec![
                OcrWord {
                    text: "two".to_owned(),
                    bounds: Rect::new(8.0, 3.0, 21.0, 11.0),
                },
                OcrWord {
                    text: "words".to_owned(),
                    bounds: Rect::new(24.0, 2.0, 52.0, 13.0),
                },
            ],
        );

        assert_eq!(line.bounds, Some(Rect::new(8.0, 2.0, 52.0, 13.0)));
    }
}
