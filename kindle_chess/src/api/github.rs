// GitHub Releases client. Used by the in-app updater to learn whether a newer
// binary has been published and where to fetch it from.
//
// We hit the public, anonymous endpoint:
//   GET https://api.github.com/repos/<owner>/<repo>/releases/latest
// which is rate-limited to 60 req/h per IP — far more than the app will ever
// need, since checks are user-initiated.
//
// The release workflow uploads two assets per tag:
//   - {ASSET_NAME}         — the cross-compiled armv7-musl binary
//   - {ASSET_NAME}.sha256  — text file, first whitespace-separated token is
//                            the lowercase hex sha256 of the binary
// These names are the contract between this client and .github/workflows/release.yml.

use log::{info, warn};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use semver::Version;
use serde::Deserialize;

use crate::version;

pub const OWNER: &str = "mxyyz";
pub const REPO: &str = "kindle-chess";
pub const ASSET_NAME: &str = "kindle-chess-armv7-musl";
pub const SHA_NAME: &str = "kindle-chess-armv7-musl.sha256";

#[derive(Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

/// Resolved info for a release that is strictly newer than the running binary.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub current: Version,
    pub latest: Version,
    pub release_name: String,
    pub release_notes: String,
    pub asset_url: String,
    pub sha256_url: String,
    pub asset_size: u64,
}

/// Hits `releases/latest` and returns the parsed JSON.
pub async fn fetch_latest_release() -> Result<Release, Box<dyn std::error::Error>> {
    let url = format!("https://api.github.com/repos/{}/{}/releases/latest", OWNER, REPO);

    let mut headers = HeaderMap::new();
    // GitHub rejects requests without a User-Agent.
    let ua = format!("kindle-hello/{}", version::VERSION);
    headers.insert(USER_AGENT, HeaderValue::from_str(&ua)?);
    headers.insert(ACCEPT, HeaderValue::from_static("application/vnd.github+json"));

    let client = reqwest::Client::builder().default_headers(headers).build()?;

    info!("Fetching latest release from {}", url);
    let response = client.get(&url).send().await?;
    if !response.status().is_success() {
        return Err(format!("GitHub API returned {}", response.status()).into());
    }
    let release: Release = response.json().await?;
    info!("Latest release: {}", release.tag_name);
    Ok(release)
}

/// `v0.2.0` / `0.2.0` / `release-0.2.0` → `Version("0.2.0")`. Anything else → None.
pub fn parse_tag_version(tag: &str) -> Option<Version> {
    let trimmed = tag.trim_start_matches('v').trim_start_matches("release-");
    Version::parse(trimmed).ok()
}

/// Returns `Ok(Some(_))` if the latest release is strictly newer AND ships
/// both expected assets, `Ok(None)` if the running binary is already at or
/// past the latest tag, and `Err(_)` for everything in between (unparseable
/// tag, release published but assets not yet uploaded — common while
/// `release.yml` is still cross-building — etc.). The UI shows `Ok(None)`
/// as "up to date" and `Err(_)` as "Check failed: <reason>", so we want the
/// "release exists but isn't yet usable" cases to land in the latter.
pub async fn check_for_update() -> Result<Option<UpdateInfo>, Box<dyn std::error::Error>> {
    let release = fetch_latest_release().await?;

    let latest = match parse_tag_version(&release.tag_name) {
        Some(v) => v,
        None => {
            warn!("Unparseable release tag: {}", release.tag_name);
            return Err(format!("unparseable release tag: {}", release.tag_name).into());
        }
    };

    let current = version::current();
    if latest <= current {
        info!("Up to date (current={}, latest={})", current, latest);
        return Ok(None);
    }

    let asset = release.assets.iter().find(|a| a.name == ASSET_NAME);
    let sha = release.assets.iter().find(|a| a.name == SHA_NAME);
    let (asset, sha) = match (asset, sha) {
        (Some(a), Some(s)) => (a, s),
        _ => {
            // Most likely cause: release.yml is still running and hasn't
            // uploaded the binary yet. Surfacing as an error gets the user a
            // "try again in a minute" message instead of a misleading
            // "you're up to date".
            warn!(
                "Release {} is missing required assets ({} and/or {})",
                release.tag_name, ASSET_NAME, SHA_NAME
            );
            return Err(format!(
                "release {} is missing assets — build may still be in progress",
                release.tag_name
            )
            .into());
        }
    };

    Ok(Some(UpdateInfo {
        current,
        latest,
        release_name: release.name.unwrap_or_else(|| release.tag_name.clone()),
        release_notes: release.body.unwrap_or_default(),
        asset_url: asset.browser_download_url.clone(),
        sha256_url: sha.browser_download_url.clone(),
        asset_size: asset.size,
    }))
}
