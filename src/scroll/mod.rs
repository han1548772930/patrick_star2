//! Scroll-frame matching, stitching, and capture-session state.

#[cfg(feature = "opencv-orb")]
mod orb;
#[cfg(feature = "opencv-orb")]
mod session;
mod stitch;
#[cfg(feature = "opencv-orb")]
mod tiled;
#[cfg(feature = "opencv-orb")]
mod worker;

pub use stitch::{OwnedPreviewPatch, PreviewPatch, PreviewRegion};
#[cfg(feature = "opencv-orb")]
pub(crate) use tiled::TiledImage;
#[cfg(feature = "opencv-orb")]
pub use worker::{ScrollCaptureWorker, ScrollWorkerEvent};
