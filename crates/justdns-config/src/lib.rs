// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Runtime configuration that the web UI can edit and that survives restarts.
//!
//! Startup still reads the environment (see the binary's `config` module), but
//! those values are only the *seed*: on first run they are written to a JSON
//! file, and from then on that file is the source of truth. The dashboard reads
//! and writes it through the API, so settings no longer live only in a single
//! browser's local storage.
//!
//! Most settings are consumed where their feature lives. IPv6 is wired through
//! live: the store publishes it as a shared flag the DNS pipeline reads per
//! query, so toggling it in the UI takes effect without a restart.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

/// How the resolver picks between the configured upstreams. Mirrors the DNS
/// core's `UpstreamMode`; kept here as its own type so the config crate stays a
/// leaf that the core and the web layer can both depend on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpstreamMode {
    /// Ask every upstream at once and take the first answer.
    #[default]
    Racing,
    /// Try the upstreams in order, moving on only when one fails.
    Fallback,
    /// Spread queries across the upstreams in turn.
    Balanced,
    /// Pick the upstream by domain rules.
    Rules,
}

/// The settings the dashboard shows and edits. Field names are serialised in
/// camelCase to match the UI's JSON directly. `#[serde(default)]` lets a file
/// written by an older build load even when newer fields are missing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Block known advertising and tracking domains.
    pub block_ads_trackers: bool,
    /// Redirect big search engines to their safe-search addresses.
    pub safe_search: bool,
    /// Block known phishing and malware domains.
    pub block_malware: bool,
    /// How the resolver chooses between upstreams.
    pub upstream_mode: UpstreamMode,
    /// Talk to the upstreams over DNS-over-TLS rather than plaintext.
    pub encrypt_upstream: bool,
    /// Answer AAAA queries instead of returning empty NODATA. Wired live.
    pub ipv6: bool,
    /// Record handled queries for the log view.
    pub query_log: bool,
    /// How many days log entries are kept.
    pub log_retention_days: u32,
    /// Serve expired cache entries while every upstream is unreachable.
    pub cache_serve_stale: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            block_ads_trackers: true,
            safe_search: false,
            block_malware: true,
            upstream_mode: UpstreamMode::Racing,
            encrypt_upstream: true,
            ipv6: false,
            query_log: true,
            log_retention_days: 7,
            cache_serve_stale: false,
        }
    }
}

/// Errors from loading or saving the configuration file.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config io ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("config parse ({path}): {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

struct Inner {
    path: PathBuf,
    settings: RwLock<Settings>,
    /// Live IPv6 flag the DNS pipeline reads per query, kept in step with
    /// `settings.ipv6` on every load and save.
    ipv6: Arc<AtomicBool>,
}

/// A cheap, cloneable handle to the persistent configuration.
#[derive(Clone)]
pub struct ConfigStore {
    inner: Arc<Inner>,
}

impl ConfigStore {
    /// Load the configuration from `path`. If the file is missing, `seed` (the
    /// values derived from the environment) is written as the starting point;
    /// a file that exists but cannot be parsed is reported as an error so a
    /// typo is not silently overwritten.
    pub fn load(path: impl Into<PathBuf>, seed: Settings) -> Result<Self, ConfigError> {
        let path = path.into();
        let settings = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|source| ConfigError::Parse {
                path: path.clone(),
                source,
            })?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                write_atomic(&path, &seed)?;
                seed
            }
            Err(source) => {
                return Err(ConfigError::Io {
                    path: path.clone(),
                    source,
                });
            }
        };

        let ipv6 = Arc::new(AtomicBool::new(settings.ipv6));
        Ok(Self {
            inner: Arc::new(Inner {
                path,
                settings: RwLock::new(settings),
                ipv6,
            }),
        })
    }

    /// A snapshot of the current settings.
    pub fn get(&self) -> Settings {
        self.inner
            .settings
            .read()
            .expect("config lock poisoned")
            .clone()
    }

    /// Replace the settings, publish the live IPv6 flag and persist to disk.
    /// The in-memory value is updated even if the write fails, so the running
    /// instance reflects the request; the error is returned so the caller can
    /// report that it was not saved durably.
    pub fn set(&self, settings: Settings) -> Result<(), ConfigError> {
        self.inner.ipv6.store(settings.ipv6, Ordering::Relaxed);
        *self.inner.settings.write().expect("config lock poisoned") = settings.clone();
        write_atomic(&self.inner.path, &settings)
    }

    /// The shared IPv6 flag for the DNS pipeline to read per query.
    pub fn ipv6_flag(&self) -> Arc<AtomicBool> {
        self.inner.ipv6.clone()
    }
}

/// Write `settings` to `path` as pretty JSON, atomically: serialise to a
/// sibling temp file first, then rename over the target so a crash mid-write
/// never leaves a half-written config.
fn write_atomic(path: &Path, settings: &Settings) -> Result<(), ConfigError> {
    let io_err = |source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(io_err)?;
        }
    }

    let json = serde_json::to_vec_pretty(settings).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })?;

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|source| ConfigError::Io {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(io_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        (dir, path)
    }

    #[test]
    fn missing_file_is_seeded_and_written() {
        let (_dir, path) = temp_path();
        let seed = Settings {
            ipv6: true,
            log_retention_days: 30,
            ..Settings::default()
        };

        let store = ConfigStore::load(&path, seed.clone()).unwrap();
        assert_eq!(store.get(), seed);
        assert!(path.exists(), "seed should be written on first run");

        // A reload reads the file, not the (now different) seed.
        let reopened = ConfigStore::load(&path, Settings::default()).unwrap();
        assert_eq!(reopened.get(), seed);
    }

    #[test]
    fn set_persists_and_publishes_ipv6() {
        let (_dir, path) = temp_path();
        let store = ConfigStore::load(&path, Settings::default()).unwrap();
        let flag = store.ipv6_flag();
        assert!(!flag.load(Ordering::Relaxed));

        let mut next = store.get();
        next.ipv6 = true;
        next.upstream_mode = UpstreamMode::Fallback;
        store.set(next.clone()).unwrap();

        assert!(
            flag.load(Ordering::Relaxed),
            "live flag follows the setting"
        );
        let reopened = ConfigStore::load(&path, Settings::default()).unwrap();
        assert_eq!(reopened.get(), next);
    }

    #[test]
    fn partial_file_fills_missing_fields_from_defaults() {
        let (_dir, path) = temp_path();
        std::fs::write(&path, r#"{"ipv6":true}"#).unwrap();

        let store = ConfigStore::load(&path, Settings::default()).unwrap();
        let settings = store.get();
        assert!(settings.ipv6);
        // A field absent from the file falls back to the default.
        assert_eq!(settings.log_retention_days, 7);
        assert!(settings.block_malware);
    }

    #[test]
    fn unparseable_file_is_an_error() {
        let (_dir, path) = temp_path();
        std::fs::write(&path, "not json").unwrap();
        assert!(matches!(
            ConfigStore::load(&path, Settings::default()),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn upstream_mode_serialises_lowercase() {
        let json = serde_json::to_string(&UpstreamMode::Balanced).unwrap();
        assert_eq!(json, "\"balanced\"");
    }
}
