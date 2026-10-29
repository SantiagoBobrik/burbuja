//! Model catalog and downloads.
//!
//! ggml model files from the whisper.cpp project's Hugging Face repos, pinned to a commit and a
//! SHA-256 (the repo's LFS oid on 2026-10-29), so a later upload can never swap a model under us.
//! Downloads go to `<file>.part`, resume with HTTP Range, are cancelable, are verified against
//! the pinned hash and are renamed into place atomically.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};
use specta::Type;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub use super::protocol::ModelArch;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ModelRole {
    Asr,
    Vad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelDef {
    pub id: &'static str,
    pub name: &'static str,
    pub role: ModelRole,
    /// The engine that runs the file.
    pub arch: ModelArch,
    pub repo: &'static str,
    pub revision: &'static str,
    pub file: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    /// Transcribes more than English. English-only models get the language hint `en`.
    pub multilingual: bool,
    pub license: &'static str,
}

/// Official repositories only; see [`OFFICIAL_SOURCES`].
pub const CATALOG: &[ModelDef] = &[
    ModelDef {
        id: "whisper-large-v3-turbo",
        name: "Whisper large-v3-turbo",
        role: ModelRole::Asr,
        arch: ModelArch::Whisper,
        repo: "ggerganov/whisper.cpp",
        revision: "5359861c739e955e79d9a303bcbc70fb988958b1",
        file: "ggml-large-v3-turbo-q5_0.bin",
        size: 574_041_195,
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        multilingual: true,
        license: "MIT",
    },
    ModelDef {
        id: "parakeet-tdt-0.6b-v3",
        name: "Parakeet TDT 0.6B v3",
        role: ModelRole::Asr,
        arch: ModelArch::Parakeet,
        repo: "ggml-org/parakeet-GGUF",
        revision: "35156454d1a39de06863303dd209fd2bed6ee079",
        file: "ggml-parakeet-tdt-0.6b-v3-q4_k.bin",
        size: 415_611_879,
        sha256: "8b205b8b39c6535e153de6fb11c51db46125d45c4f16ba496fe41a0fe71b885e",
        multilingual: true,
        license: "CC-BY-4.0",
    },
    ModelDef {
        id: "whisper-base.en",
        name: "Whisper base.en",
        role: ModelRole::Asr,
        arch: ModelArch::Whisper,
        repo: "ggerganov/whisper.cpp",
        revision: "5359861c739e955e79d9a303bcbc70fb988958b1",
        file: "ggml-base.en-q5_1.bin",
        size: 59_721_011,
        sha256: "4baf70dd0d7c4247ba2b81fafd9c01005ac77c2f9ef064e00dcf195d0e2fdd2f",
        multilingual: false,
        license: "MIT",
    },
];

/// Silero VAD ships inside the app, so it is never downloaded.
pub const SILERO_VAD: ModelDef = ModelDef {
    id: "silero-vad-v6.2.0",
    name: "Silero VAD v6.2.0",
    role: ModelRole::Vad,
    arch: ModelArch::Whisper,
    repo: "ggml-org/whisper-vad",
    revision: "9ffd54a1e1ee413ddf265af9913beaf518d1639b",
    file: "ggml-silero-v6.2.0.bin",
    size: 885_098,
    sha256: "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987",
    multilingual: true,
    license: "MIT",
};

/// Hugging Face accounts models may come from: the whisper.cpp project's own.
pub const OFFICIAL_SOURCES: &[&str] = &["ggml-org/", "ggerganov/"];

pub fn find(id: &str) -> Option<&'static ModelDef> {
    CATALOG.iter().find(|m| m.id == id)
}

impl ModelDef {
    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repo, self.revision, self.file
        )
    }

    pub fn path_in(&self, dir: &Path) -> PathBuf {
        dir.join(self.file)
    }

    /// Where an unfinished download lives until it is verified.
    pub fn part_path_in(&self, dir: &Path) -> PathBuf {
        dir.join(format!("{}.part", self.file))
    }

    pub fn is_downloaded(&self, dir: &Path) -> bool {
        std::fs::metadata(self.path_in(dir)).is_ok_and(|m| m.len() == self.size)
    }

    /// Deletes the model and any unfinished download of it from `dir`.
    pub fn remove_from(&self, dir: &Path) -> std::io::Result<()> {
        for path in [self.path_in(dir), self.part_path_in(dir)] {
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
        }
        Ok(())
    }
}

/// The model downloads in flight, each with its cancel flag. [`Downloads::begin`] registers a
/// download; it stays registered until its [`DownloadTicket`] is dropped.
#[derive(Debug, Clone, Default)]
pub struct Downloads(Arc<Mutex<HashMap<&'static str, Arc<AtomicBool>>>>);

/// One registered download. Dropping it unregisters the download.
#[derive(Debug)]
pub struct DownloadTicket {
    id: &'static str,
    cancel: Arc<AtomicBool>,
    downloads: Downloads,
}

impl Downloads {
    /// Registers a download of `id`, or `None` if one is already running.
    pub fn begin(&self, id: &'static str) -> Option<DownloadTicket> {
        let mut map = self.0.lock().expect("downloads");
        if map.contains_key(id) {
            return None;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        map.insert(id, Arc::clone(&cancel));
        Some(DownloadTicket {
            id,
            cancel,
            downloads: self.clone(),
        })
    }

    pub fn is_active(&self, id: &str) -> bool {
        self.0.lock().expect("downloads").contains_key(id)
    }

    /// Asks the download of `id` to stop, if one is running.
    pub fn cancel(&self, id: &str) {
        if let Some(flag) = self.0.lock().expect("downloads").get(id) {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

impl DownloadTicket {
    /// The flag [`download`] polls.
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }
}

impl Drop for DownloadTicket {
    fn drop(&mut self) {
        if let Ok(mut map) = self.downloads.0.lock() {
            map.remove(self.id);
        }
    }
}

/// How long a download may go without receiving a byte before it fails (a stalled connection
/// would otherwise hang forever).
const STALL_LIMIT: Duration = Duration::from_secs(60);
/// How often the cancel flag is checked while waiting for data.
const CANCEL_POLL: Duration = Duration::from_millis(250);

/// The HTTP client for model downloads: HTTPS only, redirects included (Hugging Face redirects
/// to its CDN), so a redirect can never downgrade to plain HTTP. Integrity comes from the
/// pinned SHA-256 either way; this keeps which model a user fetches private on the wire.
pub fn download_client() -> Result<reqwest::Client, DownloadError> {
    reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(5))
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| DownloadError::Network(e.without_url().to_string()))
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("download cancelled")]
    Cancelled,
    #[error("network: {0}")]
    Network(String),
    #[error("server returned {0}")]
    Status(u16),
    #[error("checksum mismatch")]
    Checksum,
    #[error("disk: {0}")]
    Io(#[from] std::io::Error),
}

/// Downloads `def` into `dir` and returns the final path. `progress(bytes, total)` is called
/// as data arrives; set `cancel` to stop (the `.part` file stays for a later resume).
pub async fn download(
    client: &reqwest::Client,
    def: &ModelDef,
    url: &str,
    dir: &Path,
    cancel: Arc<AtomicBool>,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf, DownloadError> {
    let final_path = def.path_in(dir);
    if def.is_downloaded(dir) {
        return Ok(final_path);
    }
    tokio::fs::create_dir_all(dir).await?;
    let part = def.part_path_in(dir);

    // Hash whatever is already on disk so a resumed download is verified end to end.
    let mut hasher = Sha256::new();
    let mut have = 0u64;
    if let Ok(mut f) = tokio::fs::File::open(&part).await {
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = f.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            have += n as u64;
        }
    }
    if have > def.size {
        tokio::fs::remove_file(&part).await?;
        have = 0;
        hasher = Sha256::new();
    }

    let mut request = client.get(url);
    if have > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let mut response = tokio::time::timeout(STALL_LIMIT, request.send())
        .await
        .map_err(|_| DownloadError::Network("the server did not answer".into()))?
        .map_err(|e| DownloadError::Network(e.without_url().to_string()))?;
    let status = response.status().as_u16();
    let mut file = match status {
        206 => {
            tokio::fs::OpenOptions::new()
                .append(true)
                .open(&part)
                .await?
        }
        200 => {
            // The server ignored the range: start over.
            have = 0;
            hasher = Sha256::new();
            tokio::fs::File::create(&part).await?
        }
        _ => return Err(DownloadError::Status(status)),
    };
    progress(have, def.size);

    let mut last_data = Instant::now();
    loop {
        if cancel.load(Ordering::SeqCst) {
            file.flush().await?;
            return Err(DownloadError::Cancelled);
        }
        // Wait in short slices so a cancel or a stalled server is noticed without new data.
        let chunk = match tokio::time::timeout(CANCEL_POLL, response.chunk()).await {
            Err(_) if last_data.elapsed() >= STALL_LIMIT => {
                file.flush().await?;
                return Err(DownloadError::Network("the download stalled".into()));
            }
            Err(_) => continue,
            Ok(r) => r.map_err(|e| DownloadError::Network(e.without_url().to_string()))?,
        };
        let Some(chunk) = chunk else { break };
        last_data = Instant::now();
        // Never write more than the pinned size: a misbehaving server can't fill the disk.
        if have + chunk.len() as u64 > def.size {
            drop(file);
            let _ = tokio::fs::remove_file(&part).await;
            return Err(DownloadError::Checksum);
        }
        file.write_all(&chunk).await?;
        hasher.update(&chunk);
        have += chunk.len() as u64;
        progress(have, def.size);
    }
    file.flush().await?;
    drop(file);

    let digest: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if have != def.size || digest != def.sha256 {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(DownloadError::Checksum);
    }
    tokio::fs::rename(&part, &final_path).await?;
    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncBufReadExt;
    use tokio::net::TcpListener;

    const BODY: &[u8] = b"burbuja model bytes 0123456789";

    fn def() -> ModelDef {
        let digest: String = Sha256::digest(BODY)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        ModelDef {
            id: "test",
            name: "Test",
            role: ModelRole::Asr,
            arch: ModelArch::Whisper,
            repo: "x/y",
            revision: "r",
            file: "test.gguf",
            size: BODY.len() as u64,
            sha256: Box::leak(digest.into_boxed_str()),
            multilingual: false,
            license: "MIT",
        }
    }

    /// Minimal HTTP/1.1 server that honors `Range: bytes=N-` and serves `body`.
    async fn serve(body: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let (r, mut w) = sock.split();
                    let mut lines = tokio::io::BufReader::new(r).lines();
                    let mut start = 0usize;
                    while let Ok(Some(line)) = lines.next_line().await {
                        if line.is_empty() {
                            break;
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                            start = v.trim_end_matches('-').parse().unwrap_or(0);
                        }
                    }
                    let part = &body[start..];
                    let head = if start > 0 {
                        format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            part.len()
                        )
                    } else {
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            part.len()
                        )
                    };
                    let _ = w.write_all(head.as_bytes()).await;
                    let _ = w.write_all(part).await;
                });
            }
        });
        format!("http://{addr}/model")
    }

    fn tmp() -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("burbuja-dl-{}", std::process::id()))
            .join(format!("{:?}", std::thread::current().id()).replace(['(', ')'], ""));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[tokio::test]
    async fn downloads_verifies_and_renames() {
        let url = serve(BODY).await;
        let dir = tmp();
        let def = def();
        let mut last = (0, 0);
        let path = download(
            &reqwest::Client::new(),
            &def,
            &url,
            &dir,
            Arc::default(),
            |b, t| last = (b, t),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), BODY);
        assert_eq!(last, (BODY.len() as u64, BODY.len() as u64));
        assert!(!dir.join("test.gguf.part").exists());
        assert!(def.is_downloaded(&dir));
    }

    #[tokio::test]
    async fn resumes_from_a_partial_file() {
        let url = serve(BODY).await;
        let dir = tmp();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("test.gguf.part"), &BODY[..10]).unwrap();
        let path = download(
            &reqwest::Client::new(),
            &def(),
            &url,
            &dir,
            Arc::default(),
            |_, _| {},
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), BODY);
    }

    #[tokio::test]
    async fn bad_checksum_is_rejected_and_part_removed() {
        let url = serve(b"tampered model bytes 012345678").await;
        let dir = tmp();
        let err = download(
            &reqwest::Client::new(),
            &def(),
            &url,
            &dir,
            Arc::default(),
            |_, _| {},
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DownloadError::Checksum));
        assert!(!dir.join("test.gguf.part").exists());
        assert!(!dir.join("test.gguf").exists());
    }

    #[tokio::test]
    async fn a_body_longer_than_the_pinned_size_is_cut_off() {
        let url = serve(b"burbuja model bytes 0123456789 and then a lot more bytes").await;
        let dir = tmp();
        let err = download(
            &reqwest::Client::new(),
            &def(),
            &url,
            &dir,
            Arc::default(),
            |_, _| {},
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DownloadError::Checksum));
        assert!(!dir.join("test.gguf.part").exists());
        assert!(!dir.join("test.gguf").exists());
    }

    #[tokio::test]
    async fn the_download_client_refuses_plain_http() {
        let url = serve(BODY).await;
        let dir = tmp();
        let err = download(
            &download_client().unwrap(),
            &def(),
            &url,
            &dir,
            Arc::default(),
            |_, _| {},
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DownloadError::Network(_)), "{err:?}");
        assert!(!dir.join("test.gguf").exists());
    }

    #[tokio::test]
    async fn cancel_keeps_the_part_file() {
        let url = serve(BODY).await;
        let dir = tmp();
        let cancel = Arc::new(AtomicBool::new(true));
        let err = download(
            &reqwest::Client::new(),
            &def(),
            &url,
            &dir,
            cancel,
            |_, _| {},
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DownloadError::Cancelled));
        assert!(dir.join("test.gguf.part").exists());
        def().remove_from(&dir).unwrap();
        assert!(!dir.join("test.gguf.part").exists());
    }

    #[test]
    fn a_download_is_registered_until_its_ticket_drops() {
        let downloads = Downloads::default();
        let ticket = downloads.begin("test").expect("first download");
        assert!(downloads.is_active("test"));
        assert!(downloads.begin("test").is_none(), "one download per model");
        downloads.cancel("test");
        assert!(ticket.cancel_flag().load(Ordering::SeqCst));
        drop(ticket);
        assert!(!downloads.is_active("test"));
        let again = downloads.begin("test").expect("can start again");
        assert!(!again.cancel_flag().load(Ordering::SeqCst));
    }

    #[test]
    fn catalog_is_pinned() {
        for m in CATALOG.iter().chain([&SILERO_VAD]) {
            assert_eq!(m.sha256.len(), 64, "{}", m.id);
            assert_eq!(m.revision.len(), 40, "{}", m.id);
            assert!(
                OFFICIAL_SOURCES.iter().any(|s| m.repo.starts_with(s)),
                "{} comes from {}",
                m.id,
                m.repo
            );
            assert!(m.url().starts_with("https://huggingface.co/"));
        }
        assert_eq!(find("whisper-large-v3-turbo").unwrap().size, 574_041_195);
    }
}
