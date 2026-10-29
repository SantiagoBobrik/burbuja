//! Microphone capture and level metering.

pub mod capture;
pub mod levels;

pub use capture::{Mic, MicError, MicInfo, Recording, list_microphones};
