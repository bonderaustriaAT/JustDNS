//! In-app update checking and the request/status protocol the updater sidecar
//! watches.
//!
//! The web process never talks to the Docker daemon itself — giving the
//! internet-facing DNS/API process a socket would be a needless risk. Instead it
//! only ever *reads* GitHub to learn the latest release and *writes* a small
//! request file onto the persistent data volume. A tiny, isolated updater sidecar
//! (see `deploy/updater/`) mounts the Docker socket, watches that file, performs
//! the pull/recreate/remove, and reports progress back through a status file.
//!
//! The source is public GitHub tags: no token, works out of the box on any
//! install whose repository is public. The changelog shown in the UI is the
//! matching section of `CHANGELOG.md` at that tag.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// The repository checked for releases, unless `JUSTDNS_UPDATE_REPO` overrides it.
const DEFAULT_REPO: &str = "bonderaustriaAT/JustDNS";
/// Where the persistent data volume is mounted, unless `JUSTDNS_DATA_DIR`
/// overrides it. The update request/status files live under `<data>/update`.
const DEFAULT_DATA_DIR: &str = "/data";
/// How long to wait on a GitHub request before giving up.
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
/// Hard cap on a fetched body, so a hostile or broken endpoint cannot exhaust
/// memory. Tags JSON and a changelog are both well under this.
const MAX_FETCH_BYTES: u64 = 2 * 1024 * 1024;
/// A heartbeat newer than this means the updater sidecar is alive and watching.
const UPDATER_FRESH: Duration = Duration::from_secs(120);

/// A semantic version `major.minor.patch`. Pre-release and build suffixes are
/// rejected on parse, which is exactly how we filter release tags from the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Semver {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Semver {
    /// Parse `1.2.3` or `v1.2.3`. Returns `None` for anything that is not exactly
    /// three dotted integers (so `v1.2.3-rc1`, `latest`, `dev` are all rejected).
    fn parse(tag: &str) -> Option<Self> {
        let core = tag.strip_prefix('v').unwrap_or(tag);
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

/// The cached result of the most recent check.
#[derive(Clone, Default)]
struct Cached {
    /// The highest release tag seen (e.g. `v0.10.0`), as GitHub spells it.
    latest: Option<String>,
    /// The `CHANGELOG.md` section for `latest`, as Markdown.
    notes: Option<String>,
    /// A link a human can open to read about the release.
    release_url: Option<String>,
    /// When the last check completed, in milliseconds since the Unix epoch.
    checked_at_ms: Option<u64>,
    /// The error from the last check, or `None` when it succeeded.
    error: Option<String>,
}

/// A read-only view of the last check, for the API handler to shape into JSON.
pub struct UpdateCheck {
    pub current: String,
    pub latest: Option<String>,
    pub update_available: bool,
    pub notes: Option<String>,
    pub release_url: Option<String>,
    pub checked_at_ms: Option<u64>,
    pub error: Option<String>,
}

/// Progress reported by the updater sidecar through `<update>/status.json`.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    /// `pulling`, `recreating`, `done` or `failed`.
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The version the sidecar is moving to, so the UI can confirm the target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at_ms: Option<u64>,
}

/// What the web process writes for the sidecar to act on: pull `version` and
/// recreate the JustDNS service. Only a version string crosses this boundary —
/// the sidecar builds the image name from its own fixed repository, so a
/// compromised app cannot make it pull or delete an arbitrary image.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateRequest<'a> {
    version: &'a str,
    requested_by: &'a str,
    requested_at_ms: u64,
}

/// Checks GitHub for newer releases and mediates the on-disk update protocol.
/// Cheap to share behind an `Arc`; the cached result sits behind an `RwLock`.
pub struct UpdateChecker {
    repo: String,
    current_raw: String,
    current: Option<Semver>,
    data_dir: PathBuf,
    cache: RwLock<Cached>,
}

impl UpdateChecker {
    /// Build a checker for the running `version`, taking the repository and data
    /// directory from the environment (with the compiled-in defaults).
    pub fn from_env(version: &str) -> Self {
        let repo = std::env::var("JUSTDNS_UPDATE_REPO")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_REPO.to_owned());
        let data_dir = std::env::var("JUSTDNS_DATA_DIR")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR));
        Self {
            current: Semver::parse(version),
            current_raw: version.to_owned(),
            repo,
            data_dir,
            cache: RwLock::new(Cached::default()),
        }
    }

    /// The running version string, as reported to the UI.
    pub fn current_version(&self) -> &str {
        &self.current_raw
    }

    /// The persistent data directory the update protocol files live under.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The directory holding the request/status/heartbeat files.
    fn update_dir(&self) -> PathBuf {
        self.data_dir.join("update")
    }

    /// A sensible, persistent default for the pre-update backup: under the data
    /// volume (which survives a container recreate), stamped with version and time
    /// so repeated updates never clobber one another.
    pub fn default_backup_path(&self) -> String {
        let version = self.current_raw.trim_start_matches('v');
        let stamp = now_ms();
        self.data_dir
            .join("backups")
            .join(format!("justdns-backup-{version}-{stamp}.json"))
            .to_string_lossy()
            .into_owned()
    }

    /// Whether `path` sits inside the data volume with no `..` escape — the only
    /// place a server-written backup is allowed to land, so it is actually
    /// persistent and cannot be aimed elsewhere on the host. The check is
    /// component-wise (so `/data2/x` is not "under" `/data`), which also rejects a
    /// relative path, since it cannot start with the absolute data directory.
    pub fn is_within_data(&self, path: &Path) -> bool {
        path.starts_with(&self.data_dir)
            && !path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
    }

    /// The last check, combined with whether that version is actually an upgrade.
    pub fn snapshot(&self) -> UpdateCheck {
        let cached = self
            .cache
            .read()
            .expect("update cache not poisoned")
            .clone();
        self.view(cached)
    }

    fn view(&self, cached: Cached) -> UpdateCheck {
        let update_available = match (
            self.current,
            cached.latest.as_deref().and_then(Semver::parse),
        ) {
            (Some(current), Some(latest)) => latest > current,
            _ => false,
        };
        UpdateCheck {
            current: self.current_raw.clone(),
            latest: cached.latest,
            update_available,
            notes: cached.notes,
            release_url: cached.release_url,
            checked_at_ms: cached.checked_at_ms,
            error: cached.error,
        }
    }

    /// Ask GitHub for the newest release tag and its changelog, caching the
    /// result. Blocking — call it from a background thread or `spawn_blocking`,
    /// never on the async runtime. Always records a timestamp; on failure it keeps
    /// the previous `latest`/`notes` and just stores the error.
    pub fn refresh(&self) -> UpdateCheck {
        match self.fetch_latest() {
            Ok((tag, notes)) => {
                let mut cache = self.cache.write().expect("update cache not poisoned");
                cache.release_url = Some(format!("https://github.com/{}/tree/{tag}", self.repo));
                cache.latest = Some(tag);
                cache.notes = notes;
                cache.checked_at_ms = Some(now_ms());
                cache.error = None;
            }
            Err(error) => {
                warn!(%error, "update check failed");
                let mut cache = self.cache.write().expect("update cache not poisoned");
                cache.checked_at_ms = Some(now_ms());
                cache.error = Some(error);
            }
        }
        self.snapshot()
    }

    /// Fetch the highest release tag and the changelog section for it.
    fn fetch_latest(&self) -> Result<(String, Option<String>), String> {
        let tags_url = format!(
            "https://api.github.com/repos/{}/tags?per_page=100",
            self.repo
        );
        let body = self.get(&tags_url)?;
        let tags: Vec<Tag> =
            serde_json::from_str(&body).map_err(|error| format!("bad tags response: {error}"))?;
        let latest = tags
            .iter()
            .filter_map(|tag| Semver::parse(&tag.name).map(|parsed| (parsed, &tag.name)))
            .max_by_key(|(parsed, _)| *parsed)
            .map(|(_, name)| name.clone())
            .ok_or_else(|| "no release tags found".to_owned())?;

        // The changelog is best-effort: a missing or unreadable file must not hide
        // that an update exists.
        let notes = self.fetch_changelog(&latest).unwrap_or_else(|error| {
            debug!(%error, tag = %latest, "changelog unavailable");
            None
        });
        Ok((latest, notes))
    }

    /// Fetch `CHANGELOG.md` at `tag` and return the section for that version.
    fn fetch_changelog(&self, tag: &str) -> Result<Option<String>, String> {
        let url = format!(
            "https://raw.githubusercontent.com/{}/{tag}/CHANGELOG.md",
            self.repo
        );
        let text = self.get(&url)?;
        Ok(changelog_section(&text, tag))
    }

    /// GET `url` as text, with a timeout, a size cap and a courteous User-Agent
    /// (the GitHub API rejects requests without one).
    fn get(&self, url: &str) -> Result<String, String> {
        let agent = format!("justdns/{}", self.current_raw);
        let response = ureq::get(url)
            .timeout(FETCH_TIMEOUT)
            .set("User-Agent", &agent)
            .set("Accept", "application/vnd.github+json")
            .call()
            .map_err(|error| error.to_string())?;
        let mut body = String::new();
        response
            .into_reader()
            .take(MAX_FETCH_BYTES)
            .read_to_string(&mut body)
            .map_err(|error| error.to_string())?;
        Ok(body)
    }

    /// Whether the updater sidecar is present and watching, judged by a fresh
    /// heartbeat file. When absent, the UI offers the guided-command fallback
    /// instead of a one-click update.
    pub fn updater_present(&self) -> bool {
        let heartbeat = self.update_dir().join("updater.json");
        let Ok(meta) = std::fs::metadata(&heartbeat) else {
            return false;
        };
        let Ok(modified) = meta.modified() else {
            return false;
        };
        modified
            .elapsed()
            .map(|age| age < UPDATER_FRESH)
            .unwrap_or(false)
    }

    /// The sidecar's latest progress report, if any.
    pub fn progress(&self) -> Option<UpdateProgress> {
        let status = self.update_dir().join("status.json");
        let text = std::fs::read_to_string(status).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Ask the sidecar to pull `version` and recreate the JustDNS service. The
    /// sidecar prunes older images on its own afterwards, keeping the new version
    /// and its immediate predecessor, so there is no separate cleanup request.
    pub fn request_update(&self, version: &str, requested_by: &str) -> std::io::Result<()> {
        let dir = self.update_dir();
        std::fs::create_dir_all(&dir)?;
        let request = UpdateRequest {
            version,
            requested_by,
            requested_at_ms: now_ms(),
        };
        let json = serde_json::to_vec_pretty(&request)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        // Atomic replace, so the sidecar never reads a half-written request.
        let tmp = dir.join("request.json.tmp");
        std::fs::write(&tmp, &json)?;
        std::fs::rename(tmp, dir.join("request.json"))
    }
}

/// One entry from the GitHub tags API; we only need the name.
#[derive(Deserialize)]
struct Tag {
    name: String,
}

/// Pull the `## [x.y.z]` section for `tag` out of a Keep-a-Changelog file. The
/// returned Markdown runs from that heading up to (but not including) the next
/// `## ` heading. Returns `None` if no matching section is found.
fn changelog_section(changelog: &str, tag: &str) -> Option<String> {
    let wanted = tag.trim_start_matches('v');
    let mut lines = changelog.lines();
    // Find the heading whose bracketed version matches, e.g. `## [0.10.0] - 2026-…`.
    let mut section: Vec<&str> = Vec::new();
    let mut collecting = false;
    for line in lines.by_ref() {
        if line.starts_with("## ") {
            if collecting {
                break;
            }
            if heading_version(line).is_some_and(|version| version == wanted) {
                collecting = true;
                section.push(line);
            }
        } else if collecting {
            section.push(line);
        }
    }
    if section.is_empty() {
        return None;
    }
    // Trim trailing blank lines for a tidy card.
    while section.last().is_some_and(|line| line.trim().is_empty()) {
        section.pop();
    }
    Some(section.join("\n"))
}

/// Extract the version from a `## [x.y.z] - date` heading.
fn heading_version(heading: &str) -> Option<&str> {
    let start = heading.find('[')? + 1;
    let end = heading[start..].find(']')? + start;
    Some(heading[start..end].trim())
}

/// Milliseconds since the Unix epoch.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_orders_versions() {
        assert!(Semver::parse("v0.10.0") > Semver::parse("v0.9.1"));
        assert!(Semver::parse("1.2.3") < Semver::parse("1.2.10"));
        assert_eq!(Semver::parse("0.9.1"), Semver::parse("v0.9.1"));
    }

    #[test]
    fn rejects_non_release_tags() {
        assert!(Semver::parse("v1.2.3-rc1").is_none());
        assert!(Semver::parse("latest").is_none());
        assert!(Semver::parse("dev").is_none());
        assert!(Semver::parse("v1.2").is_none());
        assert!(Semver::parse("v1.2.3.4").is_none());
    }

    #[test]
    fn extracts_the_matching_changelog_section() {
        let changelog = "\
# Changelog

## [0.10.0] - 2026-11-07
### Added
- In-app update

## [0.9.1] - 2026-11-06
### Changed
- Real logos
";
        let section = changelog_section(changelog, "v0.10.0").expect("section found");
        assert!(section.starts_with("## [0.10.0]"));
        assert!(section.contains("In-app update"));
        assert!(!section.contains("Real logos"));
        assert!(changelog_section(changelog, "v9.9.9").is_none());
    }

    #[test]
    fn update_available_only_when_newer() {
        let checker = UpdateChecker::from_env("v0.9.1");
        {
            let mut cache = checker.cache.write().unwrap();
            cache.latest = Some("v0.10.0".to_owned());
        }
        assert!(checker.snapshot().update_available);
        {
            let mut cache = checker.cache.write().unwrap();
            cache.latest = Some("v0.9.1".to_owned());
        }
        assert!(!checker.snapshot().update_available);
    }

    #[test]
    fn backup_path_is_within_data() {
        // SAFETY: single-threaded test; sets the data dir only for this check.
        unsafe { std::env::set_var("JUSTDNS_DATA_DIR", "/data") };
        let checker = UpdateChecker::from_env("v0.9.1");
        let default = checker.default_backup_path();
        assert!(checker.is_within_data(Path::new(&default)));
        assert!(!checker.is_within_data(Path::new("/etc/passwd")));
        assert!(!checker.is_within_data(Path::new("/data/../etc/passwd")));
        assert!(!checker.is_within_data(Path::new("relative/path.json")));
        assert!(!checker.is_within_data(Path::new("/data2/elsewhere.json")));
    }
}
