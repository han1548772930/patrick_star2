use crate::model::RgbaFrame;

use super::{FrameFingerprint, PreviewRegion, StitchDocument};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Alignment {
    /// Previous keypoint x minus current keypoint x.
    pub horizontal_shift: f32,
    /// Previous keypoint y minus current keypoint y. Positive means scrolling down.
    pub vertical_shift: f32,
    pub good_matches: u32,
    pub inliers: u32,
}

impl Alignment {
    pub fn inlier_ratio(self) -> f32 {
        if self.good_matches == 0 {
            0.0
        } else {
            self.inliers as f32 / self.good_matches as f32
        }
    }
}

pub trait FrameMatcher {
    fn align(&mut self, previous: &RgbaFrame, current: &RgbaFrame) -> anyhow::Result<Alignment>;

    /// Commits any cached features for the current frame after it was stitched.
    fn accept_alignment(&mut self) {}
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollConfig {
    pub duplicate_luma_difference: f32,
    pub minimum_vertical_shift: f32,
    pub maximum_vertical_shift_ratio: f32,
    pub maximum_horizontal_shift: f32,
    pub minimum_good_matches: u32,
    pub minimum_inlier_ratio: f32,
}

impl Default for ScrollConfig {
    fn default() -> Self {
        Self {
            duplicate_luma_difference: 0.75,
            minimum_vertical_shift: 4.0,
            maximum_vertical_shift_ratio: 0.9,
            maximum_horizontal_shift: 8.0,
            minimum_good_matches: 12,
            minimum_inlier_ratio: 0.55,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    DimensionsChanged,
    NotEnoughMatches,
    LowInlierRatio,
    HorizontalDrift,
    UnsupportedDirection,
    ShiftOutsideViewport,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PushOutcome {
    Duplicate,
    Rejected(RejectReason),
    Appended {
        alignment: Alignment,
        preview: PreviewRegion,
    },
}

pub struct ScrollSession<M> {
    config: ScrollConfig,
    matcher: M,
    previous_fingerprint: FrameFingerprint,
    previous: RgbaFrame,
    document: StitchDocument,
}

impl<M: FrameMatcher> ScrollSession<M> {
    pub fn new(initial: RgbaFrame, matcher: M, config: ScrollConfig) -> Self {
        let previous_fingerprint = FrameFingerprint::from_frame(&initial);
        let document = StitchDocument::from_frame(&initial);
        Self {
            config,
            matcher,
            previous_fingerprint,
            previous: initial,
            document,
        }
    }

    pub fn document(&self) -> &StitchDocument {
        &self.document
    }

    pub fn push(&mut self, frame: RgbaFrame) -> anyhow::Result<PushOutcome> {
        if frame.width() != self.previous.width() || frame.height() != self.previous.height() {
            return Ok(PushOutcome::Rejected(RejectReason::DimensionsChanged));
        }

        let fingerprint = FrameFingerprint::from_frame(&frame);
        if self
            .previous_fingerprint
            .difference(&fingerprint)
            .is_some_and(|difference| difference <= self.config.duplicate_luma_difference)
        {
            return Ok(PushOutcome::Duplicate);
        }

        let alignment = self.matcher.align(&self.previous, &frame)?;
        if alignment.good_matches < self.config.minimum_good_matches {
            return Ok(PushOutcome::Rejected(RejectReason::NotEnoughMatches));
        }
        if alignment.inlier_ratio() < self.config.minimum_inlier_ratio {
            return Ok(PushOutcome::Rejected(RejectReason::LowInlierRatio));
        }
        if alignment.horizontal_shift.abs() > self.config.maximum_horizontal_shift {
            return Ok(PushOutcome::Rejected(RejectReason::HorizontalDrift));
        }
        if alignment.vertical_shift < self.config.minimum_vertical_shift {
            return Ok(PushOutcome::Rejected(RejectReason::UnsupportedDirection));
        }
        let maximum_shift = frame.height() as f32 * self.config.maximum_vertical_shift_ratio;
        if alignment.vertical_shift > maximum_shift {
            return Ok(PushOutcome::Rejected(RejectReason::ShiftOutsideViewport));
        }

        let rows = alignment
            .vertical_shift
            .round()
            .clamp(1.0, frame.height() as f32) as u32;
        let preview = self.document.append_bottom(&frame, rows)?;
        self.matcher.accept_alignment();
        self.previous = frame;
        self.previous_fingerprint = fingerprint;
        Ok(PushOutcome::Appended { alignment, preview })
    }

    pub fn finish(self) -> RgbaFrame {
        self.document.into_frame()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::model::RectI;

    struct Matcher {
        results: VecDeque<Alignment>,
        calls: usize,
    }

    impl FrameMatcher for Matcher {
        fn align(
            &mut self,
            _previous: &RgbaFrame,
            _current: &RgbaFrame,
        ) -> anyhow::Result<Alignment> {
            self.calls += 1;
            Ok(self.results.pop_front().unwrap())
        }
    }

    fn frame(rows: &[u8]) -> RgbaFrame {
        let pixels = rows
            .iter()
            .flat_map(|value| [*value, *value, *value, 255])
            .collect();
        RgbaFrame::new(RectI::new(20, -10, 1, rows.len() as u32), pixels).unwrap()
    }

    fn alignment(vertical_shift: f32) -> Alignment {
        Alignment {
            horizontal_shift: 0.25,
            vertical_shift,
            good_matches: 40,
            inliers: 35,
        }
    }

    #[test]
    fn duplicates_skip_orb_matching() {
        let matcher = Matcher {
            results: VecDeque::new(),
            calls: 0,
        };
        let mut session =
            ScrollSession::new(frame(&[10, 20, 30, 40]), matcher, ScrollConfig::default());
        assert_eq!(
            session.push(frame(&[10, 20, 30, 40])).unwrap(),
            PushOutcome::Duplicate
        );
        assert_eq!(session.matcher.calls, 0);
    }

    #[test]
    fn accepted_alignment_appends_new_rows() {
        let matcher = Matcher {
            results: VecDeque::from([alignment(2.0)]),
            calls: 0,
        };
        let config = ScrollConfig {
            minimum_vertical_shift: 1.0,
            ..ScrollConfig::default()
        };
        let mut session = ScrollSession::new(frame(&[10, 20, 30, 40]), matcher, config);
        let outcome = session.push(frame(&[30, 40, 50, 60])).unwrap();

        assert!(matches!(
            outcome,
            PushOutcome::Appended {
                preview: PreviewRegion { top: 4, height: 2 },
                ..
            }
        ));
        assert_eq!(session.finish().height(), 6);
    }

    #[test]
    fn weak_or_drifting_alignment_is_rejected_without_changing_document() {
        let matcher = Matcher {
            results: VecDeque::from([Alignment {
                horizontal_shift: 15.0,
                ..alignment(2.0)
            }]),
            calls: 0,
        };
        let config = ScrollConfig {
            minimum_vertical_shift: 1.0,
            ..ScrollConfig::default()
        };
        let mut session = ScrollSession::new(frame(&[10, 20, 30, 40]), matcher, config);

        assert_eq!(
            session.push(frame(&[30, 40, 50, 60])).unwrap(),
            PushOutcome::Rejected(RejectReason::HorizontalDrift)
        );
        assert_eq!(session.document().height(), 4);
    }
}
