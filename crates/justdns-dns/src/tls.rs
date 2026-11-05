// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Server-side TLS material for the encrypted client-DNS endpoints.
//!
//! JustDNS terminates DNS-over-TLS (port 853) and DNS-over-HTTPS (`/dns-query`)
//! itself, so it needs a certificate and key that its own devices can verify.
//! Two sources are supported:
//!
//! * **Self-signed** (the default): on first start we mint a small local
//!   certificate authority plus a leaf certificate for the configured hostname
//!   and IPs, and write them to the TLS directory. Clients install the CA once
//!   and then trust every certificate JustDNS presents.
//! * **Uploaded**: the operator supplies their own certificate chain and private
//!   key (for example from an internal CA or ACME), which we validate and store.
//!
//! The leaf material is kept on the `/data` volume so it survives image updates
//! and restarts. The active [`rustls::ServerConfig`] is held behind an
//! [`ArcSwap`] so a certificate change takes effect for new connections without
//! rebinding the listeners.

use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arc_swap::ArcSwap;
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, Ia5String, IsCa, KeyPair,
    KeyUsagePurpose, SanType,
};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};
use tracing::info;

/// Where the certificate the clients should trust comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertSource {
    /// JustDNS mints its own local CA and leaf certificate.
    SelfSigned,
    /// The operator uploaded their own certificate chain and key.
    Uploaded,
}

/// ALPN protocol id advertised on the DoT listener (RFC 7858).
const ALPN_DOT: &[&[u8]] = &[b"dot"];
/// ALPN protocol ids advertised on the DoH listener; `h2` must come first so
/// HTTP/2 is preferred when the client supports it (RFC 8484 recommends it).
const ALPN_DOH: &[&[u8]] = &[b"h2", b"http/1.1"];

/// File name of the leaf certificate chain (leaf first, CA appended for the
/// self-signed case so the presented chain is complete).
const LEAF_CERT: &str = "cert.pem";
/// File name of the leaf private key (PKCS#8 PEM, mode `0600`).
const LEAF_KEY: &str = "key.pem";
/// File name of the generated local CA certificate (offered for download).
const CA_CERT: &str = "ca.pem";
/// File name of the generated local CA private key (never leaves the server).
const CA_KEY: &str = "ca-key.pem";

/// Live TLS material for the DoT and DoH listeners.
///
/// Cheap to clone the handle via [`Arc`]; the two [`ServerConfig`]s differ only
/// in their advertised ALPN protocols.
pub struct TlsManager {
    dir: PathBuf,
    dot_config: ArcSwap<ServerConfig>,
    doh_config: ArcSwap<ServerConfig>,
    /// SHA-256 fingerprint of the leaf certificate, hex, colon-separated.
    fingerprint: ArcSwap<String>,
}

impl TlsManager {
    /// Loads the TLS material from `dir`, minting a fresh self-signed chain when
    /// none is present and the source is [`CertSource::SelfSigned`].
    ///
    /// `hostname` and `ips` seed the certificate's subject alternative names so
    /// clients can verify it by the name or address they dial. When the source
    /// is [`CertSource::Uploaded`] but no certificate has been uploaded yet, a
    /// self-signed chain is still minted as a working default — the operator can
    /// replace it later without the endpoints ever starting without a key.
    pub fn load_or_init(
        dir: impl Into<PathBuf>,
        hostname: &str,
        ips: &[IpAddr],
        source: CertSource,
    ) -> io::Result<Arc<Self>> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;

        let leaf = dir.join(LEAF_CERT);
        let key = dir.join(LEAF_KEY);
        if !leaf.is_file() || !key.is_file() {
            // Nothing stored yet. An uploaded source with no upload still needs a
            // usable certificate, so fall back to self-signed until one arrives.
            let _ = source;
            generate_self_signed(&dir, hostname, ips)?;
            info!(dir = %dir.display(), "generated self-signed TLS certificate");
        }

        let certs = read_pem_chain(&leaf)?;
        let fingerprint = fingerprint_of(certs.first().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "certificate chain is empty")
        })?);

        Ok(Arc::new(Self {
            dot_config: ArcSwap::from_pointee(build_from_dir(&dir, ALPN_DOT)?),
            doh_config: ArcSwap::from_pointee(build_from_dir(&dir, ALPN_DOH)?),
            fingerprint: ArcSwap::from_pointee(fingerprint),
            dir,
        }))
    }

    /// The server config for the DoT listener (ALPN `dot`).
    pub fn dot_config(&self) -> Arc<ServerConfig> {
        self.dot_config.load_full()
    }

    /// The server config for the DoH listener (ALPN `h2`, `http/1.1`).
    pub fn doh_config(&self) -> Arc<ServerConfig> {
        self.doh_config.load_full()
    }

    /// SHA-256 fingerprint of the leaf certificate in use (hex, colon-separated,
    /// upper-case), suitable for showing in the UI so an operator can compare it
    /// on the client.
    pub fn fingerprint(&self) -> String {
        self.fingerprint.load().as_str().to_owned()
    }

    /// The local CA certificate in PEM form, if a self-signed chain was minted.
    /// Returns `None` when the material was uploaded (no CA to hand out).
    pub fn ca_pem(&self) -> io::Result<Option<String>> {
        let ca = self.dir.join(CA_CERT);
        match std::fs::read_to_string(&ca) {
            Ok(pem) => Ok(Some(pem)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Mints a fresh self-signed CA and leaf certificate for `hostname`/`ips`,
    /// writes them to the TLS directory, and swaps them in for new connections.
    pub fn regenerate(&self, hostname: &str, ips: &[IpAddr]) -> io::Result<()> {
        generate_self_signed(&self.dir, hostname, ips)?;
        self.reload()?;
        info!("regenerated self-signed TLS certificate");
        Ok(())
    }

    /// Validates and installs an operator-supplied certificate chain and key,
    /// then swaps them in. The previous local CA (if any) is removed, since it no
    /// longer matches the served certificate.
    pub fn install_uploaded(&self, cert_pem: &str, key_pem: &str) -> io::Result<()> {
        // Validate the pair by building a config from it before touching disk.
        let certs = parse_pem_chain(cert_pem)?;
        let key = parse_pem_key(key_pem)?;
        build_config(clone_chain(&certs), clone_key(&key), ALPN_DOT)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        write_secret(&self.dir.join(LEAF_KEY), key_pem.as_bytes())?;
        std::fs::write(self.dir.join(LEAF_CERT), cert_pem.as_bytes())?;
        // A self-signed CA no longer applies to an uploaded certificate.
        let _ = std::fs::remove_file(self.dir.join(CA_CERT));
        let _ = std::fs::remove_file(self.dir.join(CA_KEY));

        self.reload()?;
        info!("installed uploaded TLS certificate");
        Ok(())
    }

    /// Rebuilds both server configs and the fingerprint from the files on disk.
    fn reload(&self) -> io::Result<()> {
        let certs = read_pem_chain(&self.dir.join(LEAF_CERT))?;
        let fingerprint = fingerprint_of(certs.first().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "certificate chain is empty")
        })?);
        self.dot_config
            .store(Arc::new(build_from_dir(&self.dir, ALPN_DOT)?));
        self.doh_config
            .store(Arc::new(build_from_dir(&self.dir, ALPN_DOH)?));
        self.fingerprint.store(Arc::new(fingerprint));
        Ok(())
    }
}

/// Builds a [`ServerConfig`] from the leaf material in `dir` with the given ALPN.
fn build_from_dir(dir: &Path, alpn: &[&[u8]]) -> io::Result<ServerConfig> {
    let certs = read_pem_chain(&dir.join(LEAF_CERT))?;
    let key = read_pem_key(&dir.join(LEAF_KEY))?;
    build_config(certs, key, alpn).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Builds a rustls server config pinned to the `ring` crypto provider.
fn build_config(
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    alpn: &[&[u8]],
) -> Result<ServerConfig, rustls::Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    Ok(config)
}

/// Reads a PEM certificate chain from `path`.
fn read_pem_chain(path: &Path) -> io::Result<Vec<CertificateDer<'static>>> {
    parse_pem_chain(&std::fs::read_to_string(path)?)
}

/// Parses a PEM certificate chain from memory.
fn parse_pem_chain(pem: &str) -> io::Result<Vec<CertificateDer<'static>>> {
    let mut reader = io::BufReader::new(pem.as_bytes());
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "no certificate found in PEM",
        ));
    }
    Ok(certs)
}

/// Reads a PEM private key from `path`.
fn read_pem_key(path: &Path) -> io::Result<PrivateKeyDer<'static>> {
    parse_pem_key(&std::fs::read_to_string(path)?)
}

/// Parses a PEM private key from memory.
fn parse_pem_key(pem: &str) -> io::Result<PrivateKeyDer<'static>> {
    let mut reader = io::BufReader::new(pem.as_bytes());
    rustls_pemfile::private_key(&mut reader)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no private key found in PEM"))
}

/// Clones a certificate chain (the DER type borrows nothing with a `'static`
/// lifetime, so this is a plain deep copy).
fn clone_chain(certs: &[CertificateDer<'static>]) -> Vec<CertificateDer<'static>> {
    certs.iter().map(|c| c.clone().into_owned()).collect()
}

/// Clones a private key by re-parsing it (pki-types keys are not `Clone`).
fn clone_key(key: &PrivateKeyDer<'static>) -> PrivateKeyDer<'static> {
    key.clone_key()
}

/// Computes the SHA-256 fingerprint of a DER certificate as colon-separated hex.
fn fingerprint_of(cert: &CertificateDer<'_>) -> String {
    let digest = Sha256::digest(cert.as_ref());
    digest
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Mints a self-signed local CA plus a leaf certificate for `hostname`/`ips` and
/// writes `ca.pem`, `ca-key.pem`, `cert.pem` (leaf + CA chain) and `key.pem`.
fn generate_self_signed(dir: &Path, hostname: &str, ips: &[IpAddr]) -> io::Result<()> {
    let to_io = io::Error::other;

    // Local certificate authority.
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

    // Leaf certificate for the endpoints, signed by the CA.
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

    // Present the leaf followed by the CA so verification succeeds once the CA is
    // trusted, even on clients that do not keep the CA in a chain store.
    let chain_pem = format!("{}{}", leaf_cert.pem(), ca_cert.pem());
    write_secret(&dir.join(CA_KEY), ca_key.serialize_pem().as_bytes())?;
    std::fs::write(dir.join(CA_CERT), ca_cert.pem())?;
    write_secret(&dir.join(LEAF_KEY), leaf_key.serialize_pem().as_bytes())?;
    std::fs::write(dir.join(LEAF_CERT), chain_pem)?;
    Ok(())
}

/// Builds the SAN list: the hostname as a DNS name plus every configured IP, and
/// each IP literal also as a DNS name so clients that dial by address still
/// verify (some stacks match the address against DNS-name SANs).
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
        // A certificate with no SAN is useless; fall back to localhost.
        sans.push(SanType::DnsName(dns_name("localhost")?));
    }
    Ok(sans)
}

/// Wraps a host string as an IA5 DNS name, mapping the error into `io`.
fn dns_name(host: &str) -> io::Result<Ia5String> {
    Ia5String::try_from(host.to_owned()).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
}

/// Writes a secret file, restricting it to the owner (`0600`) on Unix.
fn write_secret(path: &Path, bytes: &[u8]) -> io::Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn temp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "justdns-tls-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        dir
    }

    #[test]
    fn mints_and_loads_self_signed() {
        let dir = temp_dir();
        let ips = [IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2))];
        let tls = TlsManager::load_or_init(&dir, "dns.lan", &ips, CertSource::SelfSigned)
            .expect("mint self-signed");

        // Both configs are available and a fingerprint was derived.
        assert!(!tls.dot_config().alpn_protocols.is_empty());
        assert_eq!(tls.doh_config().alpn_protocols[0], b"h2");
        assert!(tls.fingerprint().contains(':'));
        assert!(tls.ca_pem().unwrap().is_some());

        // Loading again reuses the stored material (fingerprint stays stable).
        let fp = tls.fingerprint();
        let again = TlsManager::load_or_init(&dir, "dns.lan", &ips, CertSource::SelfSigned)
            .expect("reload");
        assert_eq!(again.fingerprint(), fp);

        // Regeneration produces a different certificate.
        again.regenerate("dns.lan", &ips).expect("regenerate");
        assert_ne!(again.fingerprint(), fp);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_invalid_uploaded_pair() {
        let dir = temp_dir();
        let tls = TlsManager::load_or_init(&dir, "dns.lan", &[], CertSource::SelfSigned)
            .expect("mint self-signed");
        let err = tls.install_uploaded("not a cert", "not a key");
        assert!(err.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
