// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Safe-search enforcement: map a search service's host to the address its
//! provider reserves for the filtered variant.
//!
//! The big providers answer a special host name with a safe-search-only
//! endpoint (Google's `forcesafesearch.google.com`, YouTube's
//! `restrict.youtube.com`, and so on). When safe search is on, the pipeline
//! resolves that target instead of the original name and returns it under a
//! CNAME, exactly as AdGuard Home and Pi-hole do — robust against the endpoints'
//! changing addresses.

use hickory_proto::rr::{Name, RecordType};

/// The safe-search target for YouTube. The strict variant, matching the common
/// AdGuard/Pi-hole default.
const YOUTUBE: &str = "restrict.youtube.com";

/// The safe-search host for `name`, if it is a known search service and the
/// query asks for an address record (A or AAAA). `None` leaves the query to be
/// resolved normally.
///
/// The name is matched case-insensitively and with a leading `www.` ignored, so
/// both `google.com` and `www.google.com` are caught. Google is matched on its
/// first label, so every country domain (`google.de`, `google.co.uk`, …) is
/// covered; the other services are matched against an explicit host list.
pub fn target(name: &Name, qtype: RecordType) -> Option<Name> {
    if qtype != RecordType::A && qtype != RecordType::AAAA {
        return None;
    }

    let host = name.to_ascii();
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let bare = host.strip_prefix("www.").unwrap_or(&host);

    let target = if is_google(bare) {
        "forcesafesearch.google.com"
    } else if is_youtube(bare) {
        YOUTUBE
    } else if bare == "bing.com" {
        "strict.bing.com"
    } else if bare == "duckduckgo.com" || bare == "start.duckduckgo.com" {
        "safe.duckduckgo.com"
    } else {
        return None;
    };

    // The targets are fixed, valid host names, so this parse never fails; fall
    // back to leaving the query alone rather than panicking if that ever breaks.
    // Mark it fully qualified so it encodes as an absolute name in the CNAME.
    Name::from_ascii(target).ok().map(|mut name| {
        name.set_fqdn(true);
        name
    })
}

/// Whether `bare` (already lowercased, without a leading `www.`) is a Google
/// search domain: its first label is `google`, so every country TLD is caught
/// while sub-services such as `mail.google.com` are not.
fn is_google(bare: &str) -> bool {
    bare.split_once('.')
        .is_some_and(|(first, rest)| first == "google" && !rest.is_empty())
}

/// Whether `bare` is one of YouTube's address hosts that honour the restricted
/// endpoint. YouTube needs an explicit list because its API hosts sit under
/// `googleapis.com`.
fn is_youtube(bare: &str) -> bool {
    matches!(
        bare,
        "youtube.com"
            | "m.youtube.com"
            | "music.youtube.com"
            | "youtubei.googleapis.com"
            | "youtube.googleapis.com"
            | "youtube-nocookie.com"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_of(name: &str, qtype: RecordType) -> Option<String> {
        target(&Name::from_ascii(name).unwrap(), qtype)
            .map(|n| n.to_ascii().trim_end_matches('.').to_owned())
    }

    #[test]
    fn google_country_domains_and_www_are_caught() {
        assert_eq!(
            target_of("google.com.", RecordType::A).as_deref(),
            Some("forcesafesearch.google.com")
        );
        assert_eq!(
            target_of("www.google.de.", RecordType::A).as_deref(),
            Some("forcesafesearch.google.com")
        );
        assert_eq!(
            target_of("google.co.uk.", RecordType::AAAA).as_deref(),
            Some("forcesafesearch.google.com")
        );
        // A Google sub-service that is not search is left alone.
        assert_eq!(target_of("mail.google.com.", RecordType::A), None);
    }

    #[test]
    fn youtube_bing_ddg_are_mapped() {
        assert_eq!(
            target_of("www.youtube.com.", RecordType::A).as_deref(),
            Some("restrict.youtube.com")
        );
        assert_eq!(
            target_of("bing.com.", RecordType::A).as_deref(),
            Some("strict.bing.com")
        );
        assert_eq!(
            target_of("duckduckgo.com.", RecordType::A).as_deref(),
            Some("safe.duckduckgo.com")
        );
    }

    #[test]
    fn unknown_hosts_and_non_address_types_are_ignored() {
        assert_eq!(target_of("example.com.", RecordType::A), None);
        // A known host, but a non-address query type, is not rewritten.
        assert_eq!(target_of("google.com.", RecordType::MX), None);
    }
}
