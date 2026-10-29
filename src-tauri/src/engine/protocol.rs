//! Messages between the app and the engine worker, and how they travel over its stdin/stdout.
//!
//! A frame is a fixed [`HEADER_LEN`]-byte header followed by one body:
//!
//! | bytes | field                                 |
//! |-------|---------------------------------------|
//! | 0..2  | magic `BJ`                            |
//! | 2     | protocol version ([`VERSION`])        |
//! | 3     | reserved, always 0                    |
//! | 4..8  | JSON byte count (`u32`, LE)           |
//! | 8..12 | audio sample count (`u32`, LE)        |
//!
//! The body is the JSON message, then the audio as little-endian `f32` (16 kHz mono). Both
//! counts are checked against hard caps before anything is allocated, so a corrupt or hostile
//! header can't make either side reserve unbounded memory.

use std::io::{self, ErrorKind, Read, Write};
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::native::Arch;

const MAGIC: &[u8; 2] = b"BJ";
pub const VERSION: u8 = 1;
const HEADER_LEN: usize = 12;
/// JSON messages are small; transcripts of an hour of speech stay far below this.
const JSON_CAP: usize = 4 << 20;
/// 70 min of 16 kHz audio, above the 60 min session limit.
const AUDIO_CAP: usize = 70 * 60 * 16_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    /// Load a speech model (and the VAD model), replacing any loaded one.
    Load {
        model: PathBuf,
        arch: ModelArch,
        vad: PathBuf,
    },
    /// Run silence through the model so the first real dictation doesn't pay for GPU kernel /
    /// shader compilation.
    WarmUp,
    /// Transcribe the attached audio. VAD runs first.
    Run {
        job: u64,
        language: Option<String>,
        vocabulary: Vec<String>,
    },
    /// Abort job `job` if it is the one running. Handled by the reader thread, not queued.
    Cancel { job: u64 },
}

/// Which engine a model file needs; serialized form of [`Arch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelArch {
    Whisper,
    Parakeet,
}

impl From<ModelArch> for Arch {
    fn from(a: ModelArch) -> Self {
        match a {
            ModelArch::Whisper => Arch::Whisper,
            ModelArch::Parakeet => Arch::Parakeet,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Response {
    /// Sent once at startup, after the backends loaded.
    Ready {
        devices: Vec<String>,
    },
    Loaded(LoadedInfo),
    WarmedUp {
        ms: u64,
    },
    Transcript(TranscriptMsg),
    NoSpeech {
        job: u64,
    },
    Aborted {
        job: u64,
    },
    Error {
        job: Option<u64>,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadedInfo {
    pub arch: String,
    pub backend: String,
    pub device: String,
    pub on_gpu: bool,
    pub supports_vocabulary: bool,
    pub load_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptMsg {
    pub job: u64,
    pub text: String,
    pub language: Option<String>,
    pub vad_ms: u64,
    pub transcribe_ms: u64,
    /// Speech kept after VAD trimming, in ms.
    pub speech_ms: u64,
}

/// One received frame.
#[derive(Debug)]
pub struct Frame<T> {
    pub message: T,
    pub audio: Vec<f32>,
}

fn invalid(what: impl Into<String>) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, what.into())
}

/// Encodes `message` and `audio` as one frame and writes it with a single call.
pub fn send_frame(out: &mut impl Write, message: &impl Serialize, audio: &[f32]) -> io::Result<()> {
    let mut frame = vec![0u8; HEADER_LEN];
    serde_json::to_writer(&mut frame, message).map_err(|e| invalid(e.to_string()))?;
    let json_len = frame.len() - HEADER_LEN;
    if json_len > JSON_CAP || audio.len() > AUDIO_CAP {
        return Err(invalid("engine message over the size cap"));
    }
    frame[..2].copy_from_slice(MAGIC);
    frame[2] = VERSION;
    frame[4..8].copy_from_slice(&(json_len as u32).to_le_bytes());
    frame[8..12].copy_from_slice(&(audio.len() as u32).to_le_bytes());
    frame.reserve(audio.len() * 4);
    frame.extend(audio.iter().flat_map(|s| s.to_le_bytes()));
    out.write_all(&frame)?;
    out.flush()
}

/// Reads the next frame, or `None` when the stream ends cleanly between frames.
pub fn recv_frame<T: DeserializeOwned>(input: &mut impl Read) -> io::Result<Option<Frame<T>>> {
    let mut header = Vec::with_capacity(HEADER_LEN);
    input
        .by_ref()
        .take(HEADER_LEN as u64)
        .read_to_end(&mut header)?;
    match header.len() {
        0 => return Ok(None),
        HEADER_LEN => {}
        n => {
            return Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                format!("stream ended {n} bytes into a frame header"),
            ));
        }
    }
    if &header[..2] != MAGIC {
        return Err(invalid("not an engine frame"));
    }
    if header[2] != VERSION {
        return Err(invalid(format!(
            "engine protocol v{} (this build speaks v{VERSION})",
            header[2]
        )));
    }
    let count = |at: usize| {
        u32::from_le_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]]) as usize
    };
    let (json_len, samples) = (count(4), count(8));
    if json_len > JSON_CAP || samples > AUDIO_CAP {
        return Err(invalid("engine frame over the size cap"));
    }
    let mut body = vec![0u8; json_len + samples * 4];
    input.read_exact(&mut body)?;
    let (json, audio) = body.split_at(json_len);
    let message = serde_json::from_slice(json).map_err(|e| invalid(e.to_string()))?;
    let audio = audio
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&bytes| f32::from_le_bytes(bytes))
        .collect();
    Ok(Some(Frame { message, audio }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(message: &impl Serialize, audio: &[f32]) -> Vec<u8> {
        let mut bytes = Vec::new();
        send_frame(&mut bytes, message, audio).unwrap();
        bytes
    }

    #[test]
    fn a_run_request_arrives_with_its_audio() {
        let request = Request::Run {
            job: 42,
            language: Some("es".into()),
            vocabulary: vec!["Neuquén".into(), "Kubernetes".into()],
        };
        let audio = [0.25f32, -0.75, 1.0, 0.0, -1.0];
        let bytes = encode(&request, &audio);
        assert_eq!(&bytes[..4], &[b'B', b'J', VERSION, 0]);
        let frame: Frame<Request> = recv_frame(&mut bytes.as_slice()).unwrap().unwrap();
        assert_eq!(frame.message, request);
        assert_eq!(frame.audio, audio);
    }

    #[test]
    fn load_names_the_engine() {
        let request = Request::Load {
            model: "models/a.bin".into(),
            arch: ModelArch::Parakeet,
            vad: "vad.bin".into(),
        };
        let bytes = encode(&request, &[]);
        let frame: Frame<Request> = recv_frame(&mut bytes.as_slice()).unwrap().unwrap();
        assert_eq!(frame.message, request);
        assert!(frame.audio.is_empty());
    }

    #[test]
    fn frames_follow_each_other_until_a_clean_end() {
        let mut bytes = encode(&Response::Aborted { job: 3 }, &[]);
        bytes.extend(encode(&Response::WarmedUp { ms: 90 }, &[0.5]));
        let mut stream = bytes.as_slice();
        let first: Frame<Response> = recv_frame(&mut stream).unwrap().unwrap();
        let second: Frame<Response> = recv_frame(&mut stream).unwrap().unwrap();
        assert_eq!(first.message, Response::Aborted { job: 3 });
        assert_eq!(
            (second.message, second.audio),
            (Response::WarmedUp { ms: 90 }, vec![0.5])
        );
        assert!(recv_frame::<Response>(&mut stream).unwrap().is_none());
    }

    #[test]
    fn a_cut_frame_is_an_error_not_an_end() {
        let bytes = encode(&Response::NoSpeech { job: 9 }, &[0.1, 0.2]);
        for cut in [3, HEADER_LEN, bytes.len() - 1] {
            assert!(
                recv_frame::<Response>(&mut &bytes[..cut]).is_err(),
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn foreign_bytes_and_other_versions_are_refused() {
        let bytes = encode(&Response::NoSpeech { job: 9 }, &[]);
        let mut garbage = bytes.clone();
        garbage[1] = b'X';
        assert!(recv_frame::<Response>(&mut garbage.as_slice()).is_err());
        let mut newer = bytes;
        newer[2] = VERSION + 1;
        assert!(recv_frame::<Response>(&mut newer.as_slice()).is_err());
    }

    #[test]
    fn huge_counts_fail_before_allocating() {
        let header = |json: u32, samples: u32| {
            let mut h = vec![b'B', b'J', VERSION, 0];
            h.extend(json.to_le_bytes());
            h.extend(samples.to_le_bytes());
            h
        };
        assert!(recv_frame::<Response>(&mut header(u32::MAX, 0).as_slice()).is_err());
        assert!(recv_frame::<Response>(&mut header(2, u32::MAX).as_slice()).is_err());
    }
}
