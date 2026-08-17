use std::collections::BTreeMap;

use anyhow::{Context, Result};
use opencv::core::{self, DMatch, Mat};
use opencv::features2d;
use opencv::prelude::*;

use crate::model::RgbaFrame;
use crate::platform::ScrollDirection;

const SUPPORT_WEIGHT: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameMatch {
    pub(crate) shift: i32,
    pub(crate) raw_shift: i32,
    pub(crate) rejected: bool,
}

pub(crate) fn match_frames(
    previous: &RgbaFrame,
    next: &RgbaFrame,
    _direction: ScrollDirection,
) -> Result<FrameMatch> {
    anyhow::ensure!(
        previous.width() == next.width() && previous.height() == next.height(),
        "scroll frame dimensions changed"
    );
    let width = previous.width();
    let height = previous.height();
    anyhow::ensure!(
        width >= 3 && height >= 3,
        "capture region is too small for scrolling matching"
    );

    let inset_height = height.saturating_sub(2);
    let identical_top = identical_edge_rows(previous, next, false);
    if identical_top >= inset_height {
        return Ok(no_movement());
    }
    let identical_bottom = identical_edge_rows(previous, next, true);
    if identical_top.saturating_add(identical_bottom) > inset_height {
        return Ok(no_movement());
    }

    let crop_top = identical_top.max(31) - 31;
    let crop_bottom = identical_bottom.max(31) - 31;
    let crop_height = inset_height - crop_top - crop_bottom;
    let previous_mat = bgra_mat(previous, 1, 1 + crop_top, width - 2, crop_height)?;
    let next_mat = bgra_mat(next, 1, 1 + crop_top, width - 2, crop_height)?;
    let mut orb = features2d::ORB::create(
        2_000,
        1.2,
        8,
        31,
        0,
        2,
        features2d::ORB_ScoreType::HARRIS_SCORE,
        31,
        20,
    )
    .context("create scrolling ORB detector")?;

    let mut previous_keypoints = core::Vector::<core::KeyPoint>::new();
    let mut previous_descriptors = Mat::default();
    orb.detect_and_compute_def(
        &previous_mat,
        &Mat::default(),
        &mut previous_keypoints,
        &mut previous_descriptors,
    )
    .context("extract previous scrolling-frame features")?;
    let mut next_keypoints = core::Vector::<core::KeyPoint>::new();
    let mut next_descriptors = Mat::default();
    orb.detect_and_compute_def(
        &next_mat,
        &Mat::default(),
        &mut next_keypoints,
        &mut next_descriptors,
    )
    .context("extract next scrolling-frame features")?;
    if previous_descriptors.empty() || next_descriptors.empty() {
        return Ok(no_movement());
    }

    let matcher = features2d::BFMatcher::create(core::NORM_HAMMING, false)
        .context("create scrolling Hamming matcher")?;
    let mut matches = core::Vector::<core::Vector<DMatch>>::new();
    matcher
        .knn_train_match_def(&previous_descriptors, &next_descriptors, &mut matches, 5)
        .context("match scrolling ORB descriptors")?;

    let mut offsets = Vec::new();
    let mut groups = Vec::with_capacity(matches.len());
    for group in matches {
        let mut stored = Vec::with_capacity(group.len());
        for index in 0..group.len() {
            stored.push(
                group
                    .get(index)
                    .context("read scrolling descriptor match")?,
            );
        }
        if stored.len() >= 2 {
            let best = stored[0];
            let second = stored[1];
            if second.distance * 0.75 > best.distance {
                for matched in [best, second] {
                    if matched.distance > 20.0 {
                        continue;
                    }
                    let previous_point = previous_keypoints
                        .get(matched.query_idx as usize)
                        .context("read previous scrolling keypoint")?
                        .pt();
                    let next_point = next_keypoints
                        .get(matched.train_idx as usize)
                        .context("read next scrolling keypoint")?
                        .pt();
                    if (previous_point.x.round() as i32 - next_point.x.round() as i32).abs() > 4 {
                        continue;
                    }
                    let mut offset = next_point.y.round() as i32 - previous_point.y.round() as i32;
                    if offset.abs() < 2 {
                        offset = 0;
                    }
                    offsets.push(offset);
                }
            }
        }
        groups.push(stored);
    }
    if offsets.is_empty() {
        return Ok(no_movement());
    }

    let mut displacements = Vec::with_capacity(offsets.len());
    for matched in groups.iter().flatten() {
        if matched.distance > 20.0 {
            continue;
        }
        let previous_point = previous_keypoints
            .get(matched.query_idx as usize)
            .context("read previous scrolling vote keypoint")?
            .pt();
        let next_point = next_keypoints
            .get(matched.train_idx as usize)
            .context("read next scrolling vote keypoint")?
            .pt();
        displacements.push((
            previous_point.x.round() as i32 - next_point.x.round() as i32,
            next_point.y.round() as i32 - previous_point.y.round() as i32,
        ));
    }
    Ok(select_shift(&offsets, &displacements, height))
}

fn no_movement() -> FrameMatch {
    FrameMatch {
        shift: 0,
        raw_shift: 0,
        rejected: false,
    }
}

fn select_shift(offsets: &[i32], displacements: &[(i32, i32)], height: u32) -> FrameMatch {
    let mut exact_counts = BTreeMap::<i32, usize>::new();
    for &offset in offsets {
        *exact_counts.entry(offset).or_default() += 1;
    }
    let zero_votes = exact_counts.get(&0).copied().unwrap_or(0);
    let candidates = exact_counts.into_iter().filter_map(|(candidate, count)| {
        (zero_votes.saturating_add(count.saturating_mul(SUPPORT_WEIGHT)) >= offsets.len())
            .then_some(candidate)
    });
    let mut shift = 0;
    let mut support = 0;
    let mut has_candidate = false;
    for candidate in candidates {
        has_candidate = true;
        let votes = displacements
            .iter()
            .filter(|&&(dx, dy)| dx.abs() <= 4 && (dy - candidate).abs() <= 1)
            .count();
        if votes >= support {
            shift = candidate;
        }
        support = support.max(votes);
    }
    if !has_candidate {
        shift = 999_999;
    }
    let rejected = shift.unsigned_abs() > height.saturating_mul(3) / 5;
    FrameMatch {
        shift: if rejected { 0 } else { shift },
        raw_shift: shift,
        rejected,
    }
}

fn bgra_mat(image: &RgbaFrame, left: u32, top: u32, width: u32, height: u32) -> Result<Mat> {
    let stride = image.width() as usize * 4;
    let mut bgra = Vec::with_capacity(width as usize * height as usize * 4);
    for row in top..top + height {
        let start = row as usize * stride + left as usize * 4;
        for pixel in image.pixels()[start..start + width as usize * 4].chunks_exact(4) {
            bgra.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    Mat::from_slice(&bgra)
        .and_then(|mat| mat.reshape(4, height as i32)?.try_clone())
        .context("construct scrolling OpenCV image")
}

fn identical_edge_rows(left: &RgbaFrame, right: &RgbaFrame, from_bottom: bool) -> u32 {
    let width = left.width().min(right.width());
    let height = left.height().min(right.height());
    if width <= 2 || height <= 2 {
        return 0;
    }
    let compared = width - 2;
    let rows: Box<dyn Iterator<Item = u32>> = if from_bottom {
        Box::new((1..height - 1).rev())
    } else {
        Box::new(1..height - 1)
    };
    let left_stride = left.width() as usize * 4;
    let right_stride = right.width() as usize * 4;
    rows.take_while(|&row| {
        let left_row = &left.pixels()[row as usize * left_stride..];
        let right_row = &right.pixels()[row as usize * right_stride..];
        (1..=compared).all(|x| {
            let at = x as usize * 4;
            left_row[at..at + 3] == right_row[at..at + 3]
        })
    })
    .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tied_candidates_choose_the_later_offset() {
        let result = select_shift(&[3, 3, 7, 7], &[(0, 3), (0, 3), (0, 7), (0, 7)], 100);
        assert_eq!(result.shift, 7);
    }

    #[test]
    fn offsets_beyond_sixty_percent_are_rejected() {
        let result = select_shift(&[61], &[(0, 61)], 100);
        assert!(result.rejected);
        assert_eq!(result.raw_shift, 61);
        assert_eq!(result.shift, 0);
    }
}
