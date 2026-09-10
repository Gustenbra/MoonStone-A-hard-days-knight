//! An answer that is not here yet.
//!
//! **Ours**, like everything in this crate. The game loop has a tick to give and
//! a socket does not care: connecting to a list server on the far side of the
//! country takes as long as it takes, and a connection that will never be made
//! takes the whole of [`crate::list::PATIENCE`] to say so. Doing that on the
//! frame's own thread stops the picture dead, which is what a person reads as
//! the game having crashed.
//!
//! So the ask goes on a thread of its own and the answer is collected later, the
//! way [`crate::Opener`] already collects the router's. Nothing here is shared
//! but the channel: the work runs once, sends once, and ends.

/// Work handed to a thread, and its answer once there is one.
pub struct Later<T> {
    rx: std::sync::mpsc::Receiver<T>,
    /// Whether the thread has already answered and been collected, so a caller
    /// polling every frame is told "still waiting" and not "gone".
    done: bool,
}

impl<T: Send + 'static> Later<T> {
    /// Start the work. It runs once, on its own thread, and is never waited for.
    pub fn start(work: impl FnOnce() -> T + Send + 'static) -> Later<T> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        Later { rx, done: false }
    }

    /// The answer, once, or nothing while the work is still running.
    ///
    /// Unlike [`crate::Opener::ready`] this hands the answer over rather than
    /// keeping it, because what comes back here is a live socket and a socket
    /// belongs to one owner.
    pub fn take(&mut self) -> Option<T> {
        if self.done {
            return None;
        }
        match self.rx.try_recv() {
            Ok(v) => {
                self.done = true;
                Some(v)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            // The thread ended without sending, which it cannot do short of a
            // panic. Treated as an answer that will never come rather than as
            // something to keep asking about.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.done = true;
                None
            }
        }
    }

    /// Whether there is still something to wait for.
    pub fn waiting(&self) -> bool {
        !self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn the_answer_arrives_once_and_only_once() {
        let mut l = Later::start(|| {
            std::thread::sleep(Duration::from_millis(20));
            41 + 1
        });
        let stop = Instant::now() + Duration::from_secs(5);
        let mut got = None;
        while Instant::now() < stop && got.is_none() {
            got = l.take();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(got, Some(42));
        assert_eq!(l.take(), None, "and it is not handed over twice");
        assert!(!l.waiting());
    }

    /// The point of the thing: asking does not stop the caller.
    #[test]
    fn starting_the_work_does_not_wait_for_it() {
        let began = Instant::now();
        let mut l = Later::start(|| {
            std::thread::sleep(Duration::from_millis(300));
            "slow"
        });
        assert!(
            began.elapsed() < Duration::from_millis(100),
            "start blocked for {:?}",
            began.elapsed()
        );
        assert_eq!(l.take(), None, "and there is nothing to collect yet");
        assert!(l.waiting());
    }
}
