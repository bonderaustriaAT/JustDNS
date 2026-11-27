// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Phishing detection: spot domains built to impersonate a known brand.
//!
//! Unlike the block engine, which answers "is this exact name on a list?", this
//! layer *computes* a verdict on the queried name:
//!
//! - **Lookalike / homoglyph:** a label whose "skeleton" (confusable characters
//!   folded to a canonical form, e.g. a Cyrillic `а` → `a`, `0` → `o`) matches a
//!   protected brand, while the label itself is not the plain brand string. So
//!   `pа‌ypal.com` (Cyrillic a) and `micros0ft-login.com` are caught, but the real
//!   `paypal.com` is not.
//! - **Punycode:** the same, reached through an `xn--` label that decodes to a
//!   confusable of a brand.
//! - **Newly registered domain (NRD):** the registrable domain is on the NRD warn
//!   set the binary keeps in step with a daily feed.
//!
//! Everything here is pure and offline: the brand catalogue and the confusable
//! table are compiled in, and the only lookups are set membership and a per-label
//! character fold. The optional online (RDAP) registration check is **not** done
//! here — it is an out-of-band enrichment the binary runs off the hot path.
//!
//! A hit never changes an answer by itself: the pipeline decides, from the hit's
//! [`PhishingMode`], whether to warn (mark the query) or block it.

use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;

use crate::domain::DomainSet;

/// Whether a phishing hit should only warn (mark the query in the log) or block
/// it like a normal refusal. Carried on every [`PhishingHit`] so the pipeline
/// knows what to do without consulting the config again. Mirrors the config
/// crate's `PhishingMode`; kept local so this crate stays a leaf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhishingMode {
    /// Let the query resolve, but record the hit.
    Warn,
    /// Refuse the query before any lookup.
    Block,
}

/// What kind of phishing signal fired, and against which brand where relevant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PhishingKind {
    /// A lookalike/homoglyph of `brand` (the display name), via confusable
    /// characters in a plain label.
    Lookalike { brand: String },
    /// A lookalike of `brand` reached through a Punycode (`xn--`) label.
    Punycode { brand: String },
    /// A newly registered domain (on the NRD warn set).
    Nrd,
}

/// A phishing verdict for one queried name: what fired and whether to warn or
/// block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhishingHit {
    pub kind: PhishingKind,
    pub mode: PhishingMode,
}

/// Which family a protected brand belongs to, so the dashboard can group the
/// shipped catalogue the way the mock-up does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrandGroup {
    /// Big technology and mail providers.
    TechMail,
    /// Austrian banks.
    BanksAt,
    /// German banks.
    BanksDe,
}

impl BrandGroup {
    /// A stable machine key, for the API/UI.
    pub fn as_str(self) -> &'static str {
        match self {
            BrandGroup::TechMail => "tech-mail",
            BrandGroup::BanksAt => "banks-at",
            BrandGroup::BanksDe => "banks-de",
        }
    }
}

/// One entry in the compiled-in brand catalogue: a stable id, a display name, the
/// group it is shown under, the label token(s) an impersonator mimics, and the
/// brand's real domains (so the brand's own names are never flagged).
#[derive(Clone, Copy, Debug)]
pub struct Brand {
    pub id: &'static str,
    pub name: &'static str,
    pub group: BrandGroup,
    /// The label token(s) a lookalike imitates, e.g. `paypal`, `microsoft`.
    pub tokens: &'static [&'static str],
    /// The brand's real registrable domains, excluded from matching.
    pub domains: &'static [&'static str],
}

/// The compiled-in brand catalogue. Curated, offline, and shown in the dashboard
/// grouped by [`BrandGroup`]. The DACH banks are the part international lists miss.
pub const BRANDS: &[Brand] = &[
    // Big tech and mail.
    brand(
        "google",
        "Google",
        BrandGroup::TechMail,
        &["google"],
        &["google.com"],
    ),
    brand(
        "microsoft",
        "Microsoft",
        BrandGroup::TechMail,
        &["microsoft"],
        &["microsoft.com"],
    ),
    brand(
        "apple",
        "Apple",
        BrandGroup::TechMail,
        &["apple"],
        &["apple.com"],
    ),
    brand(
        "amazon",
        "Amazon",
        BrandGroup::TechMail,
        &["amazon"],
        &["amazon.com", "amazon.de", "amazon.at"],
    ),
    brand(
        "paypal",
        "PayPal",
        BrandGroup::TechMail,
        &["paypal"],
        &["paypal.com"],
    ),
    brand(
        "outlook",
        "Outlook",
        BrandGroup::TechMail,
        &["outlook"],
        &["outlook.com", "live.com"],
    ),
    brand(
        "gmail",
        "Gmail",
        BrandGroup::TechMail,
        &["gmail"],
        &["gmail.com"],
    ),
    brand(
        "netflix",
        "Netflix",
        BrandGroup::TechMail,
        &["netflix"],
        &["netflix.com"],
    ),
    // Austrian banks.
    brand(
        "erste",
        "Erste Bank / George",
        BrandGroup::BanksAt,
        &["george", "erstebank", "sparkasse"],
        &["george.at", "erstebank.at", "sparkasse.at"],
    ),
    brand(
        "raiffeisen",
        "Raiffeisen",
        BrandGroup::BanksAt,
        &["raiffeisen"],
        &["raiffeisen.at"],
    ),
    brand(
        "bankaustria",
        "Bank Austria",
        BrandGroup::BanksAt,
        &["bankaustria"],
        &["bankaustria.at"],
    ),
    brand(
        "bawag",
        "BAWAG",
        BrandGroup::BanksAt,
        &["bawag"],
        &["bawag.at"],
    ),
    brand(
        "oberbank",
        "Oberbank",
        BrandGroup::BanksAt,
        &["oberbank"],
        &["oberbank.at"],
    ),
    // German banks.
    brand(
        "sparkassede",
        "Sparkasse",
        BrandGroup::BanksDe,
        &["sparkasse"],
        &["sparkasse.de"],
    ),
    brand(
        "deutschebank",
        "Deutsche Bank",
        BrandGroup::BanksDe,
        &["deutschebank"],
        &["deutsche-bank.de"],
    ),
    brand(
        "commerzbank",
        "Commerzbank",
        BrandGroup::BanksDe,
        &["commerzbank"],
        &["commerzbank.de"],
    ),
    brand("ing", "ING", BrandGroup::BanksDe, &["ingbank"], &["ing.de"]),
    brand("dkb", "DKB", BrandGroup::BanksDe, &["dkbbank"], &["dkb.de"]),
    brand(
        "volksbank",
        "Volksbank",
        BrandGroup::BanksDe,
        &["volksbank"],
        &["volksbank.de"],
    ),
];

/// Const-fn helper so the catalogue above reads as a table.
const fn brand(
    id: &'static str,
    name: &'static str,
    group: BrandGroup,
    tokens: &'static [&'static str],
    domains: &'static [&'static str],
) -> Brand {
    Brand {
        id,
        name,
        group,
        tokens,
        domains,
    }
}

/// The inputs the binary hands the detector to (re)build its state, assembled
/// from the stored settings plus the NRD warn set. Kept primitive so this crate
/// never depends on the config crate.
#[derive(Clone, Debug)]
pub struct PhishingConfig {
    /// Whether phishing protection runs at all.
    pub enabled: bool,
    /// Warn or block on a lookalike/Punycode hit.
    pub homoglyph_mode: PhishingMode,
    /// Whether NRD warning is on.
    pub nrd_enabled: bool,
    /// Warn or block on an NRD hit.
    pub nrd_mode: PhishingMode,
    /// Extra brands the user protects, by their canonical domain (e.g.
    /// `meine-firma.at`). The first label becomes the matched token.
    pub extra_brands: Vec<String>,
    /// Compiled-in brand ids the user switched off.
    pub disabled_builtin: Vec<String>,
    /// The newly-registered-domain warn set, shared with the lists store that
    /// parsed it once — never copied per configure.
    pub nrd: Arc<DomainSet>,
}

impl Default for PhishingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            homoglyph_mode: PhishingMode::Warn,
            nrd_enabled: false,
            nrd_mode: PhishingMode::Warn,
            extra_brands: Vec::new(),
            disabled_builtin: Vec::new(),
            nrd: Arc::new(DomainSet::new()),
        }
    }
}

/// A protected brand label resolved for matching: its raw token, display name and
/// real domains.
#[derive(Clone, Debug)]
struct BrandLabel {
    raw: String,
    display: String,
    domains: Vec<String>,
}

/// The detector's resolved state, swapped wholesale on every config change.
#[derive(Default)]
struct State {
    enabled: bool,
    homoglyph_mode_warn: bool,
    nrd_enabled: bool,
    nrd_mode_warn: bool,
    /// Brand labels indexed by their skeleton, so matching a query label is one
    /// hash lookup after folding it.
    by_skeleton: HashMap<String, Vec<BrandLabel>>,
    /// The NRD warn set, shared with the lists store (never copied).
    nrd: Arc<DomainSet>,
}

/// The live phishing detector the pipeline consults. Cheap to share; its state is
/// swapped atomically when settings change, so a dashboard edit takes effect on
/// the next query without a restart (like [`crate::PolicyEngine`]).
pub struct PhishingDetector {
    state: ArcSwap<State>,
}

impl Default for PhishingDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl PhishingDetector {
    /// A detector that flags nothing until it is configured.
    pub fn new() -> Self {
        Self {
            state: ArcSwap::from_pointee(State::default()),
        }
    }

    /// Rebuild the detector from `config`. Takes effect on the next query.
    pub fn configure(&self, config: PhishingConfig) {
        let mode = |warn: PhishingMode| matches!(warn, PhishingMode::Warn);
        let mut by_skeleton: HashMap<String, Vec<BrandLabel>> = HashMap::new();

        let disabled = &config.disabled_builtin;
        for brand in BRANDS {
            if disabled.iter().any(|id| id == brand.id) {
                continue;
            }
            for token in brand.tokens {
                add_label(
                    &mut by_skeleton,
                    token,
                    brand.name,
                    brand.domains.iter().map(|d| (*d).to_owned()).collect(),
                );
            }
        }
        for domain in &config.extra_brands {
            let domain = normalise_host(domain);
            if domain.is_empty() {
                continue;
            }
            // The token is the registrable label (the part before the first dot).
            let token = domain.split('.').next().unwrap_or(&domain);
            if token.is_empty() {
                continue;
            }
            add_label(&mut by_skeleton, token, &domain, vec![domain.clone()]);
        }

        self.state.store(Arc::new(State {
            enabled: config.enabled,
            homoglyph_mode_warn: mode(config.homoglyph_mode),
            nrd_enabled: config.nrd_enabled,
            nrd_mode_warn: mode(config.nrd_mode),
            by_skeleton,
            nrd: config.nrd,
        }));
    }

    /// The phishing verdict for `name`, or `None` when nothing fired. Checks the
    /// more specific lookalike/Punycode signal first, then the NRD set.
    pub fn check(&self, name: &str) -> Option<PhishingHit> {
        let state = self.state.load();
        if !state.enabled {
            return None;
        }

        let host = normalise_host(name);
        if host.is_empty() {
            return None;
        }

        // Lookalike / Punycode: fold each label to its skeleton and look it up
        // among the protected brands. Only a Punycode (`xn--`) label is decoded to
        // Unicode first — a plain label is already ASCII-lowercased, so the common
        // no-`xn--` query does no IDNA work at all.
        if !state.by_skeleton.is_empty() {
            for raw_label in host.split('.') {
                if raw_label.is_empty() {
                    continue;
                }
                // A Punycode label is decoded on its own, so the hit's kind
                // reflects the label that actually matched rather than any
                // unrelated `xn--` label elsewhere in the name.
                let is_puny = raw_label.starts_with("xn--");
                let decoded;
                let label: &str = if is_puny {
                    decoded = idna::domain_to_unicode(raw_label).0;
                    &decoded
                } else {
                    raw_label
                };
                // Match the whole label and each hyphen-delimited segment, so both
                // `micros0ft.com` and `micros0ft-update.com` are caught, and a
                // hyphenated brand token (e.g. a user's `meine-firma`) still
                // matches on the whole label.
                for part in candidate_parts(label) {
                    let sk = skeleton(part);
                    let Some(candidates) = state.by_skeleton.get(&sk) else {
                        continue;
                    };
                    for cand in candidates {
                        // Only a confusable *substitution* counts: a part equal to
                        // the plain brand token is the brand itself or a benign
                        // first-party component, not a homoglyph.
                        if part == cand.raw {
                            continue;
                        }
                        if within_domain(&host, &cand.domains) {
                            continue;
                        }
                        let brand = cand.display.clone();
                        let kind = if is_puny {
                            PhishingKind::Punycode { brand }
                        } else {
                            PhishingKind::Lookalike { brand }
                        };
                        return Some(PhishingHit {
                            kind,
                            mode: warn_or_block(state.homoglyph_mode_warn),
                        });
                    }
                }
            }
        }

        // Newly registered domain.
        if state.nrd_enabled && state.nrd.blocks(&host) {
            return Some(PhishingHit {
                kind: PhishingKind::Nrd,
                mode: warn_or_block(state.nrd_mode_warn),
            });
        }

        None
    }
}

/// Add one brand label to the skeleton index.
fn add_label(
    by_skeleton: &mut HashMap<String, Vec<BrandLabel>>,
    token: &str,
    display: &str,
    domains: Vec<String>,
) {
    let raw = token.to_lowercase();
    let sk = skeleton(&raw);
    by_skeleton.entry(sk).or_default().push(BrandLabel {
        raw,
        display: display.to_owned(),
        domains,
    });
}

fn warn_or_block(warn: bool) -> PhishingMode {
    if warn {
        PhishingMode::Warn
    } else {
        PhishingMode::Block
    }
}

/// Lower-case `name`, strip a trailing dot and a leading `*.`, for comparison.
fn normalise_host(name: &str) -> String {
    let name = name.trim().trim_end_matches('.');
    let name = name.strip_prefix("*.").unwrap_or(name);
    name.to_lowercase()
}

/// Whether `host` is `domain` or a sub-domain of one of `domains`.
fn within_domain(host: &str, domains: &[String]) -> bool {
    domains
        .iter()
        .any(|d| host == d.as_str() || host.ends_with(&format!(".{d}")))
}

/// The strings to test a DNS label against: the whole label, plus each
/// hyphen-delimited segment when it contains a hyphen (so a brand used as one
/// component of a longer name is still caught). No hyphen means just the label.
fn candidate_parts(label: &str) -> Vec<&str> {
    if label.contains('-') {
        let mut parts = vec![label];
        parts.extend(label.split('-').filter(|p| !p.is_empty()));
        parts
    } else {
        vec![label]
    }
}

/// Fold a label to its skeleton: lower-case it and map each confusable character
/// to a canonical ASCII letter, so `pа‌ypal` (Cyrillic a) and `micros0ft` both
/// fold to the brand's skeleton. Characters with no mapping pass through.
fn skeleton(label: &str) -> String {
    label.to_lowercase().chars().map(deconfuse).collect()
}

/// Map a single character to its canonical confusable, conservatively — only
/// well-known homoglyphs, to keep false positives low.
fn deconfuse(c: char) -> char {
    match c {
        // Digit/letter swaps.
        '0' => 'o',
        '1' => 'l',
        '3' => 'e',
        '5' => 's',
        '7' => 't',
        // Cyrillic lookalikes.
        'а' => 'a',
        'е' => 'e',
        'о' => 'o',
        'р' => 'p',
        'с' => 'c',
        'у' => 'y',
        'х' => 'x',
        'ѕ' => 's',
        'і' => 'i',
        'ј' => 'j',
        'ԁ' => 'd',
        'һ' => 'h',
        'ё' => 'e',
        // Greek lookalikes.
        'ο' => 'o',
        'α' => 'a',
        'ε' => 'e',
        'ρ' => 'p',
        'ι' => 'i',
        'ν' => 'v',
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detector(mode: PhishingMode) -> PhishingDetector {
        let d = PhishingDetector::new();
        d.configure(PhishingConfig {
            enabled: true,
            homoglyph_mode: mode,
            nrd_enabled: true,
            nrd_mode: PhishingMode::Warn,
            extra_brands: vec!["meine-firma.at".to_owned()],
            disabled_builtin: Vec::new(),
            nrd: Arc::new(DomainSet::from_rules("neueseite.ru\n")),
        });
        d
    }

    #[test]
    fn disabled_detector_flags_nothing() {
        let d = PhishingDetector::new();
        d.configure(PhishingConfig {
            enabled: false,
            ..Default::default()
        });
        assert!(d.check("xn--pypal-4ve.com").is_none());
    }

    #[test]
    fn catches_a_cyrillic_lookalike() {
        let d = detector(PhishingMode::Warn);
        // "pаypal.com" with a Cyrillic а (U+0430).
        let hit = d.check("p\u{0430}ypal.com").expect("cyrillic lookalike");
        assert_eq!(
            hit.kind,
            PhishingKind::Lookalike {
                brand: "PayPal".to_owned()
            }
        );
        assert_eq!(hit.mode, PhishingMode::Warn);
    }

    #[test]
    fn catches_a_digit_swap() {
        let d = detector(PhishingMode::Block);
        let hit = d.check("micros0ft-update.com").expect("digit swap");
        assert_eq!(
            hit.kind,
            PhishingKind::Lookalike {
                brand: "Microsoft".to_owned()
            }
        );
        assert_eq!(hit.mode, PhishingMode::Block, "block mode is carried");
    }

    #[test]
    fn catches_a_punycode_lookalike() {
        let d = detector(PhishingMode::Warn);
        // xn--pypal-4ve.com decodes to "pаypal.com" (Cyrillic a).
        let hit = d.check("xn--pypal-4ve.com").expect("punycode lookalike");
        assert!(matches!(hit.kind, PhishingKind::Punycode { .. }));
    }

    #[test]
    fn ascii_lookalike_beside_an_unrelated_punycode_label_is_not_punycode() {
        let d = detector(PhishingMode::Warn);
        // The brand match is the plain-ASCII `paypa1` label; the `xn--` label is
        // unrelated. The kind must reflect the matched label, not the name as a
        // whole.
        let hit = d
            .check("paypa1.xn--tna.example")
            .expect("ascii lookalike still fires");
        assert!(
            matches!(&hit.kind, PhishingKind::Lookalike { brand } if brand == "PayPal"),
            "{:?}",
            hit.kind
        );
    }

    #[test]
    fn leaves_the_real_brand_domain_alone() {
        let d = detector(PhishingMode::Warn);
        assert!(d.check("paypal.com").is_none(), "the real domain");
        assert!(d.check("www.paypal.com").is_none(), "and its sub-domains");
        assert!(d.check("login.microsoft.com").is_none());
    }

    #[test]
    fn leaves_unrelated_names_alone() {
        let d = detector(PhishingMode::Warn);
        assert!(d.check("example.com").is_none());
        assert!(d.check("news.orf.at").is_none());
    }

    #[test]
    fn protects_a_user_added_brand() {
        let d = detector(PhishingMode::Warn);
        // "meine-firrna.at" mimics "meine-firma" (rn ~ m is not folded, so use a
        // digit swap on a user brand with a digit-free token instead): fold a
        // Cyrillic a in "meine-firma".
        let hit = d
            .check("meine-firm\u{0430}.at")
            .expect("user brand lookalike");
        assert!(matches!(hit.kind, PhishingKind::Lookalike { .. }));
        assert!(d.check("meine-firma.at").is_none(), "the real user brand");
    }

    #[test]
    fn flags_a_newly_registered_domain() {
        let d = detector(PhishingMode::Warn);
        let hit = d.check("sub.neueseite.ru").expect("on the NRD set");
        assert_eq!(hit.kind, PhishingKind::Nrd);
        assert!(d.check("alteseite.ru").is_none(), "not on the NRD set");
    }

    #[test]
    fn nrd_respects_its_own_switch() {
        let d = PhishingDetector::new();
        d.configure(PhishingConfig {
            enabled: true,
            nrd_enabled: false,
            nrd: Arc::new(DomainSet::from_rules("neueseite.ru\n")),
            ..Default::default()
        });
        assert!(d.check("neueseite.ru").is_none(), "nrd switched off");
    }
}
