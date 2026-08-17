use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use anyhow::{Context, Result};

use crate::model::RgbaFrame;
use crate::platform::{CapturedScrollFrame, ScrollDirection};

use super::OwnedPreviewPatch;
use super::session::{MatchedFrame, ScrollCaptureSession};

const MAX_PENDING_MATCHES: usize = 1;

enum MatcherCommand {
    Frame {
        splice_id: u64,
        captured: CapturedScrollFrame,
    },
    Finish(Sender<RgbaFrame>),
    Stop,
}

enum SpliceCommand {
    Frame(MatchedFrame),
    Finish(Sender<RgbaFrame>),
    Stop,
}

#[derive(Debug)]
pub enum ScrollWorkerEvent {
    Preview(OwnedPreviewPatch),
    FrameProcessed,
    FrameDiscarded(String),
}

pub struct ScrollCaptureWorker {
    commands: Sender<MatcherCommand>,
    events: Receiver<ScrollWorkerEvent>,
    pending: Arc<AtomicUsize>,
    next_splice_id: AtomicU64,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ScrollCaptureWorker {
    pub fn new(initial: RgbaFrame) -> Result<Self> {
        let session = ScrollCaptureSession::new(initial);
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let pending = Arc::new(AtomicUsize::new(0));
        let worker_pending = pending.clone();
        let thread = std::thread::Builder::new()
            .name("scroll-capture-matcher".to_owned())
            .spawn(move || run_matcher(session, command_rx, event_tx, worker_pending))
            .context("spawn scrolling matcher worker")?;
        Ok(Self {
            commands: command_tx,
            events: event_rx,
            pending,
            next_splice_id: AtomicU64::new(1),
            thread: Some(thread),
        })
    }

    /// Uses a one-frame gate. The platform source keeps replacing its own
    /// single slot while this returns false, so old frames never queue up.
    pub fn push_frame(&self, captured: CapturedScrollFrame) -> Result<bool> {
        if self
            .pending
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                (pending < MAX_PENDING_MATCHES).then_some(pending + 1)
            })
            .is_err()
        {
            return Ok(false);
        }
        let command = MatcherCommand::Frame {
            splice_id: self.next_splice_id.fetch_add(1, Ordering::Relaxed),
            captured,
        };
        if self.commands.send(command).is_err() {
            self.pending.fetch_sub(1, Ordering::AcqRel);
            anyhow::bail!("scrolling matcher worker stopped");
        }
        Ok(true)
    }

    pub fn poll_event(&self) -> Option<ScrollWorkerEvent> {
        self.events.try_recv().ok()
    }

    pub fn finish(&self) -> Result<RgbaFrame> {
        let (response_tx, response_rx) = mpsc::channel();
        self.commands
            .send(MatcherCommand::Finish(response_tx))
            .map_err(|_| anyhow::anyhow!("scrolling matcher worker stopped"))?;
        response_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("scrolling splice worker stopped before export"))
    }
}

impl Drop for ScrollCaptureWorker {
    fn drop(&mut self) {
        let _ = self.commands.send(MatcherCommand::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_matcher(
    mut session: ScrollCaptureSession,
    commands: Receiver<MatcherCommand>,
    events: Sender<ScrollWorkerEvent>,
    pending: Arc<AtomicUsize>,
) {
    let splice_session = session.clone();
    let (splice_tx, splice_rx) = mpsc::channel();
    let splice_events = events.clone();
    let splice_thread = std::thread::Builder::new()
        .name("scroll-capture-splice".to_owned())
        .spawn(move || run_splice(splice_session, splice_rx, splice_events))
        .expect("spawn scrolling splice worker");
    let mut previous_native_position = None;

    while let Ok(command) = commands.recv() {
        match command {
            MatcherCommand::Frame {
                splice_id,
                captured,
            } => {
                let direction = resolve_direction(&captured, &mut previous_native_position);
                if captured.discontinuity {
                    session.reset_baseline(captured.frame);
                    let _ = events.send(ScrollWorkerEvent::FrameDiscarded(
                        "scroll source reported a discontinuity".to_owned(),
                    ));
                    pending.fetch_sub(1, Ordering::AcqRel);
                    continue;
                }
                match session.match_frame(splice_id, captured, direction) {
                    Ok(Some(matched)) => {
                        let stopped = splice_tx.send(SpliceCommand::Frame(matched)).is_err();
                        pending.fetch_sub(1, Ordering::AcqRel);
                        if stopped {
                            break;
                        }
                    }
                    Ok(None) => {
                        let _ = events.send(ScrollWorkerEvent::FrameProcessed);
                        pending.fetch_sub(1, Ordering::AcqRel);
                    }
                    Err(error) => {
                        let _ = events.send(ScrollWorkerEvent::FrameDiscarded(error.to_string()));
                        pending.fetch_sub(1, Ordering::AcqRel);
                    }
                }
            }
            MatcherCommand::Finish(response) => {
                let _ = splice_tx.send(SpliceCommand::Finish(response));
                break;
            }
            MatcherCommand::Stop => {
                let _ = splice_tx.send(SpliceCommand::Stop);
                break;
            }
        }
    }
    let _ = splice_thread.join();
}

fn run_splice(
    mut session: ScrollCaptureSession,
    commands: Receiver<SpliceCommand>,
    events: Sender<ScrollWorkerEvent>,
) {
    let mut last_splice_id = 0;
    while let Ok(command) = commands.recv() {
        match command {
            SpliceCommand::Frame(mut matched) => {
                if matched.splice_id <= last_splice_id {
                    let _ = events.send(ScrollWorkerEvent::FrameDiscarded(
                        "stale scrolling frame reached the splice worker".to_owned(),
                    ));
                    continue;
                }
                last_splice_id = matched.splice_id;
                let result = session
                    .plan_matched_frame(&mut matched)
                    .and_then(|()| session.commit_matched_frame(matched));
                match result {
                    Ok(Some(patch)) => {
                        let _ = events.send(ScrollWorkerEvent::Preview(patch));
                    }
                    Ok(None) => {
                        let _ = events.send(ScrollWorkerEvent::FrameProcessed);
                    }
                    Err(error) => {
                        let _ = events.send(ScrollWorkerEvent::FrameDiscarded(error.to_string()));
                    }
                }
            }
            SpliceCommand::Finish(response) => {
                let _ = response.send(session.finish());
                break;
            }
            SpliceCommand::Stop => break,
        }
    }
}

fn resolve_direction(
    captured: &CapturedScrollFrame,
    previous_native_position: &mut Option<i64>,
) -> ScrollDirection {
    let native_direction = match (*previous_native_position, captured.native_scroll_position) {
        (Some(previous), Some(current)) if current > previous => ScrollDirection::Down,
        (Some(previous), Some(current)) if current < previous => ScrollDirection::Up,
        _ => ScrollDirection::Unknown,
    };
    if let Some(position) = captured.native_scroll_position {
        *previous_native_position = Some(position);
    }
    if captured.direction == ScrollDirection::Unknown {
        native_direction
    } else {
        captured.direction
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RectI;
    use std::time::Instant;

    fn captured(position: Option<i64>, direction: ScrollDirection) -> CapturedScrollFrame {
        CapturedScrollFrame {
            frame: RgbaFrame::new(RectI::new(0, 0, 1, 1), vec![0, 0, 0, 255]).unwrap(),
            captured_at: Instant::now(),
            direction,
            wheel_sequence: 0,
            native_scroll_position: position,
            discontinuity: false,
        }
    }

    #[test]
    fn native_position_supplies_direction_when_the_platform_has_no_wheel_hint() {
        let mut previous = Some(10);
        assert_eq!(
            resolve_direction(&captured(Some(12), ScrollDirection::Unknown), &mut previous),
            ScrollDirection::Down
        );
    }
}
