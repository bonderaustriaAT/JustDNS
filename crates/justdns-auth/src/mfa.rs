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

use totp_rs::{Builder, Secret, Totp};

/// How many recovery codes to mint per enrolment.
pub const RECOVERY_CODE_COUNT: usize = 10;
/// Characters in one recovery code (unambiguous base32 alphabet, no 0/1/8/9).
const RECOVERY_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ234567";
const RECOVERY_CODE_LEN: usize = 10;

/// Build a TOTP verifier from a stored base32 secret. SHA-1, 6 digits, 30-second
/// step, and a skew of 1 so a code from the adjacent window still passes (clock
/// drift). `issuer`/`account` only shape the enrolment URI, not the codes.
fn totp_for(secret: &str, issuer: &str, account: &str) -> Result<Totp, String> {
    let secret = Secret::try_from_base32(secret).map_err(|e| format!("bad TOTP secret: {e}"))?;
    // `Builder::new` starts from the RFC 6238 defaults (SHA-1, 6 digits, skew 1,
    // 30-second step), which is exactly what we want, so only the secret and the
    // enrolment labels need setting.
    Builder::new()
        .with_secret(secret)
        .with_issuer(Some(issuer))
        .with_account_name(account)
        .build()
        .map_err(|e| format!("cannot build TOTP: {e}"))
}

/// Seconds since the Unix epoch. totp-rs is built without its `std` feature, so
/// the current time is passed in explicitly rather than read by the library.
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A fresh random base32 TOTP secret (160 bits), ready to store and to show the
/// user for manual entry.
pub fn generate_secret() -> String {
    Secret::generate().to_base32()
}

/// The `otpauth://` enrolment URI for a secret, which the authenticator app
/// reads from the QR code. `issuer` labels the account in the app.
pub fn otpauth_uri(secret: &str, issuer: &str, account: &str) -> Result<String, String> {
    totp_for(secret, issuer, account)?
        .to_url()
        .map_err(|e| format!("cannot build otpauth URL: {e}"))
}

/// The matched time-step when `code` is a valid TOTP for `secret` right now (±1
/// window), or `None` when it is not. The step is returned so the caller can
/// reject a replay: RFC 6238 §5.2 requires a code be accepted only once, and
/// totp-rs leaves that to the caller. A later accepted step must be strictly
/// greater than the last one stored for the user.
pub fn verify_totp_step(secret: &str, code: &str) -> Option<u64> {
    totp_for(secret, "JustDNS", "user")
        .ok()?
        .check(code.trim(), now_unix())
}

/// Whether `code` is a valid TOTP for `secret` right now (±1 window). Carries no
/// replay protection on its own — use [`verify_totp_step`] and track the step for
/// anything that authenticates a login.
pub fn verify_totp(secret: &str, code: &str) -> bool {
    verify_totp_step(secret, code).is_some()
}

/// The current valid TOTP code for `secret`, for tests that drive the login flow.
#[cfg(test)]
pub(crate) fn current_code(secret: &str) -> String {
    totp_for(secret, "JustDNS", "user")
        .unwrap()
        .generate(now_unix())
        .to_string()
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
        out.push(RECOVERY_ALPHABET[uniform_index(RECOVERY_ALPHABET.len())] as char);
    }
    out
}

/// A uniformly distributed index in `0..len` from OS randomness. Rejection
/// sampling drops the top of the byte range that would otherwise fold unevenly
/// onto `len` (`256 % len` values), so every alphabet character is equally
/// likely — a plain `byte % len` would bias the first `256 % len` characters.
fn uniform_index(len: usize) -> usize {
    debug_assert!((1..=256).contains(&len));
    let limit = (256 / len) * len; // largest multiple of len that fits in a u8 range
    loop {
        let mut byte = [0u8; 1];
        getrandom::fill(&mut byte).expect("OS randomness unavailable");
        let value = byte[0] as usize;
        if value < limit {
            return value % len;
        }
    }
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
        let code = totp.generate(now_unix()).to_string();
        assert!(verify_totp(&secret, &code));
        assert!(!verify_totp(&secret, "000000"));
        assert!(!verify_totp("not-base32!!", "123456"));
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
    fn uniform_index_stays_in_range() {
        for _ in 0..1000 {
            let idx = uniform_index(RECOVERY_ALPHABET.len());
            assert!(idx < RECOVERY_ALPHABET.len());
        }
    }

    #[test]
    fn qr_svg_is_svg() {
        let uri = otpauth_uri(&generate_secret(), "JustDNS", "alice").unwrap();
        let svg = qr_svg(&uri).unwrap();
        assert!(svg.contains("<svg"));
    }
}
