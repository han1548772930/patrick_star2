//! Scroll-frame matching, stitching, and capture-session state.

mod fingerprint;
#[cfg(feature = "opencv-orb")]
mod orb;
mod session;
mod stitch;

pub use fingerprint::FrameFingerprint;
#[cfg(feature = "opencv-orb")]
pub use orb::OpenCvOrbMatcher;
pub use session::{Alignment, FrameMatcher, PushOutcome, ScrollConfig, ScrollSession};
pub use stitch::{PreviewPatch, PreviewRegion, StitchDocument};
