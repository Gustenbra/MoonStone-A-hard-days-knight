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

    /// Start a tune, looping, and stop whatever else was playing. Asking for
    /// the tune that is already on is not a restart, because the original's
    /// own loaders call `LOADMUSIC` again on every visit to the same room.
    fn play_music(&mut self, _id: &str) {}

    /// Silence. `mov ah, 2; int 60h`, which every one of the original's rooms
    /// does on the way out.
    fn stop_music(&mut self) {}

    /// Which tune is playing, if any. Ours, so a caller need not remember.
    fn music(&self) -> Option<&str> {
        None
    }
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
///
/// It still keeps track of which tune it was asked for, so a headless run can
/// say what the music *would* be doing without a sound card in the room.
#[derive(Default)]
pub struct Silent {
    playing: Option<String>,
}

impl Sink for Silent {
    fn play(&mut self, _id: &str) {}

    fn play_music(&mut self, id: &str) {
        if self.playing.as_deref() != Some(id) {
            self.playing = Some(id.to_string());
        }
    }

    fn stop_music(&mut self) {
        self.playing = None;
    }

    fn music(&self) -> Option<&str> {
        self.playing.as_deref()
    }
}

/// Records what it was asked to play, for tests.
#[derive(Default)]
pub struct Recording(pub Vec<String>, Option<String>);

impl Sink for Recording {
    fn play(&mut self, id: &str) {
        self.0.push(id.to_string());
    }

    fn play_music(&mut self, id: &str) {
        if self.1.as_deref() == Some(id) {
            return;
        }
        self.0.push(format!("music:{id}"));
        self.1 = Some(id.to_string());
    }

    fn stop_music(&mut self) {
        if self.1.take().is_some() {
            self.0.push("music:stop".into());
        }
    }

    fn music(&self) -> Option<&str> {
        self.1.as_deref()
    }
}

#[cfg(feature = "native")]
mod native {
    use super::{Clips, Sink};
    use std::io::Cursor;

    use crate::music::{self, Score};
    use std::collections::BTreeMap;

    /// The desktop backend.
    pub struct Native {
        clips: Clips,
        /// Tunes by asset id, as recovered scores rather than as audio.
        scores: BTreeMap<String, Score>,
        /// Rendered tunes, kept because rendering one is work and a room is
        /// walked into more than once.
        rendered: BTreeMap<String, std::sync::Arc<Vec<i16>>>,
        /// The tune that is playing, and the handle that stops it.
        playing: Option<(String, rodio::Sink)>,
        // Held only to keep the device open; dropping it silences everything.
        _stream: rodio::OutputStream,
        handle: rodio::OutputStreamHandle,
    }

    impl Native {
        /// Fails rather than panics when no device is available, which is the
        /// normal state in a container, over a bare SSH session, and in CI.
        pub fn new(clips: Clips) -> Result<Native, String> {
            let (stream, handle) = rodio::OutputStream::try_default().map_err(|e| e.to_string())?;
            Ok(Native {
                clips,
                scores: BTreeMap::new(),
                rendered: BTreeMap::new(),
                playing: None,
                _stream: stream,
                handle,
            })
        }

        /// Hands over a tune, as the score rather than as audio. Rendering waits
        /// until something asks to hear it, because a run may never open a door
        /// that has music behind it.
        pub fn add_score(&mut self, id: impl Into<String>, score: Score) {
            self.scores.insert(id.into(), score);
        }

        pub fn scores(&self) -> usize {
            self.scores.len()
        }
    }

    impl Sink for Native {
        fn play(&mut self, id: &str) {
            let Some(bytes) = self.clips.get(id) else {
                return;
            };
            // A fresh sink per clip, so sounds overlap instead of cutting each
            // other off. A four way fight is meant to be noisy.
            if let Ok(decoder) = rodio::Decoder::new(Cursor::new(bytes.clone())) {
                let _ = self
                    .handle
                    .play_raw(rodio::source::Source::convert_samples(decoder));
            }
        }

        fn play_music(&mut self, id: &str) {
            if self.playing.as_ref().is_some_and(|(cur, _)| cur == id) {
                return;
            }
            let Some(score) = self.scores.get(id) else {
                return;
            };
            let pcm = match self.rendered.get(id) {
                Some(p) => p.clone(),
                None => {
                    let p = std::sync::Arc::new(music::render(score));
                    self.rendered.insert(id.to_string(), p.clone());
                    p
                }
            };
            self.stop_music();
            let Ok(sink) = rodio::Sink::try_new(&self.handle) else {
                return;
            };
            let buf = rodio::buffer::SamplesBuffer::new(1, music::RATE, pcm.as_slice().to_vec());
            sink.append(rodio::source::Source::repeat_infinite(buf));
            self.playing = Some((id.to_string(), sink));
        }

        fn stop_music(&mut self) {
            if let Some((_, sink)) = self.playing.take() {
                sink.stop();
            }
        }

        fn music(&self) -> Option<&str> {
            self.playing.as_ref().map(|(id, _)| id.as_str())
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
        let mut s = Silent::default();
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
    fn silence_still_knows_which_tune_it_is_not_playing() {
        let mut s = Silent::default();
        assert_eq!(s.music(), None);
        s.play_music("music.tune3");
        assert_eq!(s.music(), Some("music.tune3"));
        s.stop_music();
        assert_eq!(s.music(), None);
    }

    #[test]
    fn asking_for_the_tune_already_playing_is_not_a_restart() {
        let mut s = Recording::default();
        s.play_music("music.tune3");
        s.play_music("music.tune3");
        s.play_music("music.tune3");
        assert_eq!(s.0, vec!["music:music.tune3"]);
        assert_eq!(s.music(), Some("music.tune3"));
    }

    #[test]
    fn a_different_tune_replaces_the_one_playing_and_stopping_is_idempotent() {
        let mut s = Recording::default();
        s.play_music("music.tune2");
        s.play_music("music.tune4");
        s.stop_music();
        s.stop_music();
        assert_eq!(
            s.0,
            vec!["music:music.tune2", "music:music.tune4", "music:stop"]
        );
        assert_eq!(s.music(), None);
    }

    #[test]
    fn an_unknown_id_is_simply_not_played() {
        let mut clips = Clips::default();
        clips.insert("sfx.known", vec![1, 2, 3]);
        assert!(clips.get("sfx.unknown").is_none());
        assert_eq!(clips.len(), 1);
    }
}
