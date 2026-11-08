// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Second-factor authentication: TOTP (RFC 6238) plus one-time recovery codes.
//!
//! The crypto lives here; the persisted per-user state (the base32 secret, the
//! confirmed flag and the Argon2-hashed recovery codes) lives on [`crate::Mfa`]
//! in the account store. A secret is a shared value a standard authenticator app
//! (Google/Microsoft Authenticator, Aegis, …) also holds; a six-digit code is
//! derived from it and the current 30-second window. Recovery codes are a
//! fallback for a lost device — each works once.

use argon2::password_hash::rand_core::{OsRng, RngCore};
use totp_rs::{Algorithm, Secret, TOTP};

/// How many recovery codes to mint per enrolment.
pub const RECOVERY_CODE_COUNT: usize = 10;
/// Characters in one recovery code (unambiguous base32 alphabet, no 0/1/8/9).
const RECOVERY_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ234567";
const RECOVERY_CODE_LEN: usize = 10;

/// Build a TOTP verifier from a stored base32 secret. SHA-1, 6 digits, 30-second
/// step, and a skew of 1 so a code from the adjacent window still passes (clock
/// drift). `issuer`/`account` only shape the enrolment URI, not the codes.
fn totp_for(secret: &str, issuer: &str, account: &str) -> Result<TOTP, String> {
    let bytes = Secret::Encoded(secret.to_owned())
        .to_bytes()
        .map_err(|e| format!("bad TOTP secret: {e}"))?;
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        bytes,
        Some(issuer.to_owned()),
        account.to_owned(),
    )
    .map_err(|e| format!("cannot build TOTP: {e}"))
}

/// A fresh random base32 TOTP secret (160 bits), ready to store and to show the
/// user for manual entry.
pub fn generate_secret() -> String {
    Secret::generate_secret().to_encoded().to_string()
}

/// The `otpauth://` enrolment URI for a secret, which the authenticator app
/// reads from the QR code. `issuer` labels the account in the app.
pub fn otpauth_uri(secret: &str, issuer: &str, account: &str) -> Result<String, String> {
    Ok(totp_for(secret, issuer, account)?.get_url())
}

/// Whether `code` is a valid TOTP for `secret` right now (±1 window).
pub fn verify_totp(secret: &str, code: &str) -> bool {
    match totp_for(secret, "JustDNS", "user") {
        Ok(totp) => totp.check_current(code.trim()).unwrap_or(false),
        Err(_) => false,
    }
}

/// Render an `otpauth://` URI as a scannable QR code in SVG, so the secret stays
/// server-side.
pub fn qr_svg(uri: &str) -> Result<String, String> {
    use qrcode::render::svg;
    use qrcode::{EcLevel, QrCode};

    let code = QrCode::with_error_correction_level(uri.as_bytes(), EcLevel::M)
        .map_err(|e| format!("cannot build QR: {e}"))?;
    let svg = code
        .render::<svg::Color>()
        .min_dimensions(200, 200)
        .quiet_zone(true)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build();
    Ok(svg)
}

/// Mint `RECOVERY_CODE_COUNT` fresh recovery codes in plaintext, grouped as
/// `XXXXX-XXXXX` for readability. Shown to the user once; only their hashes are
/// stored.
pub fn generate_recovery_codes() -> Vec<String> {
    (0..RECOVERY_CODE_COUNT)
        .map(|_| one_recovery_code())
        .collect()
}

fn one_recovery_code() -> String {
    let mut out = String::with_capacity(RECOVERY_CODE_LEN + 1);
    for i in 0..RECOVERY_CODE_LEN {
        if i == RECOVERY_CODE_LEN / 2 {
            out.push('-');
        }
        let mut byte = [0u8; 1];
        OsRng.fill_bytes(&mut byte);
        let idx = (byte[0] as usize) % RECOVERY_ALPHABET.len();
        out.push(RECOVERY_ALPHABET[idx] as char);
    }
    out
}

/// Normalise a recovery code the user typed (case, spaces, dashes) before
/// hashing or comparison, so "abcde fghij" matches the stored "ABCDE-FGHIJ".
pub fn normalise_recovery(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_roundtrips_through_a_uri() {
        let secret = generate_secret();
        let uri = otpauth_uri(&secret, "JustDNS", "alice").unwrap();
        assert!(uri.starts_with("otpauth://totp/"));
        assert!(uri.contains("JustDNS"));
    }

    #[test]
    fn a_generated_code_verifies_and_a_wrong_one_does_not() {
        let secret = generate_secret();
        let totp = totp_for(&secret, "JustDNS", "alice").unwrap();
        let now = totp.generate_current().unwrap();
        assert!(verify_totp(&secret, &now));
        assert!(!verify_totp(&secret, "000000"));
        assert!(!verify_totp("not-base32!!", &now));
    }

    #[test]
    fn recovery_codes_are_distinct_and_normalise() {
        let codes = generate_recovery_codes();
        assert_eq!(codes.len(), RECOVERY_CODE_COUNT);
        let norm: std::collections::HashSet<_> =
            codes.iter().map(|c| normalise_recovery(c)).collect();
        assert_eq!(norm.len(), RECOVERY_CODE_COUNT, "codes are unique");
        assert_eq!(normalise_recovery("abcde-fghij"), "ABCDEFGHIJ");
        assert_eq!(normalise_recovery("ABCDE FGHIJ"), "ABCDEFGHIJ");
    }

    #[test]
    fn qr_svg_is_svg() {
        let uri = otpauth_uri(&generate_secret(), "JustDNS", "alice").unwrap();
        let svg = qr_svg(&uri).unwrap();
        assert!(svg.contains("<svg"));
    }
}
