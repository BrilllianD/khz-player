//! Video playback in a separate mpv window (libmpv, feature `video`). Without the feature
//! `VideoPlayer` is a stub whose `new` fails, so callers fall back to audio only.

#[cfg_attr(not(feature = "video"), allow(dead_code))]
mod events;
pub use events::VideoEvent;

#[cfg(feature = "video")]
mod player;
#[cfg(feature = "video")]
pub use player::VideoPlayer;

#[cfg(not(feature = "video"))]
pub use stub::VideoPlayer;

#[cfg(not(feature = "video"))]
mod stub {
    use std::path::Path;
    use std::time::Duration;

    use super::VideoEvent;

    /// Placeholder in builds without the `video` feature; it cannot be constructed.
    pub struct VideoPlayer {
        never: std::convert::Infallible,
    }

    impl VideoPlayer {
        pub fn new(_repaint: egui::Context) -> anyhow::Result<Self> {
            Err(anyhow::anyhow!("built without the video feature"))
        }
        pub fn load(&self, _path: &Path, _play: bool, _start: Duration) {
            match self.never {}
        }
        pub fn play(&self) {
            match self.never {}
        }
        pub fn pause(&self) {
            match self.never {}
        }
        pub fn stop(&self) {
            match self.never {}
        }
        pub fn seek(&self, _to: Duration) {
            match self.never {}
        }
        pub fn seek_rel(&self, _ms: i64) {
            match self.never {}
        }
        pub fn set_volume(&self, _v: f32) {
            match self.never {}
        }
        pub fn set_device(&self, _dev: Option<&str>) {
            match self.never {}
        }
        pub fn raise(&self) {
            match self.never {}
        }
        pub fn position(&self) -> Duration {
            match self.never {}
        }
        pub fn poll(&self) -> Vec<VideoEvent> {
            match self.never {}
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn new_fails_without_video() {
            assert!(VideoPlayer::new(egui::Context::default()).is_err());
        }
    }
}
