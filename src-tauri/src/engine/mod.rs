//! Local speech engine: whisper.cpp (Whisper + Parakeet) in a worker subprocess.

pub mod host;
pub mod models;
pub mod native;
pub mod protocol;
pub mod worker;

pub use host::{EngineConfig, EngineError, EngineHost, EngineState, EngineStatus, Outcome};
