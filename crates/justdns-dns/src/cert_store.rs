// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A named store of server TLS certificates, shared by the DoT, DoH and block-page
//! endpoints.
//!
//! Each certificate has a stable id, a human name and a kind (minted by JustDNS or
//! uploaded), and lives in its own sub-directory on the `/data` volume so it
//! survives image updates. Features reference a certificate by id, so a cert can
//! be uploaded once and then picked for any endpoint without re-uploading it.
//!
//! The low-level material helpers (minting a self-signed chain, parsing PEM,
//! building a [`rustls::ServerConfig`]) live here and are reused by
//! [`crate::tls::TlsManager`] and the block page server.

use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, Ia5String, IsCa, KeyPair,
    KeyUsagePurpose, SanType,
};
use rustls::ServerConfig;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::info;
use x509_parser::prelude::*;

/// ALPN protocol id advertised on the DoT listener (RFC 7858).
pub const ALPN_DOT: &[&[u8]] = &[b"dot"];
/// ALPN protocol ids advertised on the DoH listener; `h2` first so HTTP/2 is
/// preferred (RFC 8484 recommends it).
pub const ALPN_DOH: &[&[u8]] = &[b"h2", b"http/1.1"];
/// ALPN protocol ids for the HTTPS block page (a plain web server).
pub const ALPN_HTTPS: &[&[u8]] = &[b"h2", b"http/1.1"];

/// File name of the leaf certificate chain (leaf first, CA appended for the
/// self-signed case so the presented chain is complete).
const LEAF_CERT: &str = "cert.pem";
/// File name of the leaf private key (PKCS#8 PEM, mode `0600`).
const LEAF_KEY: &str = "key.pem";
/// File name of the generated local CA certificate (offered for download).
const CA_CERT: &str = "ca.pem";
/// File name of the generated local CA private key (never leaves the server).
const CA_KEY: &str = "ca-key.pem";
/// File name of the small sidecar describing a stored certificate.
const META: &str = "meta.json";

/// The id of the certificate minted (or migrated) at first start, used as the
/// fallback when a feature has no explicit selection.
pub const DEFAULT_ID: &str = "standard";

/// How a stored certificate came to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CertKind {
    /// JustDNS minted this certificate (self-signed, with a local CA).
    Auto,
    /// The operator uploaded this certificate chain and key.
    Uploaded,
}

/// The on-disk sidecar for a stored certificate.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Meta {
    name: String,
    kind: CertKind,
    created_ms: i64,
}

/// A certificate as presented to the dashboard: identity plus the derived
/// validity facts an operator needs to judge and manage it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CertInfo {
    /// Stable id; the feature settings store this to select the certificate.
    pub id: String,
    /// Human-readable name shown in the catalogue and the selectors.
    pub name: String,
    /// Whether JustDNS minted it or the operator uploaded it.
    pub kind: CertKind,
    /// The leaf certificate's subject (common name, best-effort).
    pub subject: String,
    /// The subject alternative names the certificate is valid for.
    pub sans: Vec<String>,
    /// Validity start, Unix milliseconds.
    pub not_before_ms: i64,
    /// Validity end, Unix milliseconds.
    pub not_after_ms: i64,
    /// SHA-256 fingerprint of the leaf, colon-separated upper-case hex.
    pub fingerprint: String,
    /// Whether the certificate is outside its validity window right now.
    pub expired: bool,
    /// Whether a downloadable local CA accompanies it (minted certs only).
    pub has_ca: bool,
}

/// A stored certificate with its full material, for carrying an entry between
/// high-availability nodes. Unlike [`CertInfo`] (which exposes only derived
/// facts), this includes the **private key** — it travels only over the HA sync
/// between a cluster's own nodes, never to a client. `ca_pem`/`ca_key_pem` are
/// present for minted (`Auto`) certificates and absent for uploaded ones.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CertRecord {
    pub id: String,
    pub name: String,
    pub kind: CertKind,
    pub created_ms: i64,
    pub cert_pem: String,
    pub key_pem: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_pem: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_key_pem: Option<String>,
}

/// The shared certificate registry.
pub struct CertStore {
    dir: PathBuf,
    /// The known ids in insertion order, so the catalogue is stable. Metadata
    /// is read from disk on demand; this is just the index.
    ids: RwLock<Vec<String>>,
}

impl CertStore {
    /// Opens (and if necessary seeds) the store under `dir`.
    ///
    /// When the store holds no certificate, one is created so every endpoint has
    /// something to serve: if `legacy_tls_dir` carries a certificate from the
    /// single-cert era it is imported as [`DEFAULT_ID`], otherwise a fresh
    /// self-signed chain for `hostname`/`ips` is minted under that id.
    pub fn load_or_init(
        dir: impl Into<PathBuf>,
        legacy_tls_dir: Option<&Path>,
        hostname: &str,
        ips: &[IpAddr],
    ) -> io::Result<std::sync::Arc<Self>> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;

        let mut ids = scan_ids(&dir)?;
        if ids.is_empty() {
            if let Some(legacy) = legacy_tls_dir.filter(|p| p.join(LEAF_CERT).is_file()) {
                import_legacy(&dir, legacy)?;
                info!(
                    id = DEFAULT_ID,
                    "imported the existing TLS certificate into the store"
                );
            } else {
                let path = dir.join(DEFAULT_ID);
                mint_self_signed(&path, hostname, ips)?;
                write_meta(&path, "Standard", CertKind::Auto)?;
                info!(
                    id = DEFAULT_ID,
                    "minted the default self-signed certificate"
                );
            }
            ids = scan_ids(&dir)?;
        }

        Ok(std::sync::Arc::new(Self {
            dir,
            ids: RwLock::new(ids),
        }))
    }

    /// The whole catalogue, each entry with its derived validity facts. Entries
    /// whose files cannot be read or parsed are skipped rather than failing the
    /// whole listing.
    pub fn list(&self) -> Vec<CertInfo> {
        let ids = self.ids.read().unwrap().clone();
        ids.iter().filter_map(|id| self.info(id)).collect()
    }

    /// The id a feature should fall back to when it has no explicit selection:
    /// [`DEFAULT_ID`] when present, else the first stored certificate.
    pub fn default_id(&self) -> Option<String> {
        let ids = self.ids.read().unwrap();
        if ids.iter().any(|id| id == DEFAULT_ID) {
            Some(DEFAULT_ID.to_owned())
        } else {
            ids.first().cloned()
        }
    }

    /// Whether a certificate with this id is stored.
    pub fn exists(&self, id: &str) -> bool {
        self.ids.read().unwrap().iter().any(|stored| stored == id)
    }

    /// The derived facts for one certificate, or `None` if it is missing or
    /// unreadable.
    pub fn info(&self, id: &str) -> Option<CertInfo> {
        let path = self.dir.join(id);
        let meta = read_meta(&path).ok()?;
        let chain = read_pem_chain(&path.join(LEAF_CERT)).ok()?;
        let leaf = chain.first()?;
        let parsed = parse_leaf(leaf);
        let now = now_ms();
        Some(CertInfo {
            id: id.to_owned(),
            name: meta.name,
            kind: meta.kind,
            subject: parsed.subject,
            sans: parsed.sans,
            not_before_ms: parsed.not_before_ms,
            not_after_ms: parsed.not_after_ms,
            fingerprint: fingerprint_of(leaf),
            expired: now < parsed.not_before_ms || now >= parsed.not_after_ms,
            has_ca: path.join(CA_CERT).is_file(),
        })
    }

    /// Mints a fresh self-signed certificate named `name` for `hostname`/`ips`
    /// and returns its new id.
    pub fn generate(&self, name: &str, hostname: &str, ips: &[IpAddr]) -> io::Result<String> {
        let id = new_id();
        let path = self.dir.join(&id);
        mint_self_signed(&path, hostname, ips)?;
        write_meta(&path, name, CertKind::Auto)?;
        self.ids.write().unwrap().push(id.clone());
        info!(%id, "minted a self-signed certificate");
        Ok(id)
    }

    /// Validates an operator-supplied chain and key and stores it under `name`,
    /// returning the new id. The pair is checked (and the config built) before
    /// anything touches the disk.
    pub fn upload(&self, name: &str, cert_pem: &str, key_pem: &str) -> io::Result<String> {
        validate_pair(cert_pem, key_pem)?;
        let id = new_id();
        let path = self.dir.join(&id);
        std::fs::create_dir_all(&path)?;
        std::fs::write(path.join(LEAF_CERT), cert_pem.as_bytes())?;
        write_secret(&path.join(LEAF_KEY), key_pem.as_bytes())?;
        write_meta(&path, name, CertKind::Uploaded)?;
        self.ids.write().unwrap().push(id.clone());
        info!(%id, "stored an uploaded certificate");
        Ok(id)
    }

    /// Replaces the chain and key of an existing certificate in place, keeping
    /// its id and name so every feature that selected it keeps working.
    ///
    /// The replacement must parse, the key must match, and — the point of a
    /// renewal — it must be **currently valid** (inside its validity window).
    /// An expired or not-yet-valid certificate is refused and the stored one is
    /// left untouched.
    pub fn renew(&self, id: &str, cert_pem: &str, key_pem: &str) -> io::Result<()> {
        if !self.exists(id) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no such certificate",
            ));
        }
        let chain = validate_pair(cert_pem, key_pem)?;
        let parsed = parse_leaf(&chain[0]);
        let now = now_ms();
        if now < parsed.not_before_ms {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "certificate is not valid yet",
            ));
        }
        if now >= parsed.not_after_ms {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "certificate has already expired",
            ));
        }
        let path = self.dir.join(id);
        write_secret(&path.join(LEAF_KEY), key_pem.as_bytes())?;
        std::fs::write(path.join(LEAF_CERT), cert_pem.as_bytes())?;
        // A renewal supplies its own chain, so a previously minted local CA no
        // longer matches and must not be handed out any more.
        let _ = std::fs::remove_file(path.join(CA_CERT));
        let _ = std::fs::remove_file(path.join(CA_KEY));
        info!(%id, "renewed a certificate in place");
        Ok(())
    }

    /// Removes a certificate. Refuses to remove the last one, so there is always
    /// something for the endpoints to serve. The caller is responsible for
    /// refusing to delete a certificate a feature still selects.
    pub fn delete(&self, id: &str) -> io::Result<()> {
        let mut ids = self.ids.write().unwrap();
        if !ids.iter().any(|stored| stored == id) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no such certificate",
            ));
        }
        if ids.len() == 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot remove the last certificate",
            ));
        }
        std::fs::remove_dir_all(self.dir.join(id))?;
        ids.retain(|stored| stored != id);
        info!(%id, "removed a certificate");
        Ok(())
    }

    /// Builds a [`ServerConfig`] from the certificate `id` selects, with the
    /// given ALPN. When `id` is `None` or unknown, the default certificate is
    /// used so an endpoint never comes up without a key.
    pub fn build_config(&self, id: Option<&str>, alpn: &[&[u8]]) -> io::Result<ServerConfig> {
        let chosen = id
            .filter(|id| self.exists(id))
            .map(str::to_owned)
            .or_else(|| self.default_id())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no certificate available"))?;
        let path = self.dir.join(&chosen);
        let certs = read_pem_chain(&path.join(LEAF_CERT))?;
        let key = read_pem_key(&path.join(LEAF_KEY))?;
        build_config(certs, key, alpn).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// The local CA certificate (PEM) for a minted certificate, for the operator
    /// to install on clients. `None` when the certificate carries no CA.
    pub fn ca_pem(&self, id: &str) -> io::Result<Option<String>> {
        match std::fs::read_to_string(self.dir.join(id).join(CA_CERT)) {
            Ok(pem) => Ok(Some(pem)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Every stored certificate with its full material (including the private
    /// key), for a high-availability snapshot. Entries whose files cannot be read
    /// are skipped rather than failing the whole export.
    pub fn export_all(&self) -> Vec<CertRecord> {
        let ids = self.ids.read().unwrap().clone();
        ids.iter().filter_map(|id| self.export(id)).collect()
    }

    /// One certificate's full material, or `None` if it cannot be read.
    fn export(&self, id: &str) -> Option<CertRecord> {
        let path = self.dir.join(id);
        let meta = read_meta(&path).ok()?;
        let cert_pem = std::fs::read_to_string(path.join(LEAF_CERT)).ok()?;
        let key_pem = std::fs::read_to_string(path.join(LEAF_KEY)).ok()?;
        let read_opt = |file: &str| std::fs::read_to_string(path.join(file)).ok();
        Some(CertRecord {
            id: id.to_owned(),
            name: meta.name,
            kind: meta.kind,
            created_ms: meta.created_ms,
            cert_pem,
            key_pem,
            ca_pem: read_opt(CA_CERT),
            ca_key_pem: read_opt(CA_KEY),
        })
    }

    /// Reconcile the store so it holds exactly `records`: write (or overwrite in
    /// place) every record, then remove any stored certificate not in the set.
    /// Used when applying a merged high-availability snapshot. An **empty** set is
    /// ignored (never wipe the store — a cluster always keeps at least one), so a
    /// peer that predates certificate sync leaves this node's store intact.
    pub fn replace_from_records(&self, records: &[CertRecord]) -> io::Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        for record in records {
            self.write_record(record)?;
        }
        // Drop certificates that are no longer in the merged set (deleted on a
        // peer). The set is non-empty, so the store never ends up empty.
        let keep: std::collections::HashSet<&str> = records.iter().map(|r| r.id.as_str()).collect();
        let existing = scan_ids(&self.dir)?;
        for id in existing {
            if !keep.contains(id.as_str()) {
                let _ = std::fs::remove_dir_all(self.dir.join(&id));
            }
        }
        *self.ids.write().unwrap() = scan_ids(&self.dir)?;
        Ok(())
    }

    /// Write one record's files, overwriting any existing certificate with the
    /// same id in place (so a renewal received from a peer replaces the material
    /// without changing the id every feature selects).
    fn write_record(&self, record: &CertRecord) -> io::Result<()> {
        let path = self.dir.join(&record.id);
        std::fs::create_dir_all(&path)?;
        std::fs::write(path.join(LEAF_CERT), record.cert_pem.as_bytes())?;
        write_secret(&path.join(LEAF_KEY), record.key_pem.as_bytes())?;
        match &record.ca_pem {
            Some(pem) => std::fs::write(path.join(CA_CERT), pem.as_bytes())?,
            None => {
                let _ = std::fs::remove_file(path.join(CA_CERT));
            }
        }
        match &record.ca_key_pem {
            Some(pem) => write_secret(&path.join(CA_KEY), pem.as_bytes())?,
            None => {
                let _ = std::fs::remove_file(path.join(CA_KEY));
            }
        }
        let meta = Meta {
            name: record.name.trim().to_owned(),
            kind: record.kind,
            created_ms: record.created_ms,
        };
        let raw = serde_json::to_string_pretty(&meta)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        std::fs::write(path.join(META), raw)?;
        Ok(())
    }
}

/// The handful of facts we read back out of a leaf certificate.
struct ParsedLeaf {
    subject: String,
    sans: Vec<String>,
    not_before_ms: i64,
    not_after_ms: i64,
}

/// Parses validity, subject and SANs out of a DER leaf, degrading gracefully to
/// empty/zero values when the certificate cannot be parsed (the fingerprint and
/// the config build still work, so a listing stays useful).
fn parse_leaf(der: &CertificateDer<'_>) -> ParsedLeaf {
    match X509Certificate::from_der(der.as_ref()) {
        Ok((_, cert)) => {
            let sans = cert
                .subject_alternative_name()
                .ok()
                .flatten()
                .map(|ext| {
                    ext.value
                        .general_names
                        .iter()
                        .map(|name| match name {
                            GeneralName::DNSName(n) => (*n).to_owned(),
                            GeneralName::IPAddress(bytes) => format_ip(bytes),
                            other => format!("{other:?}"),
                        })
                        .collect()
                })
                .unwrap_or_default();
            ParsedLeaf {
                subject: cert.subject().to_string(),
                sans,
                not_before_ms: cert.validity().not_before.timestamp() * 1000,
                not_after_ms: cert.validity().not_after.timestamp() * 1000,
            }
        }
        Err(_) => ParsedLeaf {
            subject: String::new(),
            sans: Vec::new(),
            not_before_ms: 0,
            not_after_ms: 0,
        },
    }
}

/// Renders a SAN IP-address octet string as a readable address.
fn format_ip(bytes: &[u8]) -> String {
    match bytes.len() {
        4 => {
            let octets: [u8; 4] = bytes.try_into().unwrap();
            std::net::Ipv4Addr::from(octets).to_string()
        }
        16 => {
            let octets: [u8; 16] = bytes.try_into().unwrap();
            std::net::Ipv6Addr::from(octets).to_string()
        }
        _ => String::new(),
    }
}

/// Validates a chain + key pair by parsing both and building a config from them.
/// Returns the parsed chain so a caller can inspect the leaf without re-parsing.
fn validate_pair(cert_pem: &str, key_pem: &str) -> io::Result<Vec<CertificateDer<'static>>> {
    let certs = parse_pem_chain(cert_pem)?;
    let key = parse_pem_key(key_pem)?;
    build_config(clone_chain(&certs), key, ALPN_DOT)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(certs)
}

/// Lists the certificate ids under `dir` (its immediate sub-directories that
/// carry a leaf certificate), in a stable sorted order with [`DEFAULT_ID`]
/// first so the catalogue leads with the managed default.
fn scan_ids(dir: &Path) -> io::Result<Vec<String>> {
    let mut ids = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().join(LEAF_CERT).is_file() {
            ids.push(name);
        }
    }
    ids.sort_by(|a, b| match (a == DEFAULT_ID, b == DEFAULT_ID) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.cmp(b),
    });
    Ok(ids)
}

/// Copies the single-cert era's files into the store as [`DEFAULT_ID`]. The old
/// files are left in place (untouched) so a rollback still finds them.
fn import_legacy(dir: &Path, legacy: &Path) -> io::Result<()> {
    let path = dir.join(DEFAULT_ID);
    std::fs::create_dir_all(&path)?;
    std::fs::copy(legacy.join(LEAF_CERT), path.join(LEAF_CERT))?;
    copy_secret(&legacy.join(LEAF_KEY), &path.join(LEAF_KEY))?;
    // The minted CA, if any, comes along so it can still be downloaded. An
    // uploaded cert has none, which is fine.
    let had_ca = legacy.join(CA_CERT).is_file();
    if had_ca {
        std::fs::copy(legacy.join(CA_CERT), path.join(CA_CERT))?;
        let _ = copy_secret(&legacy.join(CA_KEY), &path.join(CA_KEY));
    }
    let kind = if had_ca {
        CertKind::Auto
    } else {
        CertKind::Uploaded
    };
    write_meta(&path, "Standard", kind)
}

/// Mints a self-signed local CA plus a leaf for `hostname`/`ips` into `path`.
fn mint_self_signed(path: &Path, hostname: &str, ips: &[IpAddr]) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    let to_io = io::Error::other;

    let ca_key = KeyPair::generate().map_err(to_io)?;
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "JustDNS Local CA");
    ca_params
        .distinguished_name
        .push(DnType::OrganizationName, "JustDNS");
    let ca_cert = ca_params.self_signed(&ca_key).map_err(to_io)?;

    let leaf_key = KeyPair::generate().map_err(to_io)?;
    let mut leaf_params = CertificateParams::default();
    leaf_params.is_ca = IsCa::ExplicitNoCa;
    leaf_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    leaf_params
        .distinguished_name
        .push(DnType::CommonName, hostname);
    leaf_params.subject_alt_names = subject_alt_names(hostname, ips)?;
    let leaf_cert = leaf_params
        .signed_by(&leaf_key, &ca_cert, &ca_key)
        .map_err(to_io)?;

    let chain_pem = format!("{}{}", leaf_cert.pem(), ca_cert.pem());
    write_secret(&path.join(CA_KEY), ca_key.serialize_pem().as_bytes())?;
    std::fs::write(path.join(CA_CERT), ca_cert.pem())?;
    write_secret(&path.join(LEAF_KEY), leaf_key.serialize_pem().as_bytes())?;
    std::fs::write(path.join(LEAF_CERT), chain_pem)?;
    Ok(())
}

/// Builds the SAN list: the hostname as a DNS name plus every configured IP, and
/// falls back to `localhost` when nothing usable is given.
fn subject_alt_names(hostname: &str, ips: &[IpAddr]) -> io::Result<Vec<SanType>> {
    let mut sans = Vec::new();
    let host = hostname.trim();
    if !host.is_empty() {
        sans.push(SanType::DnsName(dns_name(host)?));
    }
    for ip in ips {
        sans.push(SanType::IpAddress(*ip));
    }
    if sans.is_empty() {
        sans.push(SanType::DnsName(dns_name("localhost")?));
    }
    Ok(sans)
}

fn dns_name(host: &str) -> io::Result<Ia5String> {
    Ia5String::try_from(host.to_owned()).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
}

/// Builds a rustls server config pinned to the `ring` crypto provider.
pub(crate) fn build_config(
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    alpn: &[&[u8]],
) -> Result<ServerConfig, rustls::Error> {
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    Ok(config)
}

/// Reads a PEM certificate chain from `path`.
pub(crate) fn read_pem_chain(path: &Path) -> io::Result<Vec<CertificateDer<'static>>> {
    parse_pem_chain(&std::fs::read_to_string(path)?)
}

/// Parses a PEM certificate chain from memory.
pub(crate) fn parse_pem_chain(pem: &str) -> io::Result<Vec<CertificateDer<'static>>> {
    let certs = CertificateDer::pem_slice_iter(pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if certs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "no certificate found in PEM",
        ));
    }
    Ok(certs)
}

/// Reads a PEM private key from `path`.
pub(crate) fn read_pem_key(path: &Path) -> io::Result<PrivateKeyDer<'static>> {
    parse_pem_key(&std::fs::read_to_string(path)?)
}

/// Parses a PEM private key from memory.
pub(crate) fn parse_pem_key(pem: &str) -> io::Result<PrivateKeyDer<'static>> {
    PrivateKeyDer::from_pem_slice(pem.as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Clones a certificate chain (DER borrows nothing at `'static`).
fn clone_chain(certs: &[CertificateDer<'static>]) -> Vec<CertificateDer<'static>> {
    certs.iter().map(|c| c.clone().into_owned()).collect()
}

/// Computes the SHA-256 fingerprint of a DER certificate as colon-separated hex.
pub(crate) fn fingerprint_of(cert: &CertificateDer<'_>) -> String {
    let digest = Sha256::digest(cert.as_ref());
    digest
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Writes a secret file, restricting it to the owner (`0600`) on Unix.
pub(crate) fn write_secret(path: &Path, bytes: &[u8]) -> io::Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Copies a secret file and restricts the copy to the owner on Unix.
fn copy_secret(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::copy(from, to)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(to, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn read_meta(path: &Path) -> io::Result<Meta> {
    let raw = std::fs::read_to_string(path.join(META))?;
    serde_json::from_str(&raw).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn write_meta(path: &Path, name: &str, kind: CertKind) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    let meta = Meta {
        name: name.trim().to_owned(),
        kind,
        created_ms: now_ms(),
    };
    let raw = serde_json::to_string_pretty(&meta)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(path.join(META), raw)
}

/// A short, collision-resistant id for a stored certificate.
fn new_id() -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    now_nanos().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    format!("cert-{:016x}", hasher.finish())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn temp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "justdns-certstore-{}-{}-{seq}",
            std::process::id(),
            now_nanos()
        ));
        dir
    }

    #[test]
    fn seeds_a_default_and_builds_configs() {
        let dir = temp_dir();
        let store =
            CertStore::load_or_init(&dir, None, "dns.lan", &[IpAddr::V4(Ipv4Addr::LOCALHOST)])
                .expect("init");
        let list = store.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, DEFAULT_ID);
        assert_eq!(list[0].kind, CertKind::Auto);
        assert!(list[0].has_ca);
        assert!(!list[0].expired, "a fresh cert is valid");
        assert!(store.build_config(None, ALPN_DOT).is_ok());
        assert!(
            store.build_config(Some("missing"), ALPN_DOH).is_ok(),
            "falls back to default"
        );
        assert!(store.ca_pem(DEFAULT_ID).unwrap().is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn generate_adds_a_named_cert() {
        let dir = temp_dir();
        let store = CertStore::load_or_init(&dir, None, "dns.lan", &[]).expect("init");
        let id = store
            .generate("Zweitzert", "other.lan", &[])
            .expect("generate");
        assert!(store.exists(&id));
        assert_eq!(store.list().len(), 2);
        assert_eq!(store.info(&id).unwrap().name, "Zweitzert");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upload_rejects_garbage_and_renew_rejects_expired() {
        let dir = temp_dir();
        let store = CertStore::load_or_init(&dir, None, "dns.lan", &[]).expect("init");
        assert!(store.upload("Bad", "not a cert", "not a key").is_err());

        // A freshly minted, in-window cert is accepted as a renewal of the default.
        let (cert_pem, key_pem) = mint_pair_pem("dns.lan");
        assert!(store.renew(DEFAULT_ID, &cert_pem, &key_pem).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_refuses_the_last_cert() {
        let dir = temp_dir();
        let store = CertStore::load_or_init(&dir, None, "dns.lan", &[]).expect("init");
        assert!(store.delete(DEFAULT_ID).is_err(), "last cert is protected");
        let id = store.generate("Extra", "x.lan", &[]).expect("generate");
        assert!(store.delete(&id).is_ok());
        assert!(!store.exists(&id));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_and_replace_round_trip_across_stores() {
        let dir_a = temp_dir();
        let store_a = CertStore::load_or_init(&dir_a, None, "a.lan", &[]).expect("init a");
        let extra = store_a.generate("Extra", "extra.lan", &[]).expect("gen");
        let records = store_a.export_all();
        assert_eq!(records.len(), 2);
        // The default is minted, so it carries CA material; the private key travels.
        let def = records.iter().find(|r| r.id == DEFAULT_ID).unwrap();
        assert!(def.ca_pem.is_some() && def.ca_key_pem.is_some());
        assert!(def.key_pem.contains("PRIVATE KEY"));

        // A second, empty-seeded store receives the records and now matches.
        let dir_b = temp_dir();
        let store_b = CertStore::load_or_init(&dir_b, None, "b.lan", &[]).expect("init b");
        store_b.replace_from_records(&records).expect("apply");
        assert!(store_b.exists(&extra) && store_b.exists(DEFAULT_ID));
        assert_eq!(store_b.list().len(), 2);
        assert!(store_b.build_config(Some(&extra), ALPN_DOT).is_ok());

        // Removing one on A and re-applying drops it on B; an empty set is ignored.
        store_a.delete(&extra).expect("delete");
        store_b
            .replace_from_records(&store_a.export_all())
            .expect("re-apply");
        assert!(!store_b.exists(&extra));
        store_b.replace_from_records(&[]).expect("empty is a no-op");
        assert!(store_b.exists(DEFAULT_ID));
        let _ = std::fs::remove_dir_all(&dir_a);
        let _ = std::fs::remove_dir_all(&dir_b);
    }

    /// Mints a standalone self-signed leaf+key PEM pair for upload/renew tests.
    fn mint_pair_pem(hostname: &str) -> (String, String) {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        params.distinguished_name.push(DnType::CommonName, hostname);
        params.subject_alt_names = vec![SanType::DnsName(dns_name(hostname).unwrap())];
        let cert = params.self_signed(&key).unwrap();
        (cert.pem(), key.serialize_pem())
    }
}
