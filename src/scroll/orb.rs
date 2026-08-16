use anyhow::Context;
use opencv::core::{DMatch, KeyPoint, Mat, NORM_HAMMING, Ptr, Vector};
use opencv::features2d::{
    BFMatcher, DescriptorMatcherTraitConst, Feature2DTrait, ORB, ORB_ScoreType,
};
use opencv::imgproc::{COLOR_RGBA2GRAY, cvt_color_def};
use opencv::prelude::{KeyPointTraitConst, MatTraitConst};

use crate::model::RgbaFrame;

use super::{Alignment, FrameMatcher};

const LOWE_RATIO: f32 = 0.75;
const MINIMUM_INLIER_TOLERANCE: f32 = 2.0;
const MAXIMUM_INLIER_TOLERANCE: f32 = 8.0;

pub struct OpenCvOrbMatcher {
    orb: Ptr<ORB>,
    matcher: Ptr<BFMatcher>,
    accepted: Option<Features>,
    candidate: Option<Features>,
}

impl OpenCvOrbMatcher {
    pub fn new(max_features: i32) -> anyhow::Result<Self> {
        anyhow::ensure!(max_features > 0, "ORB feature count must be positive");
        let orb = ORB::create(
            max_features,
            1.2,
            8,
            31,
            0,
            2,
            ORB_ScoreType::HARRIS_SCORE,
            31,
            20,
        )
        .context("failed to create OpenCV ORB detector")?;
        let matcher = BFMatcher::create(NORM_HAMMING, false)
            .context("failed to create OpenCV Hamming matcher")?;
        Ok(Self {
            orb,
            matcher,
            accepted: None,
            candidate: None,
        })
    }

    fn extract(&mut self, frame: &RgbaFrame) -> anyhow::Result<Features> {
        let height = i32::try_from(frame.height()).context("ORB frame height exceeds i32")?;
        let flat = Mat::from_slice(frame.pixels()).context("failed to wrap ORB RGBA frame")?;
        let rgba = flat
            .reshape(4, height)
            .context("failed to shape ORB RGBA frame")?;
        let mut grayscale = Mat::default();
        cvt_color_def(&rgba, &mut grayscale, COLOR_RGBA2GRAY)
            .context("failed to convert ORB frame to grayscale")?;

        let mut keypoints = Vector::new();
        let mut descriptors = Mat::default();
        self.orb
            .detect_and_compute_def(
                &grayscale,
                &Mat::default(),
                &mut keypoints,
                &mut descriptors,
            )
            .context("OpenCV ORB feature extraction failed")?;
        Ok(Features {
            keypoints,
            descriptors,
        })
    }

    fn match_features(&self, previous: &Features, current: &Features) -> anyhow::Result<Alignment> {
        if previous.descriptors.empty() || current.descriptors.empty() {
            return Ok(Alignment {
                horizontal_shift: 0.0,
                vertical_shift: 0.0,
                good_matches: 0,
                inliers: 0,
            });
        }

        let mut nearest: Vector<Vector<DMatch>> = Vector::new();
        self.matcher
            .knn_train_match_def(&previous.descriptors, &current.descriptors, &mut nearest, 2)
            .context("OpenCV ORB descriptor matching failed")?;

        let mut shifts = Vec::with_capacity(nearest.len());
        for pair in nearest {
            if pair.len() < 2 {
                continue;
            }
            let best = pair.get(0)?;
            let second = pair.get(1)?;
            if best.distance >= second.distance * LOWE_RATIO {
                continue;
            }
            let Some(previous_point) = keypoint(&previous.keypoints, best.query_idx) else {
                continue;
            };
            let Some(current_point) = keypoint(&current.keypoints, best.train_idx) else {
                continue;
            };
            shifts.push((
                previous_point.x - current_point.x,
                previous_point.y - current_point.y,
            ));
        }
        Ok(estimate_alignment(&shifts))
    }
}

impl FrameMatcher for OpenCvOrbMatcher {
    fn align(&mut self, previous: &RgbaFrame, current: &RgbaFrame) -> anyhow::Result<Alignment> {
        if self.accepted.is_none() {
            self.accepted = Some(self.extract(previous)?);
        }
        let candidate = self.extract(current)?;
        let alignment = self.match_features(
            self.accepted
                .as_ref()
                .expect("accepted ORB features were initialized"),
            &candidate,
        )?;
        self.candidate = Some(candidate);
        Ok(alignment)
    }

    fn accept_alignment(&mut self) {
        self.accepted = self.candidate.take();
    }
}

struct Features {
    keypoints: Vector<KeyPoint>,
    descriptors: Mat,
}

fn keypoint(keypoints: &Vector<KeyPoint>, index: i32) -> Option<opencv::core::Point2f> {
    usize::try_from(index)
        .ok()
        .and_then(|index| keypoints.get(index).ok())
        .map(|point| point.pt())
}

fn estimate_alignment(shifts: &[(f32, f32)]) -> Alignment {
    if shifts.is_empty() {
        return Alignment {
            horizontal_shift: 0.0,
            vertical_shift: 0.0,
            good_matches: 0,
            inliers: 0,
        };
    }

    let center_x = median(shifts.iter().map(|shift| shift.0));
    let center_y = median(shifts.iter().map(|shift| shift.1));
    let deviation_x = median(shifts.iter().map(|shift| (shift.0 - center_x).abs()));
    let deviation_y = median(shifts.iter().map(|shift| (shift.1 - center_y).abs()));
    let tolerance_x = (deviation_x * 2.5).clamp(MINIMUM_INLIER_TOLERANCE, MAXIMUM_INLIER_TOLERANCE);
    let tolerance_y = (deviation_y * 2.5).clamp(MINIMUM_INLIER_TOLERANCE, MAXIMUM_INLIER_TOLERANCE);
    let inliers = shifts
        .iter()
        .copied()
        .filter(|shift| {
            (shift.0 - center_x).abs() <= tolerance_x && (shift.1 - center_y).abs() <= tolerance_y
        })
        .collect::<Vec<_>>();

    Alignment {
        horizontal_shift: median(inliers.iter().map(|shift| shift.0)),
        vertical_shift: median(inliers.iter().map(|shift| shift.1)),
        good_matches: shifts.len() as u32,
        inliers: inliers.len() as u32,
    }
}

fn median(values: impl Iterator<Item = f32>) -> f32 {
    let mut values = values.collect::<Vec<_>>();
    if values.is_empty() {
        return 0.0;
    }
    values.sort_unstable_by(f32::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) * 0.5
    } else {
        values[middle]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robust_alignment_ignores_displacement_outliers() {
        let alignment = estimate_alignment(&[
            (0.0, 40.0),
            (0.5, 39.5),
            (-0.5, 40.5),
            (0.25, 40.25),
            (45.0, -120.0),
        ]);

        assert!((alignment.horizontal_shift - 0.125).abs() < 0.2);
        assert!((alignment.vertical_shift - 40.125).abs() < 0.2);
        assert_eq!(alignment.good_matches, 5);
        assert_eq!(alignment.inliers, 4);
    }

    #[test]
    fn empty_matches_produce_zero_confidence() {
        assert_eq!(
            estimate_alignment(&[]),
            Alignment {
                horizontal_shift: 0.0,
                vertical_shift: 0.0,
                good_matches: 0,
                inliers: 0,
            }
        );
    }

    #[test]
    fn opencv_orb_recovers_a_real_vertical_frame_shift() {
        let previous = patterned_view(0);
        let current = patterned_view(24);
        let mut matcher = OpenCvOrbMatcher::new(1_200).unwrap();
        let alignment = matcher.align(&previous, &current).unwrap();

        assert!(alignment.good_matches >= 12, "{alignment:?}");
        assert!(alignment.inlier_ratio() >= 0.55, "{alignment:?}");
        assert!(alignment.horizontal_shift.abs() <= 2.0, "{alignment:?}");
        assert!(
            (alignment.vertical_shift - 24.0).abs() <= 2.0,
            "{alignment:?}"
        );
    }

    fn patterned_view(start_y: u32) -> RgbaFrame {
        const WIDTH: u32 = 320;
        const HEIGHT: u32 = 180;
        let mut pixels = Vec::with_capacity(WIDTH as usize * HEIGHT as usize * 4);
        for y in start_y..start_y + HEIGHT {
            for x in 0..WIDTH {
                let block = ((x / 11) * 37 + (y / 9) * 53 + ((x + y) / 17) * 29) as u8;
                let edge = if x % 31 < 3 || y % 27 < 3 { 220 } else { block };
                pixels.extend_from_slice(&[
                    edge,
                    edge.wrapping_add(17),
                    edge.wrapping_add(41),
                    255,
                ]);
            }
        }
        RgbaFrame::new(crate::model::RectI::new(0, 0, WIDTH, HEIGHT), pixels).unwrap()
    }
}
