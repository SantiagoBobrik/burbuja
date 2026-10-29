//! Dictation orchestration. `controller` is the single owner of dictation state; `pipeline`
//! is what a finished recording goes through.

pub mod controller;
pub mod pipeline;

pub use controller::{ControllerHandle, Msg};
