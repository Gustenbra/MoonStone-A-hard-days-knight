//! Where sound actually comes out.
//!
//! Behind a trait, for three reasons: a browser build needs a different backend
//! entirely, a headless server needs none at all, and tests need to assert what
//! was played without a sound card in the room.

use std::collections::BTreeMap;

pub trait Sink {
    /// Play a clip by asset id. Unknown ids are ignored: a missing sound should
    /// never be louder than a missing sound.
    fn play(&mut self, id: &str);
}

/// Clip bytes by asset id. Held as raw file contents so the backend decides how
/// to decode them, and so the same library serves a browser build.
#[derive(Default)]
pub struct Clips(pub BTreeMap<String, Vec<u8>>);

impl Clips {
    pub fn insert(&mut self, id: impl Into<String>, bytes: Vec<u8>) {
        self.0.insert(id.into(), bytes);
    }
    pub fn get(&self, id: &str) -> Option<&Vec<u8>> {
        self.0.get(id)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Plays nothing. What a server uses, and what the desktop falls back to when
/// there is no audio device: no sound is a far better outcome than no game.
#[derive(Default)]
pub struct Silent;

impl Sink for Silent {
    fn play(&mut self, _id: &str) {}
}

/// Records what it was asked to play, for tests.
#[derive(Default)]
pub struct Recording(pub Vec<String>);

impl Sink for Recording {
    fn play(&mut self, id: &str) {
        self.0.push(id.to_string());
    }
}

#[cfg(feature = "native")]
mod native {
    use super::{Clips, Sink};
    use std::io::Cursor;

    /// The desktop backend.
    pub struct Native {
        clips: Clips,
        // Held only to keep the device open; dropping it silences everything.
        _stream: rodio::OutputStream,
        handle: rodio::OutputStreamHandle,
    }

    impl Native {
        /// Fails rather than panics when no device is available, which is the
        /// normal state in a container, over a bare SSH session, and in CI.
        pub fn new(clips: Clips) -> Result<Native, String> {
            let (stream, handle) =
                rodio::OutputStream::try_default().map_err(|e| e.to_string())?;
            Ok(Native { clips, _stream: stream, handle })
        }
    }

    impl Sink for Native {
        fn play(&mut self, id: &str) {
            let Some(bytes) = self.clips.get(id) else { return };
            // A fresh sink per clip, so sounds overlap instead of cutting each
            // other off. A four way fight is meant to be noisy.
            if let Ok(decoder) = rodio::Decoder::new(Cursor::new(bytes.clone())) {
                let _ = self.handle.play_raw(rodio::source::Source::convert_samples(decoder));
            }
        }
    }
}

#[cfg(feature = "native")]
pub use native::Native;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_swallows_everything() {
        let mut s = Silent;
        s.play("sfx.swish");
    }

    #[test]
    fn a_recording_sink_reports_what_it_was_asked_for() {
        let mut s = Recording::default();
        s.play("sfx.swish");
        s.play("sfx.hit3");
        assert_eq!(s.0, vec!["sfx.swish", "sfx.hit3"]);
    }

    #[test]
    fn an_unknown_id_is_simply_not_played() {
        let mut clips = Clips::default();
        clips.insert("sfx.known", vec![1, 2, 3]);
        assert!(clips.get("sfx.unknown").is_none());
        assert_eq!(clips.len(), 1);
    }
}
