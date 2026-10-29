//! Signature sounds: synthesis (`synth`) and playback (`player`).

pub mod player;
pub mod synth;

pub use player::SoundPlayer;
pub use synth::SoundKind;
