// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The curated catalogue of one-click community lists.
//!
//! Most are Hagezi's DNS blocklists, the most widely used ready-made lists for
//! AdGuard Home and Pi-hole, alongside the JustDNS-maintained DACH supplement for
//! German-language reach-measurement trackers. Each entry is described in plain
//! language — what it blocks and, just as important, how likely it is to break
//! something the user wants (its false-positive risk) — so enabling one is an
//! informed choice, not a leap of faith.
//!
//! Most URLs point at the raw files on the Hagezi repository's `main` branch,
//! using the `wildcard/…-onlydomains.txt` variants: plain one-domain-per-line
//! lists that JustDNS's parser reads directly, and the smallest form Hagezi
//! ships (so the in-memory sets stay as lean as the list allows). The DACH
//! supplement follows the same plain one-domain-per-line form from its own
//! raw-GitHub file. jsDelivr is deliberately avoided — the whole Hagezi
//! repository is far over jsDelivr's per-package size limit, so its CDN refuses
//! these files. The URLs are compiled in, not persisted: a stored subscription
//! only records the preset's key, so the catalogue can move without rewriting
//! anyone's saved lists.
//!
//! Besides Hagezi the catalogue carries a few other widely used lists (OISD,
//! StevenBlack, Frogeye, malware-filter, Phishing Army), each in a format the
//! parser reads directly: plain domains or hosts-file lines.

use serde::Serialize;

use crate::ListKind;

/// The exact prefixes a catalogue URL may live under: the Hagezi
/// `wildcard/` lists, the JustDNS-maintained lists and the other curated sources. Pinning the full repo
/// path (not just the host) keeps a mistyped repo or a stray personal fork out of
/// the catalogue, which matters because these lists are fetched and applied on
/// every instance. The URLs are fetched by key, not persisted.
#[cfg(test)]
const TRUSTED_PREFIXES: &[&str] = &[
    "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/",
    "https://raw.githubusercontent.com/hagezi/nrd/main/domains/",
    "https://raw.githubusercontent.com/bonderaustriaAT/justdns-blocklist/main/",
    "https://raw.githubusercontent.com/bonderaustriaAT/justdns-threat-ips/main/",
    "https://raw.githubusercontent.com/StevenBlack/hosts/master/",
    "https://big.oisd.nl/",
    "https://small.oisd.nl/",
    "https://hostfiles.frogeye.fr/",
    "https://malware-filter.gitlab.io/malware-filter/",
    "https://phishing.army/download/",
];

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
    /// Coarse grouping for the dashboard's catalogue filter: one of `ads`,
    /// `security`, `privacy`, `bypass` or `allow`.
    pub category: &'static str,
    /// Whether JustDNS marks this list as a recommended starting choice. The
    /// dashboard shows a badge on it so a new user can pick a sensible default
    /// without weighing every entry. A category carries one general default plus
    /// at most one regional supplement (e.g. the DACH list alongside the general
    /// ads pick), so no category shows more than two badges.
    pub recommended: bool,
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
        category: "ads",
        recommended: false,
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
        category: "ads",
        recommended: true,
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
        category: "ads",
        recommended: false,
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
        category: "ads",
        recommended: false,
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
        category: "ads",
        recommended: false,
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
        key: "oisd-small",
        category: "ads",
        recommended: false,
        name: "OISD Small",
        url: "https://small.oisd.nl/domainswild2",
        kind: ListKind::Block,
        description: "Die kompakte OISD-Liste nach dem Motto „Block. Don't break.“: \
Werbung, Tracking und Malware, kuratiert aus vielen Quellen und auf möglichst wenige \
Nebenwirkungen getrimmt. Eine schlanke Alternative zu Hagezi Light.",
        false_positive: "Sehr gering. OISD entfernt Fehlalarme konsequent, im Alltag fällt die \
Liste praktisch nicht auf.",
        fp_level: 1,
    },
    Preset {
        key: "oisd-big",
        category: "ads",
        recommended: false,
        name: "OISD Big",
        url: "https://big.oisd.nl/domainswild2",
        kind: ListKind::Block,
        description: "Die volle OISD-Liste: deutlich mehr Werbe-, Tracking-, Malware- und \
Phishing-Domains als Small, weiterhin mit dem Anspruch, nichts kaputt zu machen. Eine der \
meistgenutzten Listen für Pi-hole und AdGuard Home, vergleichbar mit Hagezi Normal bis Pro.",
        false_positive: "Gering. Fehlalarme werden schnell bereinigt; selten muss eine Domain \
freigegeben werden.",
        fp_level: 2,
    },
    Preset {
        key: "stevenblack-unified",
        category: "ads",
        recommended: false,
        name: "StevenBlack Unified",
        url: "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts",
        kind: ListKind::Block,
        description: "Der Klassiker unter den hosts-Dateien: Werbung und Malware aus mehreren \
etablierten Quellen zusammengeführt (Basisvariante ohne Erweiterungen). Gut gepflegt und seit \
Jahren weit verbreitet.",
        false_positive: "Gering. Konservativ zusammengestellt, gelegentlich trifft es einen \
Tracking-Link aus Newslettern.",
        fp_level: 2,
    },
    Preset {
        key: "hagezi-popupads",
        category: "ads",
        recommended: false,
        name: "Hagezi Pop-Up Ads",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/popupads-onlydomains.txt",
        kind: ListKind::Block,
        description: "Spezialliste gegen Pop-up-, Pop-under- und Weiterleitungs-Werbung, wie \
sie vor allem auf Streaming- und Download-Seiten auftaucht. Ergänzung zu einer allgemeinen \
Werbeliste.",
        false_positive: "Sehr gering. Zielt nur auf aggressive Werbenetzwerke ohne \
Seitenfunktion.",
        fp_level: 1,
    },
    Preset {
        key: "hagezi-tif",
        category: "security",
        recommended: true,
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
        key: "malware-filter-urlhaus",
        category: "security",
        recommended: false,
        name: "URLhaus Malware-Domains",
        url: "https://malware-filter.gitlab.io/malware-filter/urlhaus-filter-domains.txt",
        kind: ListKind::Block,
        description: "Domains, über die aktuell Malware verteilt wird, aus der URLhaus-Datenbank \
von abuse.ch (aufbereitet von malware-filter, alle 12 Stunden neu). Klein, schnell \
aktualisiert und rein auf Schadsoftware fokussiert.",
        false_positive: "Sehr gering. Gelistet werden nur aktiv missbrauchte Domains; große \
Hoster sind bewusst ausgenommen.",
        fp_level: 1,
    },
    Preset {
        key: "malware-filter-phishing",
        category: "security",
        recommended: false,
        name: "Phishing Filter",
        url: "https://malware-filter.gitlab.io/malware-filter/phishing-filter-domains.txt",
        kind: ListKind::Block,
        description: "Phishing-Domains aus OpenPhish, PhishTank und weiteren Quellen \
(malware-filter, alle 12 Stunden neu). Schützt vor gefälschten Login- und Bankseiten.",
        false_positive: "Sehr gering. Enthält nur gemeldete Phishing-Seiten.",
        fp_level: 1,
    },
    Preset {
        key: "phishing-army",
        category: "security",
        recommended: false,
        name: "Phishing Army",
        url: "https://phishing.army/download/phishing_army_blocklist.txt",
        kind: ListKind::Block,
        description: "Große Community-Liste gegen Phishing, zusammengeführt aus PhishTank, \
OpenPhish, CERT.PL und weiteren Quellen, mehrmals täglich aktualisiert (CC BY-NC 4.0). \
Breitere Abdeckung als der Phishing Filter.",
        false_positive: "Gering. Durch die Größe kann vereinzelt eine bereits bereinigte Seite \
noch gelistet sein.",
        fp_level: 2,
    },
    Preset {
        key: "hagezi-fake",
        category: "security",
        recommended: false,
        name: "Hagezi Fake",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/fake-onlydomains.txt",
        kind: ListKind::Block,
        description: "Fake-Shops, Abo-Fallen, betrügerische Trading-Plattformen und andere \
Betrugsseiten. Ergänzt Threat Intelligence gezielt um Online-Betrug.",
        false_positive: "Sehr gering. Zielt nur auf nachweislich betrügerische Angebote.",
        fp_level: 1,
    },
    Preset {
        key: "hagezi-doh-vpn",
        category: "bypass",
        recommended: false,
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
        category: "privacy",
        recommended: false,
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
        key: "hagezi-native-winoffice",
        category: "privacy",
        recommended: false,
        name: "Hagezi Windows/Office-Telemetrie",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/native.winoffice-onlydomains.txt",
        kind: ListKind::Block,
        description: "Blockt die Telemetrie- und Tracking-Domains von Windows und Microsoft \
Office. Updates, Store und Anmeldung bleiben erreichbar.",
        false_positive: "Gering bis mittel. Einzelne Diagnose- oder Feedback-Funktionen von \
Windows können ausfallen.",
        fp_level: 2,
    },
    Preset {
        key: "hagezi-native-samsung",
        category: "privacy",
        recommended: false,
        name: "Hagezi Samsung-Telemetrie",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/native.samsung-onlydomains.txt",
        kind: ListKind::Block,
        description: "Blockt Telemetrie und Werbe-Tracking von Samsung-Geräten (Smartphones, \
Smart-TVs, Haushaltsgeräte).",
        false_positive: "Gering bis mittel. Einzelne Samsung-Dienste wie Werbe-Kacheln auf dem \
TV verschwinden, die Grundfunktionen bleiben.",
        fp_level: 2,
    },
    Preset {
        key: "hagezi-native-lgwebos",
        category: "privacy",
        recommended: false,
        name: "Hagezi LG webOS-Telemetrie",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/native.lgwebos-onlydomains.txt",
        kind: ListKind::Block,
        description: "Blockt Telemetrie, Werbung und Zuschauer-Tracking (ACR) von LG-Smart-TVs \
mit webOS.",
        false_positive: "Gering bis mittel. Werbe- und Empfehlungsflächen im TV-Menü können \
leer bleiben.",
        fp_level: 2,
    },
    Preset {
        key: "hagezi-native-amazon",
        category: "privacy",
        recommended: false,
        name: "Hagezi Amazon-Telemetrie",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/native.amazon-onlydomains.txt",
        kind: ListKind::Block,
        description: "Blockt Telemetrie und Werbe-Tracking von Amazon-Geräten wie Fire TV, \
Echo und Kindle.",
        false_positive: "Mittel. Werbung im Fire-TV-Menü verschwindet, vereinzelt reagieren \
Amazon-Apps eingeschränkt.",
        fp_level: 3,
    },
    Preset {
        key: "hagezi-native-xiaomi",
        category: "privacy",
        recommended: false,
        name: "Hagezi Xiaomi-Telemetrie",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/native.xiaomi-onlydomains.txt",
        kind: ListKind::Block,
        description: "Blockt Telemetrie und Werbung von Xiaomi-, Redmi- und POCO-Geräten \
(MIUI/HyperOS).",
        false_positive: "Gering bis mittel. System-Werbung verschwindet, Cloud-Funktionen \
bleiben in der Regel erhalten.",
        fp_level: 2,
    },
    Preset {
        key: "hagezi-native-tiktok",
        category: "privacy",
        recommended: false,
        name: "Hagezi TikTok-Tracking",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/native.tiktok-onlydomains.txt",
        kind: ListKind::Block,
        description: "Blockt die Analyse- und Tracking-Domains von TikTok bzw. ByteDance, \
die App selbst bleibt nutzbar.",
        false_positive: "Gering. Gedacht für Netze, in denen TikTok genutzt, aber weniger \
mitgeschnitten werden soll.",
        fp_level: 2,
    },
    Preset {
        key: "frogeye-firstparty",
        category: "privacy",
        recommended: false,
        name: "Frogeye First-Party-Tracker",
        url: "https://hostfiles.frogeye.fr/firstparty-trackers.txt",
        kind: ListKind::Block,
        description: "Tracker, die sich per CNAME als Subdomain der besuchten Website tarnen \
und so klassische Tracking-Blocker umgehen. Ergänzt das CNAME-Uncloaking von JustDNS um eine \
fertige Liste bekannter Fälle.",
        false_positive: "Gering. Es werden nur Subdomains gelistet, die nachweislich auf \
Tracking-Anbieter zeigen.",
        fp_level: 2,
    },
    Preset {
        key: "justdns-dach",
        category: "ads",
        recommended: true,
        name: "JustDNS DACH",
        url: "https://raw.githubusercontent.com/bonderaustriaAT/justdns-blocklist/main/dach-ads-trackers.txt",
        kind: ListKind::Block,
        description: "Die hauseigene DACH-Ergänzung: deutschsprachige Reichweitenmessung und \
Tracking, die internationale Listen erfahrungsgemäß schwächer abdecken (INFOnline/IVW, VG Wort \
METIS, ÖWA). Jede Domain einzeln geprüft — als Zusatz zu Hagezi Normal gedacht, nicht als Ersatz.",
        false_positive: "Sehr gering. Bewusst nur reine Mess-Beacons ohne Seitenfunktion; im \
Zweifel bleibt eine Domain draußen.",
        fp_level: 1,
    },
    Preset {
        key: "justdns-threat-ips-v4",
        category: "threatip",
        recommended: true,
        name: "JustDNS Threat-IPs",
        url: "https://raw.githubusercontent.com/bonderaustriaAT/justdns-threat-ips/main/threat-ips-v4.txt",
        kind: ListKind::BlockIp,
        description: "Keine Domain-, sondern eine IP-Blockliste: prueft die Antwort-Adressen (A-Records) \
des Resolvers. Zeigt eine Antwort auf eine bekannte Schad-IP (C2, Dropper, Malware aus unserem \
Honeypot-Netz), wird die ganze Antwort verworfen. Greift auch dort, wo die Domain selbst noch nicht \
gelistet ist. Nur IPv4, Einzel-IPs und CIDR.",
        false_positive: "Gering. Die IPs stammen aus laufend gepflegter Honeypot-Beobachtung; ein \
geteilter Hoster-Range kann theoretisch Mitlaeufer treffen, daher nur bestaetigte Schad-IPs.",
        fp_level: 2,
    },
    Preset {
        key: "nrd-7",
        category: "nrd",
        recommended: true,
        name: "Neu registrierte Domains: letzte 7 Tage",
        url: "https://raw.githubusercontent.com/hagezi/nrd/main/domains/nrd7.txt",
        kind: ListKind::Warn,
        description: "Domains der letzten 7 Tage (HaGeZi, GPLv3). Kleinstes Fenster, rund 40 MB.",
        false_positive: "Als Warnung gedacht. Zum Blockieren nur mit Bedacht: viele neue Domains \
sind harmlos.",
        fp_level: 4,
    },
    Preset {
        key: "nrd-14",
        category: "nrd",
        recommended: false,
        name: "Neu registrierte Domains: Tage 8 bis 14",
        url: "https://raw.githubusercontent.com/hagezi/nrd/main/domains/nrd14-8.txt",
        kind: ListKind::Warn,
        description: "Tage 8 bis 14 (HaGeZi, GPLv3). Mit der 7-Tage-Liste zusammen 14 Tage. \
Rund 46 MB.",
        false_positive: "Als Warnung unkritisch; beim Blockieren hohe Fehlalarmquote.",
        fp_level: 4,
    },
    Preset {
        key: "nrd-21",
        category: "nrd",
        recommended: false,
        name: "Neu registrierte Domains: Tage 15 bis 21",
        url: "https://raw.githubusercontent.com/hagezi/nrd/main/domains/nrd21-15.txt",
        kind: ListKind::Warn,
        description: "Tage 15 bis 21 (HaGeZi, GPLv3). Breitere Abdeckung, mehr Speicher.",
        false_positive: "Als Warnung unkritisch; beim Blockieren hohe Fehlalarmquote.",
        fp_level: 4,
    },
    Preset {
        key: "nrd-28",
        category: "nrd",
        recommended: false,
        name: "Neu registrierte Domains: Tage 22 bis 28",
        url: "https://raw.githubusercontent.com/hagezi/nrd/main/domains/nrd28-22.txt",
        kind: ListKind::Warn,
        description: "Tage 22 bis 28 (HaGeZi, GPLv3). Zusammen rund 28 Tage. Viel Speicher nötig.",
        false_positive: "Als Warnung unkritisch; beim Blockieren hohe Fehlalarmquote.",
        fp_level: 4,
    },
    Preset {
        key: "hagezi-referral-allow",
        category: "allow",
        recommended: false,
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
    fn keys_are_unique_and_urls_are_trusted_raw_https() {
        let mut keys = std::collections::HashSet::new();
        for preset in presets() {
            assert!(
                keys.insert(preset.key),
                "duplicate preset key {}",
                preset.key
            );
            assert!(
                TRUSTED_PREFIXES
                    .iter()
                    .any(|prefix| preset.url.starts_with(prefix)),
                "{} is not under a trusted raw-GitHub prefix",
                preset.key
            );
            assert!(!preset.description.is_empty());
            assert!(!preset.false_positive.is_empty());
            assert!((1..=5).contains(&preset.fp_level));
            assert!(
                [
                    "ads", "security", "privacy", "bypass", "nrd", "allow", "threatip"
                ]
                .contains(&preset.category),
                "{} has an unknown category {}",
                preset.key,
                preset.category
            );
        }
    }

    #[test]
    fn lookup_by_key() {
        assert!(preset("hagezi-tif").is_some());
        assert!(preset("nope").is_none());
    }

    #[test]
    fn recommended_badge_is_the_curated_default_set() {
        let recommended: std::collections::HashSet<&str> = presets()
            .iter()
            .filter(|preset| preset.recommended)
            .map(|preset| preset.key)
            .collect();
        assert_eq!(
            recommended,
            [
                "hagezi-multi",
                "hagezi-tif",
                "justdns-dach",
                "nrd-7",
                "justdns-threat-ips-v4",
            ]
            .into_iter()
            .collect(),
            "the recommended-badge set changed; update this test deliberately",
        );

        // The field's contract: one general default per category, plus at most
        // one regional supplement — so no category carries more than two badges.
        let mut per_category: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for preset in presets().iter().filter(|preset| preset.recommended) {
            *per_category.entry(preset.category).or_default() += 1;
        }
        assert!(
            per_category.values().all(|&count| count <= 2),
            "more than two recommended presets in a category: {per_category:?}",
        );
    }
}
