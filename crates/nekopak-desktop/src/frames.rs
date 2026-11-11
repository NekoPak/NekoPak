//! Video frame preview: a decoder running on its own thread that answers
//! seeks and paces a silent preview. Decoding is delegated to the platform
//! (Media Foundation on Windows); where none exists the viewer falls back to
//! the external player.
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::{
    sync::mpsc::{Receiver, RecvTimeoutError, Sender},
    time::{Duration, Instant},
};

pub struct Frame {
    pub pixels: SharedPixelBuffer<Rgba8Pixel>,
    pub at_ms: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct Info {
    pub duration_ms: u64,
}

pub enum Event {
    Opened(Info),
    Frame(Frame),
    /// The preview reached the end of the video.
    Ended,
    Failed(String),
}

enum Command {
    /// Target position; `exact` decodes forward to it instead of stopping at
    /// the preceding keyframe (used once a drag ends).
    Seek(u64, bool),
    Play,
    Pause,
    Stop,
}

/// What a platform decoder provides.
pub trait Decoder {
    fn info(&self) -> Info;
    /// Positions the stream and returns the first frame at or after the
    /// keyframe; with `exact` the frame at (or just after) `ms`.
    fn seek(&mut self, ms: u64, exact: bool) -> Result<Frame, String>;
    /// Next frame in presentation order; `None` at the end.
    fn next(&mut self) -> Result<Option<Frame>, String>;
}

/// Handle to a running preview. Dropping it stops the decoder thread.
pub struct Session {
    commands: Sender<Command>,
}

impl Session {
    pub fn seek(&self, ms: u64, exact: bool) {
        let _ = self.commands.send(Command::Seek(ms, exact));
    }
    pub fn play(&self) {
        let _ = self.commands.send(Command::Play);
    }
    pub fn pause(&self) {
        let _ = self.commands.send(Command::Pause);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
    }
}

/// Whether this build can decode video frames at all.
pub fn available() -> bool {
    #[cfg(feature = "automation")]
    if std::env::var_os("NEKOPAK_FAKE_FRAMES").is_some() {
        return true;
    }
    cfg!(windows)
}

fn open_decoder(url: &str) -> Result<Box<dyn Decoder>, String> {
    #[cfg(feature = "automation")]
    if std::env::var_os("NEKOPAK_FAKE_FRAMES").is_some() {
        let _ = url;
        return Ok(Box::new(synthetic::Synthetic::default()));
    }
    #[cfg(windows)]
    {
        crate::frames_win::open(url)
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        Err("no video decoder on this platform".into())
    }
}

/// Starts a decoder thread for a loopback media session URL. Events arrive on
/// `events`; the receiver decides what to show.
pub fn open(url: String, events: tokio::sync::mpsc::UnboundedSender<Event>) -> Session {
    let (commands, inbox) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("video-preview".into())
        .spawn({
            let events = events.clone();
            move || run(&url, inbox, events)
        });
    if spawned.is_err() {
        let _ = events.send(Event::Failed("could not start the preview thread".into()));
    }
    Session { commands }
}

fn run(url: &str, inbox: Receiver<Command>, events: tokio::sync::mpsc::UnboundedSender<Event>) {
    let mut decoder = match open_decoder(url) {
        Ok(decoder) => decoder,
        Err(message) => {
            let _ = events.send(Event::Failed(message));
            return;
        }
    };
    let info = decoder.info();
    if events.send(Event::Opened(info)).is_err() {
        return;
    }
    match decoder.seek(0, false) {
        Ok(frame) => {
            let _ = events.send(Event::Frame(frame));
        }
        Err(message) => {
            let _ = events.send(Event::Failed(message));
            return;
        }
    }
    // While playing: wall-clock start and the media time it corresponds to.
    let mut playing: Option<(Instant, u64)> = None;
    let mut position = 0u64;
    loop {
        let command = match playing {
            None => match inbox.recv() {
                Ok(command) => Some(command),
                Err(_) => return,
            },
            Some(_) => match inbox.try_recv() {
                Ok(command) => Some(command),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
            },
        };
        if let Some(mut command) = command {
            // A drag produces many seeks; only the newest one matters.
            while let Command::Seek(..) = command {
                match inbox.try_recv() {
                    Ok(next) => command = next,
                    Err(_) => break,
                }
            }
            match command {
                Command::Stop => return,
                Command::Pause => playing = None,
                Command::Play => {
                    if info.duration_ms > 0 && position + 250 >= info.duration_ms {
                        // Replaying from the end starts over.
                        match decoder.seek(0, false) {
                            Ok(frame) => {
                                position = frame.at_ms;
                                let _ = events.send(Event::Frame(frame));
                            }
                            Err(message) => {
                                let _ = events.send(Event::Failed(message));
                                return;
                            }
                        }
                    }
                    playing = Some((Instant::now(), position));
                }
                Command::Seek(ms, exact) => match decoder.seek(ms, exact) {
                    Ok(frame) => {
                        position = frame.at_ms;
                        if playing.is_some() {
                            playing = Some((Instant::now(), position));
                        }
                        if events.send(Event::Frame(frame)).is_err() {
                            return;
                        }
                    }
                    Err(message) => {
                        let _ = events.send(Event::Failed(message));
                        return;
                    }
                },
            }
            continue;
        }
        let Some((started, origin)) = playing else {
            continue;
        };
        match decoder.next() {
            Ok(Some(frame)) => {
                position = frame.at_ms;
                let due = started + Duration::from_millis(frame.at_ms.saturating_sub(origin));
                let now = Instant::now();
                if due > now {
                    // Wake early for commands so pause and seek feel instant.
                    match inbox.recv_timeout(due - now) {
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                        Ok(Command::Stop) => return,
                        Ok(Command::Pause) => playing = None,
                        Ok(Command::Play) => {}
                        Ok(Command::Seek(ms, exact)) => {
                            match decoder.seek(ms, exact) {
                                Ok(sought) => {
                                    position = sought.at_ms;
                                    playing = Some((Instant::now(), position));
                                    let _ = events.send(Event::Frame(sought));
                                }
                                Err(message) => {
                                    let _ = events.send(Event::Failed(message));
                                    return;
                                }
                            }
                            continue;
                        }
                    }
                } else if now - due > Duration::from_millis(120) {
                    // Decoding is behind: drop this frame rather than drift.
                    continue;
                }
                if events.send(Event::Frame(frame)).is_err() {
                    return;
                }
            }
            Ok(None) => {
                playing = None;
                let _ = events.send(Event::Ended);
            }
            Err(message) => {
                let _ = events.send(Event::Failed(message));
                return;
            }
        }
    }
}

/// Deterministic frames for headless UI tests; never part of a release build.
#[cfg(feature = "automation")]
mod synthetic {
    use super::{Decoder, Frame, Info};
    use slint::{Rgba8Pixel, SharedPixelBuffer};

    const DURATION: u64 = 12_000;
    const STEP: u64 = 40;

    #[derive(Default)]
    pub struct Synthetic {
        at: u64,
    }

    fn draw(at: u64) -> Frame {
        let (width, height) = (480u32, 270u32);
        let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
        let bar = (at * u64::from(width) / DURATION) as u32;
        for (index, pixel) in pixels.make_mut_slice().iter_mut().enumerate() {
            let (x, y) = (index as u32 % width, index as u32 / width);
            let lit = x < bar && y > height - 24;
            *pixel = Rgba8Pixel {
                r: if lit {
                    255
                } else {
                    (x * 200 / width) as u8 + 20
                },
                g: if lit {
                    255
                } else {
                    (y * 160 / height) as u8 + 30
                },
                b: if lit { 255 } else { 140 },
                a: 255,
            };
        }
        Frame { pixels, at_ms: at }
    }

    impl Decoder for Synthetic {
        fn info(&self) -> Info {
            Info {
                duration_ms: DURATION,
            }
        }
        fn seek(&mut self, ms: u64, _exact: bool) -> Result<Frame, String> {
            self.at = (ms.min(DURATION) / STEP) * STEP;
            Ok(draw(self.at))
        }
        fn next(&mut self) -> Result<Option<Frame>, String> {
            if self.at + STEP > DURATION {
                return Ok(None);
            }
            self.at += STEP;
            Ok(Some(draw(self.at)))
        }
    }
}
