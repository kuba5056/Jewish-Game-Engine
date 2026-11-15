//! Sound playback on `kira`.
//!
//! Load a [`Sound`] once, then [`Audio::play`] it as often as needed. If the
//! machine has no working audio output, everything still runs; it is just
//! silent.

use std::path::Path;

use kira::backend::mock::MockBackend;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::sound::PlaybackState;
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Tween};

/// A decoded sound held in memory, ready to play. Cheap to clone.
///
/// Meant for sound effects and short music loops: the whole file is
/// decoded up front.
#[derive(Clone)]
pub struct Sound(StaticSoundData);

impl Sound {
    /// Reads and decodes a WAV, OGG Vorbis, MP3 or FLAC file.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        StaticSoundData::from_file(path)
            .map(Self)
            .map_err(|e| e.to_string())
    }

    /// Length in seconds.
    pub fn duration(&self) -> f32 {
        self.0.duration().as_secs_f32()
    }
}

/// How one playback of a sound should be heard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playback {
    /// Loudness as a multiplier: 1 is unchanged, 0.5 is half, 0 is silent.
    pub volume: f32,
    /// Speed and pitch together: 2 is an octave up and twice as fast.
    pub pitch: f32,
    /// Stereo position from -1 (left) through 0 (centre) to 1 (right).
    pub pan: f32,
    /// Repeat until stopped.
    pub looping: bool,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pitch: 1.0,
            pan: 0.0,
            looping: false,
        }
    }
}

/// Converts a loudness multiplier to the decibels kira works in.
fn decibels(volume: f32) -> Decibels {
    if volume <= 0.001 {
        Decibels::SILENCE
    } else {
        Decibels(20.0 * volume.log10())
    }
}

/// Controls a sound that is playing. Dropping it lets the sound play on.
pub struct SoundHandle(Option<StaticSoundHandle>);

impl SoundHandle {
    /// Stops the sound (with a few milliseconds of fade to avoid a click).
    pub fn stop(&mut self) {
        if let Some(handle) = &mut self.0 {
            handle.stop(Tween::default());
        }
    }

    pub fn pause(&mut self) {
        if let Some(handle) = &mut self.0 {
            handle.pause(Tween::default());
        }
    }

    pub fn resume(&mut self) {
        if let Some(handle) = &mut self.0 {
            handle.resume(Tween::default());
        }
    }

    /// Changes the loudness; see [`Playback::volume`].
    pub fn set_volume(&mut self, volume: f32) {
        if let Some(handle) = &mut self.0 {
            handle.set_volume(decibels(volume), Tween::default());
        }
    }

    /// Whether the sound has neither finished nor been stopped. Always
    /// `false` when there is no audio output.
    pub fn is_playing(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|handle| handle.state() != PlaybackState::Stopped)
    }
}

enum Output {
    Device(Box<AudioManager<DefaultBackend>>),
    /// Runs the full playback path without producing sound.
    Headless(Box<AudioManager<MockBackend>>),
    /// No usable audio output: every call does nothing.
    Disabled,
}

/// Plays sounds through the system's default audio output.
pub struct Audio {
    output: Output,
}

impl Audio {
    /// Opens the default audio output. If that fails (no sound card, no
    /// audio server), logs a warning and returns a silent `Audio`.
    pub fn new() -> Self {
        let output = match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
            Ok(manager) => Output::Device(Box::new(manager)),
            Err(e) => {
                log::warn!("no audio output, sound is off: {e}");
                Output::Disabled
            }
        };
        Self { output }
    }

    /// An `Audio` that goes through all the motions without a sound device,
    /// for tests and servers.
    pub fn headless() -> Self {
        let output = match AudioManager::<MockBackend>::new(AudioManagerSettings::default()) {
            Ok(manager) => Output::Headless(Box::new(manager)),
            Err(_) => Output::Disabled,
        };
        Self { output }
    }

    /// Whether sounds can actually be heard.
    pub fn is_available(&self) -> bool {
        matches!(self.output, Output::Device(_))
    }

    /// Plays a sound once at normal volume.
    pub fn play(&mut self, sound: &Sound) -> SoundHandle {
        self.play_with(sound, Playback::default())
    }

    /// Plays a sound with the given volume, pitch, pan and looping.
    pub fn play_with(&mut self, sound: &Sound, playback: Playback) -> SoundHandle {
        let mut data = sound
            .0
            .volume(decibels(playback.volume))
            .playback_rate(playback.pitch.max(0.01) as f64)
            .panning(playback.pan.clamp(-1.0, 1.0));
        if playback.looping {
            data = data.loop_region(..);
        }
        let played = match &mut self.output {
            Output::Device(manager) => manager.play(data).map_err(|e| e.to_string()),
            Output::Headless(manager) => manager.play(data).map_err(|e| e.to_string()),
            Output::Disabled => return SoundHandle(None),
        };
        match played {
            Ok(handle) => SoundHandle(Some(handle)),
            Err(e) => {
                // Typically too many sounds playing at once.
                log::warn!("could not play sound: {e}");
                SoundHandle(None)
            }
        }
    }

    /// Sets the loudness of everything; see [`Playback::volume`].
    pub fn set_master_volume(&mut self, volume: f32) {
        let volume = decibels(volume);
        match &mut self.output {
            Output::Device(manager) => manager.main_track().set_volume(volume, Tween::default()),
            Output::Headless(manager) => manager.main_track().set_volume(volume, Tween::default()),
            Output::Disabled => {}
        }
    }
}

impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn thud() -> Sound {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/sounds/thud.wav");
        Sound::from_file(&path).unwrap()
    }

    #[test]
    fn decodes_a_wav_file() {
        let duration = thud().duration();
        assert!((duration - 0.22).abs() < 0.01, "{duration}");
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(Sound::from_file(Path::new("no/such/sound.wav")).is_err());
    }

    #[test]
    fn volume_converts_to_decibels() {
        assert_eq!(decibels(1.0), Decibels(0.0));
        assert!((decibels(0.5).0 + 6.02).abs() < 0.01);
        assert_eq!(decibels(0.0), Decibels::SILENCE);
        assert_eq!(decibels(-1.0), Decibels::SILENCE);
    }

    #[test]
    fn headless_playback_starts_and_stops() {
        let mut audio = Audio::headless();
        assert!(!audio.is_available());
        let mut handle = audio.play_with(
            &thud(),
            Playback {
                looping: true,
                volume: 0.5,
                ..Playback::default()
            },
        );
        assert!(handle.is_playing());
        handle.set_volume(0.2);
        handle.stop();
        audio.set_master_volume(0.8);
    }

    #[test]
    fn disabled_audio_is_a_silent_no_op() {
        let mut audio = Audio {
            output: Output::Disabled,
        };
        let mut handle = audio.play(&thud());
        assert!(!handle.is_playing());
        handle.stop();
        handle.set_volume(1.0);
    }
}
