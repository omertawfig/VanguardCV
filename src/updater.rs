//! GitHub Releases auto-updater for VanguardCV (Windows installer channel).
//!
//! - Queries the official GitHub Releases API for the latest published release.
//! - Compares semantic versions strictly (`v0.0.1` < `v0.0.2`).
//! - Downloads the Windows installer (`VanguardCV-Setup-*-x64.exe`) into an
//!   isolated temp directory with atomic `.part` → final rename.
//! - Verifies Windows PE magic (`MZ`) and cryptographic SHA-256 digest against
//!   the release's `SHA256SUMS.txt` asset before allowing installation.
//! - Launches the verified Inno Setup installer cleanly with `/SILENT` flags so
//!   it replaces the app and relaunches the updated version automatically.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use futures::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

pub const GITHUB_REPO_OWNER: &str = "omertawfig";
pub const GITHUB_REPO_NAME: &str = "VanguardCV";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/omertawfig/VanguardCV/releases/latest";
pub const RELEASES_PAGE_URL: &str = "https://github.com/omertawfig/VanguardCV/releases";

const UPDATER_USER_AGENT: &str = concat!("VanguardCV/", env!("CARGO_PKG_VERSION"));
const MAX_INSTALLER_BYTES: u64 = 512 * 1024 * 1024; // 512 MiB sanity ceiling

/// Semantic version (`major.minor.patch` with optional pre-release tag).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl SemVer {
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim().trim_start_matches(['v', 'V']);
        if trimmed.is_empty() {
            return None;
        }
        // Strip build metadata (`+...`) if present.
        let without_build = trimmed.split('+').next().unwrap_or(trimmed);
        let (core, pre) = match without_build.split_once('-') {
            Some((c, p)) if !p.trim().is_empty() => (c, Some(p.trim().to_string())),
            Some((c, _)) => (c, None),
            None => (without_build, None),
        };
        let mut parts = core.split('.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
        let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
            .then_with(|| match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                // Stable release is newer than pre-release of the same core version.
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Returns `true` when `latest` is strictly newer than `current`.
pub fn is_newer_version(current: &str, latest: &str) -> bool {
    match (SemVer::parse(current), SemVer::parse(latest)) {
        (Some(cur), Some(lat)) => lat > cur,
        _ => false,
    }
}

/// Metadata for a newer release available on GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateReleaseInfo {
    pub version: String,
    pub tag_name: String,
    pub release_notes: String,
    pub html_url: String,
    pub installer_name: String,
    pub installer_download_url: String,
    pub installer_size_bytes: u64,
    pub checksums_url: Option<String>,
}

/// Result of checking GitHub Releases for a newer version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCheckOutcome {
    UpToDate { current_version: String },
    UpdateAvailable(UpdateReleaseInfo),
}

/// Verified installer ready on disk to execute and replace the current build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadedUpdate {
    pub version: String,
    pub installer_path: PathBuf,
    pub sha256_hex: String,
    pub size_bytes: u64,
    pub checksum_verified: bool,
}

/// High-level UI state machine for the Settings update card.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    ReadyToInstall,
    Installing,
    Error(String),
}

#[derive(Debug, Default, Deserialize)]
struct GithubReleaseWire {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAssetWire>,
}

#[derive(Debug, Default, Deserialize)]
struct GithubAssetWire {
    #[serde(default)]
    name: String,
    #[serde(default)]
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

fn build_http_client(timeout_secs: u64) -> Result<Client> {
    Client::builder()
        .user_agent(UPDATER_USER_AGENT)
        .connect_timeout(Duration::from_secs(12))
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .context("failed to initialize updater HTTP client")
}

fn select_windows_installer(assets: &[GithubAssetWire]) -> Option<&GithubAssetWire> {
    // Prefer exact `VanguardCV-Setup-*-x64.exe` installer naming first.
    if let Some(exact) = assets.iter().find(|a| {
        let lower = a.name.to_ascii_lowercase();
        lower.ends_with(".exe")
            && lower.contains("vanguardcv")
            && lower.contains("setup")
            && !a.browser_download_url.trim().is_empty()
    }) {
        return Some(exact);
    }
    // Fallback to any `.exe` installer asset.
    assets.iter().find(|a| {
        a.name.to_ascii_lowercase().ends_with(".exe") && !a.browser_download_url.trim().is_empty()
    })
}

fn select_checksums_asset(assets: &[GithubAssetWire]) -> Option<&GithubAssetWire> {
    assets.iter().find(|a| {
        let lower = a.name.to_ascii_lowercase();
        (lower == "sha256sums.txt" || lower.contains("sha256"))
            && !a.browser_download_url.trim().is_empty()
    })
}

/// Checks the configured GitHub Releases endpoint for a newer stable Windows release.
pub async fn check_for_updates(
    api_url: &str,
    current_version: &str,
) -> Result<UpdateCheckOutcome> {
    let http = build_http_client(20)?;
    let resp = http
        .get(api_url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|err| {
            if err.is_timeout() {
                anyhow!("Update check timed out. Check your internet connection and try again.")
            } else if err.is_connect() {
                anyhow!("Could not reach GitHub to check for updates.")
            } else {
                anyhow!("Update check failed: {err}")
            }
        })?;

    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        // Repository has no published release yet — treat as up to date cleanly.
        return Ok(UpdateCheckOutcome::UpToDate {
            current_version: current_version.to_string(),
        });
    }
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS
    {
        bail!("GitHub API rate limit reached while checking for updates. Try again later.");
    }
    if !status.is_success() {
        bail!("Update server returned HTTP {}.", status.as_u16());
    }

    let wire: GithubReleaseWire = resp
        .json()
        .await
        .context("failed to parse GitHub release metadata")?;

    if wire.draft || wire.prerelease {
        return Ok(UpdateCheckOutcome::UpToDate {
            current_version: current_version.to_string(),
        });
    }

    let latest_version = wire
        .tag_name
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_string();
    if !is_newer_version(current_version, &latest_version) {
        return Ok(UpdateCheckOutcome::UpToDate {
            current_version: current_version.to_string(),
        });
    }

    let installer = select_windows_installer(&wire.assets).ok_or_else(|| {
        anyhow!(
            "Release {} is published on GitHub, but no Windows installer (.exe) asset was attached yet.",
            wire.tag_name
        )
    })?;

    let checksums_url = select_checksums_asset(&wire.assets)
        .map(|a| a.browser_download_url.trim().to_string())
        .filter(|u| !u.is_empty());

    let release_notes = wire
        .body
        .unwrap_or_default()
        .trim()
        .to_string();

    let html_url = wire
        .html_url
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| RELEASES_PAGE_URL.to_string());

    Ok(UpdateCheckOutcome::UpdateAvailable(UpdateReleaseInfo {
        version: latest_version,
        tag_name: wire.tag_name,
        release_notes,
        html_url,
        installer_name: installer.name.clone(),
        installer_download_url: installer.browser_download_url.clone(),
        installer_size_bytes: installer.size,
        checksums_url,
    }))
}

/// Parses a standard `SHA256SUMS.txt` (`<hex>  <filename>` or `<hex> *<filename>`)
/// and returns the expected lowercase SHA-256 hex digest for `target_filename`.
pub fn parse_sha256_for_asset(checksums_text: &str, target_filename: &str) -> Option<String> {
    let target_lower = target_filename.trim().to_ascii_lowercase();
    for line in checksums_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(hash) = parts.next() else {
            continue;
        };
        let Some(raw_name) = parts.next() else {
            continue;
        };
        let clean_name = raw_name
            .trim_start_matches('*')
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(raw_name)
            .to_ascii_lowercase();
        if clean_name == target_lower
            && hash.len() == 64
            && hash.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

/// Default isolated staging folder for downloaded installers.
pub fn default_update_staging_dir() -> PathBuf {
    std::env::temp_dir().join("vanguard-cv-updates")
}

/// Sanitizes an asset filename to a safe basename without path traversal.
fn safe_installer_filename(name: &str, version: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect::<String>();
    if base.to_ascii_lowercase().ends_with(".exe") && base.len() >= 5 {
        base
    } else {
        format!("VanguardCV-Setup-{version}-x64.exe")
    }
}

/// Downloads the Windows installer to `target_dir`, checks the PE `MZ` header,
/// and verifies SHA-256 against `SHA256SUMS.txt` when present in the release.
pub async fn download_and_verify_installer(
    release: &UpdateReleaseInfo,
    target_dir: &Path,
) -> Result<DownloadedUpdate> {
    tokio::fs::create_dir_all(target_dir)
        .await
        .with_context(|| format!("failed to create update staging dir {}", target_dir.display()))?;

    let http = build_http_client(300)?;

    // Fetch expected SHA-256 first (if published with the release).
    let expected_sha256 = if let Some(checksums_url) = release.checksums_url.as_deref() {
        let resp = http
            .get(checksums_url)
            .send()
            .await
            .context("failed to download SHA256SUMS.txt for update verification")?;
        if !resp.status().is_success() {
            bail!(
                "Could not download update checksum file (HTTP {}).",
                resp.status().as_u16()
            );
        }
        let text = resp
            .text()
            .await
            .context("failed to read SHA256SUMS.txt body")?;
        let parsed = parse_sha256_for_asset(&text, &release.installer_name).ok_or_else(|| {
            anyhow!(
                "SHA256SUMS.txt did not contain a valid checksum entry for {}.",
                release.installer_name
            )
        })?;
        Some(parsed)
    } else {
        None
    };

    let filename = safe_installer_filename(&release.installer_name, &release.version);
    let final_path = target_dir.join(&filename);
    let part_path = target_dir.join(format!("{filename}.part"));
    let _ = tokio::fs::remove_file(&part_path).await;

    let resp = http
        .get(&release.installer_download_url)
        .send()
        .await
        .context("failed to download Windows update installer")?;
    if !resp.status().is_success() {
        bail!(
            "Update installer download failed (HTTP {}).",
            resp.status().as_u16()
        );
    }

    let mut file = tokio::fs::File::create(&part_path)
        .await
        .with_context(|| format!("failed to create {}", part_path.display()))?;
    let mut hasher = Sha256::new();
    let mut total_bytes: u64 = 0;
    let mut magic = [0u8; 2];
    let mut magic_len = 0usize;

    let mut stream = resp.bytes_stream();
    while let Some(chunk_res) = stream.next().await {
        let chunk = match chunk_res {
            Ok(c) => c,
            Err(err) => {
                drop(file);
                let _ = tokio::fs::remove_file(&part_path).await;
                return Err(anyhow!("Update download interrupted: {err}"));
            }
        };
        total_bytes = total_bytes.saturating_add(chunk.len() as u64);
        if total_bytes > MAX_INSTALLER_BYTES {
            drop(file);
            let _ = tokio::fs::remove_file(&part_path).await;
            bail!("Update installer exceeded maximum allowed size.");
        }
        if magic_len < 2 {
            let take = (2 - magic_len).min(chunk.len());
            magic[magic_len..magic_len + take].copy_from_slice(&chunk[..take]);
            magic_len += take;
        }
        hasher.update(&chunk);
        if let Err(err) = file.write_all(&chunk).await {
            drop(file);
            let _ = tokio::fs::remove_file(&part_path).await;
            return Err(err).context("failed writing update installer to disk");
        }
    }
    file.flush().await?;
    drop(file);

    if magic_len < 2 || &magic != b"MZ" {
        let _ = tokio::fs::remove_file(&part_path).await;
        bail!("Downloaded file is not a valid Windows executable installer.");
    }

    let computed_sha256 = hex_lower(&hasher.finalize());
    let checksum_verified = if let Some(expected) = expected_sha256 {
        if computed_sha256 != expected {
            let _ = tokio::fs::remove_file(&part_path).await;
            bail!(
                "Update integrity check failed: SHA-256 mismatch (expected {expected}, got {computed_sha256})."
            );
        }
        true
    } else {
        false
    };

    if final_path.exists() {
        let _ = tokio::fs::remove_file(&final_path).await;
    }
    tokio::fs::rename(&part_path, &final_path)
        .await
        .context("failed to finalize downloaded update installer")?;

    Ok(DownloadedUpdate {
        version: release.version.clone(),
        installer_path: final_path,
        sha256_hex: computed_sha256,
        size_bytes: total_bytes,
        checksum_verified,
    })
}

fn hex_lower(bytes: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(LUT[(b >> 4) as usize] as char);
        out.push(LUT[(b & 0x0f) as usize] as char);
    }
    out
}

/// Launches the verified Windows Inno Setup installer in quiet update mode so
/// it replaces the running binary once VanguardCV exits and relaunches the app.
#[cfg(target_os = "windows")]
pub fn launch_windows_installer(installer_path: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    if !installer_path.is_file() {
        bail!(
            "Update installer is missing at {}. Please download it again.",
            installer_path.display()
        );
    }

    // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP so the installer survives
    // VanguardCV exiting immediately after spawn.
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    Command::new(installer_path)
        .args([
            "/SILENT",
            "/CLOSEAPPLICATIONS",
            "/NORESTART",
            "/SP-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .with_context(|| format!("failed to launch installer {}", installer_path.display()))?;

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn launch_windows_installer(_installer_path: &Path) -> Result<()> {
    bail!("In-app installer execution is currently supported on Windows releases only.")
}

/// Formats byte counts cleanly for the Settings update card (e.g. `14.2 MB`).
pub fn format_download_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", (bytes as f64) / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.0} KB", (bytes as f64) / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn semver_ordering_handles_v_prefix_and_prereleases() {
        assert!(is_newer_version("0.0.1", "0.0.2"));
        assert!(is_newer_version("0.0.1", "v0.1.0"));
        assert!(is_newer_version("0.0.1", "v1.0.0"));
        assert!(is_newer_version("0.0.2-beta.1", "0.0.2"));
        assert!(!is_newer_version("0.0.1", "0.0.1"));
        assert!(!is_newer_version("0.0.2", "0.0.1"));
        assert!(!is_newer_version("0.0.2", "0.0.2-rc.1"));
        assert!(!is_newer_version("0.0.1", "not-a-version"));
    }

    #[test]
    fn parses_sha256sums_lines_with_binary_marker_and_paths() {
        let digest = "a".repeat(64);
        let text = format!(
            "# comment\n{digest} *dist/VanguardCV-Setup-0.0.2-x64.exe\n{}  other.zip\n",
            "b".repeat(64)
        );
        assert_eq!(
            parse_sha256_for_asset(&text, "VanguardCV-Setup-0.0.2-x64.exe"),
            Some(digest)
        );
        assert_eq!(parse_sha256_for_asset(&text, "missing.exe"), None);
    }

    #[tokio::test]
    async fn checks_downloads_and_verifies_sha256_against_mock_release_server() {
        let payload = b"MZ\x90\x00FAKE_WINDOWS_INNO_INSTALLER_BYTES_V002".to_vec();
        let expected_sha = hex_lower(&Sha256::digest(&payload));

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let base = format!("http://{addr}");

        let payload_clone = payload.clone();
        let sha_clone = expected_sha.clone();
        let base_clone = base.clone();
        let server = tokio::spawn(async move {
            // Serve 3 requests: /releases/latest, /SHA256SUMS.txt, /VanguardCV-Setup-0.0.2-x64.exe
            for _ in 0..3 {
                let (mut sock, _) = listener.accept().await.expect("accept");
                let mut buf = vec![0u8; 4096];
                let n = sock.read(&mut buf).await.expect("read");
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let first_line = req.lines().next().unwrap_or("");

                if first_line.contains("GET /releases/latest ") {
                    let body = serde_json::json!({
                        "tag_name": "v0.0.2",
                        "body": "## What's New\n- Faster PDF export",
                        "html_url": "https://github.com/omertawfig/VanguardCV/releases/tag/v0.0.2",
                        "draft": false,
                        "prerelease": false,
                        "assets": [
                            {
                                "name": "VanguardCV-Setup-0.0.2-x64.exe",
                                "browser_download_url": format!("{base_clone}/VanguardCV-Setup-0.0.2-x64.exe"),
                                "size": payload_clone.len()
                            },
                            {
                                "name": "SHA256SUMS.txt",
                                "browser_download_url": format!("{base_clone}/SHA256SUMS.txt"),
                                "size": 100
                            }
                        ]
                    })
                    .to_string();
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    sock.write_all(resp.as_bytes()).await.unwrap();
                } else if first_line.contains("GET /SHA256SUMS.txt ") {
                    let body = format!("{sha_clone}  VanguardCV-Setup-0.0.2-x64.exe\n");
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    sock.write_all(resp.as_bytes()).await.unwrap();
                } else if first_line.contains("GET /VanguardCV-Setup-0.0.2-x64.exe ") {
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        payload_clone.len()
                    );
                    sock.write_all(header.as_bytes()).await.unwrap();
                    sock.write_all(&payload_clone).await.unwrap();
                }
            }
        });

        let outcome = check_for_updates(&format!("{base}/releases/latest"), "0.0.1")
            .await
            .expect("check should succeed");
        let release = match outcome {
            UpdateCheckOutcome::UpdateAvailable(info) => info,
            other => panic!("expected UpdateAvailable, got {other:?}"),
        };
        assert_eq!(release.version, "0.0.2");
        assert_eq!(release.installer_name, "VanguardCV-Setup-0.0.2-x64.exe");

        let tmp = tempfile::tempdir().expect("tempdir");
        let downloaded = download_and_verify_installer(&release, tmp.path())
            .await
            .expect("verified download should succeed");
        server.await.expect("server");

        assert!(downloaded.checksum_verified);
        assert_eq!(downloaded.sha256_hex, expected_sha);
        assert_eq!(downloaded.size_bytes, payload.len() as u64);
        assert!(downloaded.installer_path.is_file());
    }

    #[tokio::test]
    async fn rejects_and_deletes_tampered_installer_on_sha256_mismatch() {
        let payload = b"MZ\x90\x00TAMPERED_PAYLOAD".to_vec();
        let wrong_sha = "0".repeat(64);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let base = format!("http://{addr}");

        let payload_clone = payload.clone();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut sock, _) = listener.accept().await.expect("accept");
                let mut buf = vec![0u8; 4096];
                let n = sock.read(&mut buf).await.expect("read");
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let first_line = req.lines().next().unwrap_or("");
                if first_line.contains("SHA256SUMS.txt") {
                    let body = format!("{wrong_sha}  VanguardCV-Setup-0.0.2-x64.exe\n");
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    sock.write_all(resp.as_bytes()).await.unwrap();
                } else {
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        payload_clone.len()
                    );
                    sock.write_all(header.as_bytes()).await.unwrap();
                    sock.write_all(&payload_clone).await.unwrap();
                }
            }
        });

        let release = UpdateReleaseInfo {
            version: "0.0.2".to_string(),
            tag_name: "v0.0.2".to_string(),
            release_notes: String::new(),
            html_url: RELEASES_PAGE_URL.to_string(),
            installer_name: "VanguardCV-Setup-0.0.2-x64.exe".to_string(),
            installer_download_url: format!("{base}/VanguardCV-Setup-0.0.2-x64.exe"),
            installer_size_bytes: payload.len() as u64,
            checksums_url: Some(format!("{base}/SHA256SUMS.txt")),
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let err = download_and_verify_installer(&release, tmp.path())
            .await
            .expect_err("tampered download must fail");
        server.await.expect("server");
        assert!(err.to_string().contains("SHA-256 mismatch"));
        assert!(!tmp.path().join("VanguardCV-Setup-0.0.2-x64.exe").exists());
        assert!(
            !tmp.path()
                .join("VanguardCV-Setup-0.0.2-x64.exe.part")
                .exists()
        );
    }

    #[tokio::test]
    async fn treats_github_404_as_up_to_date_cleanly() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.expect("accept");
            let mut buf = vec![0u8; 2048];
            let _ = sock.read(&mut buf).await;
            let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
            sock.write_all(resp.as_bytes()).await.unwrap();
        });

        let outcome = check_for_updates(&format!("http://{addr}/releases/latest"), "0.0.1")
            .await
            .expect("404 should resolve cleanly");
        server.await.expect("server");
        assert_eq!(
            outcome,
            UpdateCheckOutcome::UpToDate {
                current_version: "0.0.1".to_string()
            }
        );
    }
}
