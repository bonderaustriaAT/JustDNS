// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Scheduled off-box backups: JustDNS itself connects to an SMB share and drops a
//! password-encrypted copy of the whole configuration there on a fixed interval,
//! keeping the newest few. Everything runs in userspace without mounting anything
//! (the container is unprivileged): SMB shells out to `smbclient`, NFS talks to
//! the server in-process through libnfs (behind the `nfs` build feature).
//!
//! The backup artefact is the very same one the manual download produces
//! ([`crate::build_backup`] + `seal_backup`), so there is only one backup format;
//! the share credentials and the backup password are write-only secrets that
//! travel across the cluster sealed, inside [`justdns_config::Settings`].

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use justdns_config::{BackupProtocol, BackupSync};
use tracing::{info, warn};

#[cfg(feature = "nfs")]
use libnfs::Nfs;
#[cfg(feature = "nfs")]
use nix::fcntl::OFlag;
#[cfg(feature = "nfs")]
use nix::sys::stat::Mode;
#[cfg(feature = "nfs")]
use std::io::Read;

use crate::AppState;

/// File-name prefix and suffix of a scheduled backup on the share. The date in the
/// middle keeps the names chronologically sortable, which is what [`prune`] relies
/// on to drop the oldest.
const BACKUP_PREFIX: &str = "justdns-backup-";
const BACKUP_SUFFIX: &str = ".jdbak";

/// How often the scheduler wakes to check whether a backup is due. The actual
/// cadence is the configured `interval_hours`; this only bounds how late a run can
/// be. Kept well under an hour so a short interval is still honoured.
const SCHEDULER_TICK: Duration = Duration::from_secs(600);

/// A place the scheduler can put a backup, list what is already there and delete
/// old ones. Implemented per protocol; every method works without mounting.
trait BackupTransport {
    /// Upload the local file at `local` as `name` (a bare file name) into the
    /// configured folder. The caller stages the bytes to disk first so the
    /// transport can stream from the file instead of holding them in memory.
    fn put(&self, name: &str, local: &Path) -> Result<(), String>;
    /// The backup files already in the folder (names only).
    fn list(&self) -> Result<Vec<String>, String>;
    /// Delete one file by name.
    fn delete(&self, name: &str) -> Result<(), String>;
}

/// SMB/CIFS transport: shells out to `smbclient`, so nothing is mounted and no
/// elevated privilege is needed. The credentials ride in a short-lived
/// owner-only file passed with `-A` (never on the command line, where a
/// process listing would show them), and any error is scrubbed of the secret.
struct SmbTransport {
    host: String,
    share: String,
    path: String,
    username: String,
    password: String,
    domain: String,
}

impl SmbTransport {
    /// The `//host/share` service string smbclient takes as its first argument.
    fn service(&self) -> String {
        format!(
            "//{}/{}",
            self.host.trim_matches('/'),
            self.share.trim_matches('/')
        )
    }

    /// The common smbclient arguments: the service, authentication (an `-A`
    /// credentials file, or `-N` for a guest share), an optional workgroup,
    /// and the starting directory.
    fn base_args(&self, auth_file: Option<&Path>) -> Vec<String> {
        let mut args = vec![self.service()];
        match auth_file {
            Some(file) => {
                args.push("-A".to_owned());
                args.push(file.to_string_lossy().into_owned());
            }
            None => args.push("-N".to_owned()),
        }
        if !self.domain.is_empty() {
            args.push("-W".to_owned());
            args.push(self.domain.clone());
        }
        let dir = self.path.trim_matches('/');
        if !dir.is_empty() {
            args.push("-D".to_owned());
            args.push(dir.to_owned());
        }
        args
    }

    /// Write the share credentials to a fresh file for one smbclient run, or
    /// `None` for a guest share (which signs in anonymously and carries no
    /// secret). Owner-only permissions where the platform has them; the file
    /// is deleted by the caller once the run ends.
    fn write_auth_file(&self) -> Result<Option<PathBuf>, String> {
        if self.username.is_empty() {
            return Ok(None);
        }
        let mut path = std::env::temp_dir();
        path.push(format!(
            "justdns-smb-auth-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let mut body = format!("username = {}\npassword = {}", self.username, self.password);
        if !self.domain.is_empty() {
            body.push_str(&format!("\ndomain = {}", self.domain));
        }
        std::fs::write(&path, body.as_bytes())
            .map_err(|error| scrub(&format!("Anmeldedatei nicht schreibbar: {error}"), &self.password))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(Some(path))
    }

    /// Run one smbclient `-c` command and return its stdout, or a scrubbed error.
    fn run(&self, command: &str) -> Result<String, String> {
        let auth_file = self.write_auth_file()?;
        let mut args = self.base_args(auth_file.as_deref());
        args.push("-c".to_owned());
        args.push(command.to_owned());
        let spawned = Command::new("smbclient").args(&args).output();
        if let Some(file) = &auth_file {
            let _ = std::fs::remove_file(file);
        }
        let output = spawned.map_err(|error| {
            scrub(
                &format!("smbclient konnte nicht gestartet werden: {error}"),
                &self.password,
            )
        })?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = {
            let err = stderr.trim();
            if err.is_empty() { stdout.trim() } else { err }
        };
        let detail = if detail.is_empty() {
            format!("smbclient endete mit {}", output.status)
        } else {
            detail.to_owned()
        };
        Err(scrub(&detail, &self.password))
    }
}

impl BackupTransport for SmbTransport {
    fn put(&self, name: &str, local: &Path) -> Result<(), String> {
        let local = local.to_string_lossy().replace('\\', "/");
        self.run(&format!("put \"{local}\" \"{name}\"")).map(|_| ())
    }

    fn list(&self) -> Result<Vec<String>, String> {
        let out = self.run("ls")?;
        Ok(out
            .split_whitespace()
            .filter(|token| token.starts_with(BACKUP_PREFIX) && token.ends_with(BACKUP_SUFFIX))
            .map(str::to_owned)
            .collect())
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        self.run(&format!("del \"{name}\"")).map(|_| ())
    }
}

/// NFS transport: talks to the NFS server in-process via libnfs, so nothing is
/// mounted and no privilege is needed. `share` is the export path (e.g.
/// `/volume1/backups`), `path` an optional subfolder inside it. A fresh context
/// is opened per operation, mirroring the stateless SMB shell-outs. Compiled only
/// with the `nfs` feature; otherwise [`transport_for`] returns a clear error.
#[cfg(feature = "nfs")]
struct NfsTransport {
    host: String,
    export: String,
    path: String,
}

#[cfg(feature = "nfs")]
impl NfsTransport {
    /// Connect and mount the export, ready for one file operation.
    fn mount(&self) -> Result<Nfs, String> {
        let nfs = Nfs::new().map_err(|error| format!("NFS-Kontext: {error}"))?;
        nfs.mount(&self.host, &self.export).map_err(|error| {
            format!(
                "NFS-Mount {}:{} fehlgeschlagen: {error}",
                self.host, self.export
            )
        })?;
        Ok(nfs)
    }

    /// The export-relative directory the backups live in (`/` or `/subfolder`).
    fn dir(&self) -> String {
        let sub = self.path.trim_matches('/');
        if sub.is_empty() {
            "/".to_owned()
        } else {
            format!("/{sub}")
        }
    }

    /// The export-relative path of one backup file.
    fn file_path(&self, name: &str) -> String {
        let sub = self.path.trim_matches('/');
        if sub.is_empty() {
            format!("/{name}")
        } else {
            format!("/{sub}/{name}")
        }
    }
}

#[cfg(feature = "nfs")]
impl BackupTransport for NfsTransport {
    fn put(&self, name: &str, local: &Path) -> Result<(), String> {
        let mut src = std::fs::File::open(local)
            .map_err(|error| format!("NFS: Quelldatei nicht lesbar: {error}"))?;
        let mut nfs = self.mount()?;
        let file = nfs
            .create(
                Path::new(&self.file_path(name)),
                // O_WRONLY so the handle is writable (nfs_create does not force an
                // access mode), O_TRUNC to overwrite a same-named leftover.
                OFlag::O_WRONLY | OFlag::O_TRUNC,
                Mode::from_bits_truncate(0o644),
            )
            .map_err(|error| format!("NFS: {name} lie\u{df} sich nicht anlegen: {error}"))?;
        // Stream the file in 1 MiB chunks so a large backup is never held in memory
        // as a whole; a full in-memory copy on a tight node can push the resolver
        // into OOM.
        let mut buf = vec![0u8; 1 << 20];
        let mut offset = 0u64;
        loop {
            let read = src
                .read(&mut buf)
                .map_err(|error| format!("NFS: Lesen der Quelldatei fehlgeschlagen: {error}"))?;
            if read == 0 {
                break;
            }
            // pwrite may write fewer bytes than asked, so advance until all are out.
            let mut written = 0usize;
            while written < read {
                let n = file
                    .pwrite(&buf[written..read], offset + written as u64)
                    .map_err(|error| format!("NFS: Schreiben fehlgeschlagen: {error}"))?;
                if n <= 0 {
                    return Err("NFS: Schreiben lieferte keine Bytes".to_owned());
                }
                written += n as usize;
            }
            offset += read as u64;
        }
        Ok(())
    }

    fn list(&self) -> Result<Vec<String>, String> {
        let mut nfs = self.mount()?;
        let dir = nfs
            .opendir(Path::new(&self.dir()))
            .map_err(|error| format!("NFS: Ordner lie\u{df} sich nicht lesen: {error}"))?;
        let mut names = Vec::new();
        for entry in dir {
            let entry = entry.map_err(|error| format!("NFS: Eintrag unlesbar: {error}"))?;
            if let Some(file) = entry.path.file_name().and_then(|s| s.to_str()) {
                if file.starts_with(BACKUP_PREFIX) && file.ends_with(BACKUP_SUFFIX) {
                    names.push(file.to_owned());
                }
            }
        }
        Ok(names)
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        let nfs = self.mount()?;
        nfs.unlink(Path::new(&self.file_path(name)))
            .map_err(|error| format!("NFS: {name} lie\u{df} sich nicht l\u{f6}schen: {error}"))
    }
}

/// Remove `secret` from `text` so a password can never reach a log or an error
/// shown in the UI. A no-op when the secret is empty.
fn scrub(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        text.to_owned()
    } else {
        text.replace(secret, "***")
    }
}

/// A unique temp path for the local copy smbclient uploads, so two runs never
/// collide on the same file.
fn unique_temp_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("jdbak-{}-{}", crate::now_ms(), name));
    path
}

/// The transport for a configuration, or an error when it is not usable yet.
fn transport_for(cfg: &BackupSync) -> Result<Box<dyn BackupTransport>, String> {
    match cfg.protocol {
        BackupProtocol::Smb => Ok(Box::new(SmbTransport {
            host: cfg.host.trim().to_owned(),
            share: cfg.share.trim().to_owned(),
            path: cfg.path.trim().to_owned(),
            username: cfg.username.clone(),
            password: cfg.password.clone(),
            domain: cfg.domain.trim().to_owned(),
        })),
        BackupProtocol::Nfs => nfs_transport(cfg),
    }
}

/// The NFS transport, when the `nfs` feature is compiled in.
#[cfg(feature = "nfs")]
fn nfs_transport(cfg: &BackupSync) -> Result<Box<dyn BackupTransport>, String> {
    Ok(Box::new(NfsTransport {
        host: cfg.host.trim().to_owned(),
        export: cfg.share.trim().to_owned(),
        path: cfg.path.trim().to_owned(),
    }))
}

/// Without the `nfs` feature the binding is not linked, so NFS is unavailable and
/// the caller gets a clear error instead.
#[cfg(not(feature = "nfs"))]
fn nfs_transport(_cfg: &BackupSync) -> Result<Box<dyn BackupTransport>, String> {
    Err("NFS ist in diesem Build nicht verf\u{fc}gbar".to_owned())
}

/// The dated file name for a backup taken at `now_ms` (UTC), chronologically
/// sortable by plain string order.
pub fn backup_filename(now_ms: u64) -> String {
    let (y, mo, d, h, mi, s) = civil_from_ms(now_ms);
    format!("{BACKUP_PREFIX}{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}{BACKUP_SUFFIX}")
}

/// Given the backup files on the share and how many to keep, the names to delete
/// (oldest first). The dated names sort chronologically, so keeping the last
/// `keep` by name keeps the newest.
pub fn prune(mut entries: Vec<String>, keep: u32) -> Vec<String> {
    entries.sort();
    let keep = keep.max(1) as usize;
    if entries.len() <= keep {
        return Vec::new();
    }
    let remove = entries.len() - keep;
    entries.into_iter().take(remove).collect()
}

/// Split a Unix-millisecond instant into UTC `(year, month, day, hour, min, sec)`.
/// Uses Howard Hinnant's `civil_from_days`, so it needs no date-library dependency
/// and is deterministic for the file-name stamp.
fn civil_from_ms(ms: u64) -> (i64, u32, u32, u32, u32, u32) {
    let secs = (ms / 1000) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = (rem / 3600) as u32;
    let minute = ((rem % 3600) / 60) as u32;
    let second = (rem % 60) as u32;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day, hour, minute, second)
}

/// Build, seal and upload one backup, then prune the share to the newest `keep`.
/// Returns an error string on any step so the caller can surface and record it.
pub fn run_once(state: &AppState) -> Result<(), String> {
    let cfg = state.config.get().backup_sync;
    if cfg.backup_password.is_empty() {
        return Err("Kein Backup-Passwort gesetzt".to_owned());
    }
    if cfg.host.trim().is_empty() || cfg.share.trim().is_empty() {
        return Err("Host und Share m\u{fc}ssen gesetzt sein".to_owned());
    }
    let transport = transport_for(&cfg)?;

    // Build and seal in a tight scope so the plaintext config (which can be large
    // on a node with many list entries) is freed as soon as it is sealed, and never
    // held alongside the sealed copy longer than needed. Keeping several full copies
    // of the config in memory at once is what can push a tight node into OOM.
    let json = {
        let mut backup = crate::build_backup(state);
        // Strip the write-only secrets even though the file is sealed, exactly as
        // the manual download does; the backup password alone must never reveal them.
        crate::redact_settings_secrets(&mut backup.settings);
        serde_json::to_vec(&backup).map_err(|error| error.to_string())?
    };
    let sealed = justdns_secret::seal_backup(&cfg.backup_password, &json);
    drop(json);

    // Stage the sealed bytes to a local temp file and upload from there, so the
    // transport streams from disk instead of holding yet another in-memory copy
    // during the (slow) upload.
    let name = backup_filename(crate::now_ms());
    let tmp = unique_temp_path(&name);
    {
        let mut file = std::fs::File::create(&tmp)
            .map_err(|error| format!("Temp-Datei lie\u{df} sich nicht anlegen: {error}"))?;
        file.write_all(&sealed)
            .map_err(|error| format!("Temp-Datei lie\u{df} sich nicht schreiben: {error}"))?;
    }
    drop(sealed);
    let put = transport.put(&name, &tmp);
    let _ = std::fs::remove_file(&tmp);
    put?;

    // Pruning is best-effort: a failed list or delete must not fail an otherwise
    // successful upload, so a transient share error just leaves the extra copies.
    match transport.list() {
        Ok(entries) => {
            for stale in prune(entries, cfg.keep) {
                if let Err(error) = transport.delete(&stale) {
                    warn!(%error, file = %stale, "config backup: could not prune old file");
                }
            }
        }
        Err(error) => warn!(%error, "config backup: could not list share for pruning"),
    }
    Ok(())
}

/// Verify the configuration by writing a tiny probe file to the share and removing
/// it again (the "test connection" action). The probe name is not a backup name,
/// so it is never listed or pruned as one.
pub fn test_connection(cfg: &BackupSync) -> Result<(), String> {
    let transport = transport_for(cfg)?;
    let probe = format!("justdns-writetest-{}.tmp", crate::now_ms());
    let tmp = unique_temp_path(&probe);
    std::fs::write(&tmp, b"justdns connectivity probe")
        .map_err(|error| format!("Probe-Datei lie\u{df} sich nicht anlegen: {error}"))?;
    let put = transport.put(&probe, &tmp);
    let _ = std::fs::remove_file(&tmp);
    put?;
    transport.delete(&probe)
}

/// Write the outcome of a run back into the config as status (never a secret), so
/// the UI can show when the last backup ran and whether it worked.
pub fn record_result(state: &AppState, ok: bool, error: String) {
    let mut settings = state.config.get();
    settings.backup_sync.last_run_ms = crate::now_ms();
    settings.backup_sync.last_ok = ok;
    settings.backup_sync.last_error = error;
    if let Err(error) = state.config.set(settings) {
        warn!(%error, "config backup: could not persist run status");
    }
}

/// Persist only the attempt time, before a run starts its heavy build and upload.
/// If the process is killed mid-backup (for example an OOM kill on a tight node),
/// `last_run_ms` is already advanced, so the scheduler does not treat the backup as
/// due again on the next start and loop. The outcome fields stay as they were and
/// are set correctly by [`record_result`] once a run actually finishes.
fn mark_attempt(state: &AppState) {
    let mut settings = state.config.get();
    settings.backup_sync.last_run_ms = crate::now_ms();
    if let Err(error) = state.config.set(settings) {
        warn!(%error, "config backup: could not persist attempt time");
    }
}

/// Start the scheduler thread: it wakes periodically and, when auto-backup is on
/// and this node is the primary (or standalone), runs a backup once the configured
/// interval has elapsed since the last one. Mirrors the other `serve()` background
/// loops. Only the primary backs up so a cluster does not write N copies at once; a
/// manual "back up now" is allowed on any node.
pub fn spawn_scheduler(state: AppState) {
    std::thread::spawn(move || {
        // A short warm-up so the stores and config are up before the first check.
        std::thread::sleep(Duration::from_secs(90));
        loop {
            let cfg = state.config.get().backup_sync;
            if cfg.enabled && crate::cluster_role(&state).is_primary {
                let interval_ms = u64::from(cfg.interval_hours.max(1)) * 3_600_000;
                let due = cfg.last_run_ms == 0
                    || crate::now_ms().saturating_sub(cfg.last_run_ms) >= interval_ms;
                if due {
                    // Record the attempt before the heavy run, so a crash mid-backup
                    // cannot leave the backup "due" and make the next start loop.
                    mark_attempt(&state);
                    match run_once(&state) {
                        Ok(()) => {
                            record_result(&state, true, String::new());
                            info!("config backup uploaded to share");
                        }
                        Err(error) => {
                            record_result(&state, false, error.clone());
                            warn!(%error, "config backup failed");
                        }
                    }
                }
            }
            std::thread::sleep(SCHEDULER_TICK);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_is_dated_and_sorts_chronologically() {
        // 2026-11-19 03:00:00 UTC = 1_795_057_200_000 ms.
        let name = backup_filename(1_795_057_200_000);
        assert_eq!(name, "justdns-backup-20261119-030000.jdbak");
        // A later instant sorts after an earlier one by plain string order.
        let later = backup_filename(1_795_057_200_000 + 3_600_000);
        assert!(later > name);
    }

    #[test]
    fn prune_keeps_the_newest() {
        let entries = vec![
            "justdns-backup-20261101-030000.jdbak".to_owned(),
            "justdns-backup-20261103-030000.jdbak".to_owned(),
            "justdns-backup-20261102-030000.jdbak".to_owned(),
        ];
        let remove = prune(entries, 2);
        assert_eq!(
            remove,
            vec!["justdns-backup-20261101-030000.jdbak".to_owned()]
        );
    }

    #[test]
    fn prune_keeps_everything_when_under_the_limit() {
        let entries = vec!["justdns-backup-20261101-030000.jdbak".to_owned()];
        assert!(prune(entries, 14).is_empty());
    }

    #[test]
    fn prune_never_keeps_zero() {
        let entries = vec![
            "justdns-backup-20261101-030000.jdbak".to_owned(),
            "justdns-backup-20261102-030000.jdbak".to_owned(),
        ];
        // keep = 0 is clamped to 1, so exactly one survives.
        assert_eq!(prune(entries, 0).len(), 1);
    }

    #[test]
    fn scrub_hides_the_password() {
        assert_eq!(
            scrub("login as hunter2 failed", "hunter2"),
            "login as *** failed"
        );
        assert_eq!(scrub("no secret here", ""), "no secret here");
    }

    #[test]
    fn smb_args_carry_credentials_via_file_not_the_command_line() {
        let smb = SmbTransport {
            host: "nas".to_owned(),
            share: "backups".to_owned(),
            path: "justdns".to_owned(),
            username: "user".to_owned(),
            password: "hunter2".to_owned(),
            domain: "WORKGROUP".to_owned(),
        };
        let args = smb.base_args(Some(Path::new("/tmp/creds")));
        let joined = args.join(" ");
        assert!(
            joined.contains("-A /tmp/creds"),
            "authentication rides in the file"
        );
        assert!(
            !joined.contains("user%hunter2"),
            "the password never appears as a process argument"
        );

        // A guest share signs in anonymously; no credentials file involved.
        let guest = SmbTransport {
            username: String::new(),
            ..smb
        };
        assert!(guest.base_args(None).contains(&"-N".to_owned()));
    }
}
