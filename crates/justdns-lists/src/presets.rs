// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The curated catalogue of one-click community lists.
//!
//! These are Hagezi's DNS blocklists, the most widely used ready-made lists for
//! AdGuard Home and Pi-hole. Each entry is described in plain language — what it
//! blocks and, just as important, how likely it is to break something the user
//! wants (its false-positive risk) — so enabling one is an informed choice, not
//! a leap of faith.
//!
//! The URLs point at the raw files on the Hagezi repository's `main` branch,
//! using the `wildcard/…-onlydomains.txt` variants: plain one-domain-per-line
//! lists that JustDNS's parser reads directly, and the smallest form Hagezi
//! ships (so the in-memory sets stay as lean as the list allows). jsDelivr is
//! deliberately avoided — the whole Hagezi repository is far over jsDelivr's
//! per-package size limit, so its CDN refuses these files. The URLs are
//! compiled in, not persisted: a stored subscription only records the preset's
//! key, so the catalogue can move without rewriting anyone's saved lists.

use serde::Serialize;

use crate::ListKind;

/// Raw base for the Hagezi `wildcard/` lists on the `main` branch; every preset
/// URL is expected to live under this prefix.
#[cfg(test)]
const HAGEZI: &str = "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard";

/// One entry in the curated catalogue: a ready-made list the user can switch on
/// with a single click, with enough description to judge it.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    /// Stable machine key; also the id of the subscription it creates.
    pub key: &'static str,
    /// Short display name.
    pub name: &'static str,
    /// Where the list is fetched from.
    pub url: &'static str,
    /// Whether the list allows or blocks the names on it.
    pub kind: ListKind,
    /// What the list is for, in plain language.
    pub description: &'static str,
    /// How likely it is to block something wanted, so the user can weigh it.
    pub false_positive: &'static str,
    /// The false-positive risk on a 1 (negligible) to 5 (high) scale, so the
    /// dashboard can show it as a meter next to the prose.
    pub fp_level: u8,
}

/// The whole catalogue, ordered from the gentlest list to the most aggressive.
pub fn presets() -> &'static [Preset] {
    PRESETS
}

/// Look up a preset by its key.
pub fn preset(key: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|preset| preset.key == key)
}

static PRESETS: &[Preset] = &[
    Preset {
        key: "hagezi-light",
        name: "Hagezi Light",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/light-onlydomains.txt",
        kind: ListKind::Block,
        description: "Die schlanke Einsteigerliste: blockt die verbreitetsten Werbe- und \
Tracking-Domains, bewusst zurückhaltend gehalten. Guter Start, wenn bisher gar nichts geblockt \
wurde.",
        false_positive: "Sehr gering. Für den Alltag die sicherste Wahl — hier geht praktisch \
nie etwas Gewünschtes verloren.",
        fp_level: 1,
    },
    Preset {
        key: "hagezi-multi",
        name: "Hagezi Normal",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/multi-onlydomains.txt",
        kind: ListKind::Block,
        description: "Die empfohlene Standardliste: Werbung, Tracker, Telemetrie und bekannte \
Badware in einem ausgewogenen Paket. Für die meisten Haushalte die richtige Wahl.",
        false_positive: "Gering. Gelegentlich kann ein Tracking-Dienst betroffen sein, den eine \
Website zum Laden braucht — im Zweifel die Domain auf die eigene Allowlist setzen.",
        fp_level: 2,
    },
    Preset {
        key: "hagezi-pro",
        name: "Hagezi Pro",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/pro-onlydomains.txt",
        kind: ListKind::Block,
        description: "Umfangreicher als Normal: zusätzlich mehr Telemetrie und aufdringliche \
Dienste. Für Nutzer, die bewusst strenger filtern wollen und kleine Korrekturen in Kauf nehmen.",
        false_positive: "Mittel. Vereinzelt werden Funktionen von Apps oder Websites \
beeinträchtigt; rechne damit, hin und wieder etwas freizugeben.",
        fp_level: 3,
    },
    Preset {
        key: "hagezi-pro-plus",
        name: "Hagezi Pro++",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/pro.plus-onlydomains.txt",
        kind: ListKind::Block,
        description: "Aggressive Variante von Pro: blockt zusätzlich viele \
Hersteller-Telemetrie, Smart-TV- und App-Tracking. Für erfahrene Nutzer, die maximalen \
Datenschutz über Bequemlichkeit stellen.",
        false_positive: "Erhöht. Einige Dienste (etwa Push-Nachrichten oder Login über Dritte) \
können ausfallen. Nur mit Bereitschaft zur Pflege der Allowlist einsetzen.",
        fp_level: 4,
    },
    Preset {
        key: "hagezi-ultimate",
        name: "Hagezi Ultimate",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/ultimate-onlydomains.txt",
        kind: ListKind::Block,
        description: "Die kompromissloseste Liste: alles aus Pro++ plus weitere Telemetrie und \
Nerv-Domains, hunderttausende Einträge. Maximale Filterung ohne Rücksicht auf Komfort.",
        false_positive: "Hoch. Rechne mit spürbaren Nebenwirkungen und regelmäßiger Pflege der \
Allowlist. Nicht für ein Netz empfohlen, in dem andere mitnutzen.",
        fp_level: 5,
    },
    Preset {
        key: "hagezi-tif",
        name: "Hagezi Threat Intelligence",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/tif.medium-onlydomains.txt",
        kind: ListKind::Block,
        description: "Reine Sicherheitsliste (Variante Medium): bekannte Phishing-, Malware-, \
Betrugs- und Krypto-Miner-Domains aus laufend gepflegten Bedrohungsquellen. Ergänzt die \
Werbe-Listen um echten Schutz, ohne die riesige Vollversion in den Speicher zu laden.",
        false_positive: "Gering und zielt nur auf Schädliches. Lässt sich gefahrlos zusätzlich \
zu einer Werbeliste aktivieren.",
        fp_level: 1,
    },
    Preset {
        key: "hagezi-doh-vpn",
        name: "Hagezi DoH/VPN-Bypass",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/doh-vpn-proxy-bypass-onlydomains.txt",
        kind: ListKind::Block,
        description: "Blockt öffentliche DNS-over-HTTPS-, VPN- und Proxy-Server, mit denen sich \
Geräte im Netz an JustDNS vorbeimogeln könnten. Hält den Filter auch dort durchsetzbar, wo Apps \
eigenes DNS mitbringen.",
        false_positive: "Mittel. Wer selbst einen DoH-Dienst oder ein VPN nutzen will, muss \
dessen Domain freigeben — sonst funktioniert es nicht mehr.",
        fp_level: 3,
    },
    Preset {
        key: "hagezi-native-apple",
        name: "Hagezi Apple-Telemetrie",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/native.apple-onlydomains.txt",
        kind: ListKind::Block,
        description: "Blockt die Telemetrie-Domains, die Apple-Geräte (iPhone, iPad, Mac) im \
Hintergrund kontaktieren. Gezielt auf native Apple-Dienste zugeschnitten.",
        false_positive: "Gering bis mittel. Einzelne Apple-Komfortfunktionen können betroffen \
sein; der Großteil von iOS/macOS läuft unverändert weiter.",
        fp_level: 2,
    },
    Preset {
        key: "hagezi-referral-allow",
        name: "Hagezi Referral-Freigabe",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/whitelist-referral-onlydomains.txt",
        kind: ListKind::Allow,
        description: "Eine Freigabeliste (keine Sperrliste): gibt Referral- und \
Gutschein-Domains frei, die von den strengeren Sperrlisten mitgeblockt werden und sonst \
Partnerlinks oder Cashback-Dienste lahmlegen. Sinnvoll zusätzlich zu Pro oder Pro++.",
        false_positive: "Diese Liste erlaubt gezielt — sie lässt einige Tracking-nahe \
Referral-Dienste wieder durch. Nur aktivieren, wenn solche Links tatsächlich gebraucht werden.",
        fp_level: 1,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_urls_are_hagezi_https() {
        let mut keys = std::collections::HashSet::new();
        for preset in presets() {
            assert!(
                keys.insert(preset.key),
                "duplicate preset key {}",
                preset.key
            );
            assert!(
                preset.url.starts_with(HAGEZI),
                "{} off-catalogue",
                preset.key
            );
            assert!(!preset.description.is_empty());
            assert!(!preset.false_positive.is_empty());
            assert!((1..=5).contains(&preset.fp_level));
        }
    }

    #[test]
    fn lookup_by_key() {
        assert!(preset("hagezi-tif").is_some());
        assert!(preset("nope").is_none());
    }
}
