// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The curated catalogue that maps a queried domain to the company behind it.
//!
//! This powers the privacy dashboard: it turns a raw query log ("this device
//! looked up `connect.facebook.net`") into a plain-language story ("this device
//! contacted Meta 240 times"). Each [`Tracker`] names an organisation, a coarse
//! category, and the registrable domains it is known to serve third-party
//! tracking, advertising, analytics or session-replay from.
//!
//! The catalogue is **compiled in** (like the bundled block lists and the
//! blockable-services catalogue), so attribution is fully local and offline — a
//! query is never sent anywhere to find out who owns a domain. It is deliberately
//! a *curated* set of the best-known trackers rather than an exhaustive crawl:
//! the data sets that claim exhaustiveness (DuckDuckGo Tracker Radar) ship under
//! non-commercial licences that are incompatible with this project, so the module
//! is structured so a licence-compatible source can be folded in later.
//!
//! Matching reuses the same parent-suffix walk as [`crate::DomainSet`]: a lookup
//! of `px.ads.doubleclick.net` tries the whole name, then `ads.doubleclick.net`,
//! then `doubleclick.net` — which hits — attributing it to Google.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The coarse grouping shown on the dashboard. Kept as a string (not an enum) so
/// this crate stays dependency-free; the web layer maps it to a typed value.
///
/// One of: `advertising`, `analytics`, `social`, `fingerprinting`.
pub type Category = &'static str;

/// One organisation and the registrable domains it serves tracking from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tracker {
    /// The company shown to the user, e.g. `Google` or `Meta`.
    pub org: &'static str,
    /// Coarse grouping for the dashboard filter (see [`Category`]).
    pub category: Category,
    /// The registrable domains attributed to this organisation. A queried name
    /// matches one exactly or as a sub-domain, so only the registrable roots are
    /// listed (plus the occasional known sub-domain used only for tracking).
    pub domains: &'static [&'static str],
}

/// The whole catalogue.
pub fn trackers() -> &'static [Tracker] {
    TRACKERS
}

/// The distinct categories present, in display order.
pub const CATEGORIES: &[Category] = &["advertising", "analytics", "social", "fingerprinting"];

/// Every registrable domain the catalogue attributes to `org`, or `None` when no
/// company by that exact name is listed. The match is case-sensitive on the
/// display name, so pass the `org` string exactly as [`classify`] reports it.
/// Lets a caller block a whole company — all the domains behind it, not only the
/// ones already seen.
pub fn domains_for_org(org: &str) -> Option<&'static [&'static str]> {
    TRACKERS
        .iter()
        .find(|tracker| tracker.org == org)
        .map(|tracker| tracker.domains)
}

/// The domain → tracker index, built once on first use. Keys are the registrable
/// domains; the parent-suffix walk in [`classify`] reaches them from any
/// sub-domain.
fn index() -> &'static HashMap<&'static str, &'static Tracker> {
    static INDEX: OnceLock<HashMap<&'static str, &'static Tracker>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut map = HashMap::new();
        for tracker in TRACKERS {
            for domain in tracker.domains {
                map.insert(*domain, tracker);
            }
        }
        map
    })
}

/// Attribute a queried name to the organisation behind it, or `None` when it is
/// not a known tracker. Case-insensitive; a trailing dot and a `*.` wildcard are
/// ignored. Walks parent suffixes, so `ad.doubleclick.net` resolves to the
/// `doubleclick.net` entry.
pub fn classify(name: &str) -> Option<&'static Tracker> {
    let name = name.trim().trim_end_matches('.');
    let name = name.strip_prefix("*.").unwrap_or(name);
    if name.is_empty() {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    let index = index();

    let mut candidate = lower.as_str();
    loop {
        if let Some(tracker) = index.get(candidate) {
            return Some(tracker);
        }
        let dot = candidate.find('.')?;
        candidate = &candidate[dot + 1..];
    }
}

/// The catalogue. Grouped by the organisation's primary category; an org that
/// spans several (Meta does social pixels and ads) is filed under its most
/// recognisable one. Domains are registrable roots unless a sub-domain is the
/// only tracking surface.
static TRACKERS: &[Tracker] = &[
    // ----------------------------------------------------------------------
    // Advertising — ad networks, exchanges, DSPs/SSPs, verification, mobile ads
    // ----------------------------------------------------------------------
    Tracker {
        org: "Google",
        category: "advertising",
        domains: &[
            "doubleclick.net",
            "googlesyndication.com",
            "googleadservices.com",
            "googletagservices.com",
            "2mdn.net",
            "admob.com",
            "admob.net",
            "invitemedia.com",
            "googleadapis.com",
        ],
    },
    Tracker {
        org: "Amazon",
        category: "advertising",
        domains: &[
            "amazon-adsystem.com",
            "assoc-amazon.com",
            "serving-sys.com",
            "sizmek.com",
            "peer39.net",
        ],
    },
    Tracker {
        org: "Microsoft Advertising",
        category: "advertising",
        domains: &["msads.net", "msecnd.net", "ads.microsoft.com", "atdmt.com"],
    },
    Tracker {
        org: "The Trade Desk",
        category: "advertising",
        domains: &["adsrvr.org"],
    },
    Tracker {
        org: "Criteo",
        category: "advertising",
        domains: &["criteo.com", "criteo.net"],
    },
    Tracker {
        org: "Taboola",
        category: "advertising",
        domains: &["taboola.com", "zemanta.com"],
    },
    Tracker {
        org: "Outbrain",
        category: "advertising",
        domains: &["outbrain.com", "outbrainimg.com"],
    },
    Tracker {
        org: "Xandr",
        category: "advertising",
        domains: &["adnxs.com", "adnxs-simple.com"],
    },
    Tracker {
        org: "PubMatic",
        category: "advertising",
        domains: &["pubmatic.com"],
    },
    Tracker {
        org: "OpenX",
        category: "advertising",
        domains: &["openx.net"],
    },
    Tracker {
        org: "Magnite",
        category: "advertising",
        domains: &["rubiconproject.com", "magnite.com", "3lift.com"],
    },
    Tracker {
        org: "Index Exchange",
        category: "advertising",
        domains: &["casalemedia.com", "indexww.com"],
    },
    Tracker {
        org: "Equativ",
        category: "advertising",
        domains: &["smartadserver.com"],
    },
    Tracker {
        org: "Media.net",
        category: "advertising",
        domains: &["media.net"],
    },
    Tracker {
        org: "Yahoo",
        category: "advertising",
        domains: &[
            "advertising.com",
            "adtech.com",
            "flurry.com",
            "adtechus.com",
        ],
    },
    Tracker {
        org: "Oracle",
        category: "advertising",
        domains: &[
            "bluekai.com",
            "addthis.com",
            "moatads.com",
            "eloqua.com",
            "en25.com",
            "bkrtx.com",
            "nexac.com",
            "grapeshot.com",
        ],
    },
    Tracker {
        org: "LiveRamp",
        category: "advertising",
        domains: &["rlcdn.com", "liveramp.com", "pippio.com"],
    },
    Tracker {
        org: "Lotame",
        category: "advertising",
        domains: &["crwdcntrl.net"],
    },
    Tracker {
        org: "DoubleVerify",
        category: "advertising",
        domains: &["doubleverify.com"],
    },
    Tracker {
        org: "Integral Ad Science",
        category: "advertising",
        domains: &["adsafeprotected.com", "iasds01.com"],
    },
    Tracker {
        org: "Teads",
        category: "advertising",
        domains: &["teads.tv"],
    },
    Tracker {
        org: "Sharethrough",
        category: "advertising",
        domains: &["sharethrough.com"],
    },
    Tracker {
        org: "Sovrn",
        category: "advertising",
        domains: &["lijit.com"],
    },
    Tracker {
        org: "GumGum",
        category: "advertising",
        domains: &["gumgum.com"],
    },
    Tracker {
        org: "33Across",
        category: "advertising",
        domains: &["33across.com"],
    },
    Tracker {
        org: "ID5",
        category: "advertising",
        domains: &["id5-sync.com"],
    },
    Tracker {
        org: "BidSwitch",
        category: "advertising",
        domains: &["bidswitch.net"],
    },
    Tracker {
        org: "Smaato",
        category: "advertising",
        domains: &["smaato.net"],
    },
    Tracker {
        org: "InMobi",
        category: "advertising",
        domains: &["inmobi.com"],
    },
    Tracker {
        org: "Liftoff",
        category: "advertising",
        domains: &["vungle.com", "liftoff.io"],
    },
    Tracker {
        org: "AdColony",
        category: "advertising",
        domains: &["adcolony.com"],
    },
    Tracker {
        org: "AppLovin",
        category: "advertising",
        domains: &["applovin.com", "applvn.com"],
    },
    Tracker {
        org: "ironSource",
        category: "advertising",
        domains: &["ironsrc.com", "supersonicads.com"],
    },
    Tracker {
        org: "Chartboost",
        category: "advertising",
        domains: &["chartboost.com"],
    },
    Tracker {
        org: "Tapjoy",
        category: "advertising",
        domains: &["tapjoy.com"],
    },
    Tracker {
        org: "Unity Ads",
        category: "advertising",
        domains: &["unityads.unity3d.com"],
    },
    Tracker {
        org: "Adform",
        category: "advertising",
        domains: &["adform.net"],
    },
    Tracker {
        org: "Flashtalking",
        category: "advertising",
        domains: &["flashtalking.com"],
    },
    Tracker {
        org: "Innovid",
        category: "advertising",
        domains: &["innovid.com"],
    },
    Tracker {
        org: "Kargo",
        category: "advertising",
        domains: &["kargo.com"],
    },
    Tracker {
        org: "PubNative",
        category: "advertising",
        domains: &["pubnative.net"],
    },
    Tracker {
        org: "Smart Protected / Verve",
        category: "advertising",
        domains: &["verve.com", "mopub.com"],
    },
    // ----------------------------------------------------------------------
    // Analytics — measurement, tag managers, marketing clouds, A/B, attribution
    // ----------------------------------------------------------------------
    Tracker {
        org: "Google Analytics",
        category: "analytics",
        domains: &[
            "google-analytics.com",
            "googletagmanager.com",
            "app-measurement.com",
            "analytics.google.com",
        ],
    },
    Tracker {
        org: "Adobe",
        category: "analytics",
        domains: &[
            "omtrdc.net",
            "2o7.net",
            "demdex.net",
            "everesttech.net",
            "adobedtm.com",
            "hitbox.com",
        ],
    },
    Tracker {
        org: "Comscore",
        category: "analytics",
        domains: &["scorecardresearch.com", "comscore.com"],
    },
    Tracker {
        org: "Nielsen",
        category: "analytics",
        domains: &["imrworldwide.com", "nielsen.com"],
    },
    Tracker {
        org: "Quantcast",
        category: "analytics",
        domains: &["quantserve.com", "quantcount.com"],
    },
    Tracker {
        org: "Chartbeat",
        category: "analytics",
        domains: &["chartbeat.com", "chartbeat.net"],
    },
    Tracker {
        org: "Hotjar",
        category: "analytics",
        domains: &["hotjar.com", "hotjar.io"],
    },
    Tracker {
        org: "Mixpanel",
        category: "analytics",
        domains: &["mixpanel.com", "mxpnl.com"],
    },
    Tracker {
        org: "Segment",
        category: "analytics",
        domains: &["segment.com", "segment.io"],
    },
    Tracker {
        org: "Amplitude",
        category: "analytics",
        domains: &["amplitude.com"],
    },
    Tracker {
        org: "Heap",
        category: "analytics",
        domains: &["heapanalytics.com"],
    },
    Tracker {
        org: "New Relic",
        category: "analytics",
        domains: &["nr-data.net", "newrelic.com"],
    },
    Tracker {
        org: "Cloudflare",
        category: "analytics",
        domains: &["cloudflareinsights.com"],
    },
    Tracker {
        org: "Yandex",
        category: "analytics",
        domains: &["yandex.ru", "yandex.net", "yandexadexchange.net"],
    },
    Tracker {
        org: "Tealium",
        category: "analytics",
        domains: &["tealium.com", "tiqcdn.com"],
    },
    Tracker {
        org: "Ensighten",
        category: "analytics",
        domains: &["ensighten.com"],
    },
    Tracker {
        org: "Optimizely",
        category: "analytics",
        domains: &["optimizely.com", "optimizely.net"],
    },
    Tracker {
        org: "VWO",
        category: "analytics",
        domains: &["visualwebsiteoptimizer.com"],
    },
    Tracker {
        org: "Pendo",
        category: "analytics",
        domains: &["pendo.io"],
    },
    Tracker {
        org: "Akamai",
        category: "analytics",
        domains: &["go-mpulse.net"],
    },
    Tracker {
        org: "Bombora",
        category: "analytics",
        domains: &["bombora.com"],
    },
    Tracker {
        org: "Parse.ly",
        category: "analytics",
        domains: &["parsely.com"],
    },
    Tracker {
        org: "HubSpot",
        category: "analytics",
        domains: &[
            "hubspot.com",
            "hs-analytics.net",
            "hsadspixel.net",
            "hs-scripts.com",
            "hubspot.net",
        ],
    },
    Tracker {
        org: "Marketo",
        category: "analytics",
        domains: &["marketo.net", "mktoresp.com"],
    },
    Tracker {
        org: "Salesforce",
        category: "analytics",
        domains: &[
            "pardot.com",
            "exacttarget.com",
            "krxd.net",
            "demandbase.com",
        ],
    },
    Tracker {
        org: "Intercom",
        category: "analytics",
        domains: &["intercom.io", "intercomcdn.com"],
    },
    Tracker {
        org: "Drift",
        category: "analytics",
        domains: &["drift.com", "driftt.com"],
    },
    Tracker {
        org: "Zendesk",
        category: "analytics",
        domains: &["zopim.com", "zdassets.com"],
    },
    Tracker {
        org: "Branch",
        category: "analytics",
        domains: &["branch.io", "app.link"],
    },
    Tracker {
        org: "Adjust",
        category: "analytics",
        domains: &["adjust.com"],
    },
    Tracker {
        org: "AppsFlyer",
        category: "analytics",
        domains: &["appsflyer.com"],
    },
    Tracker {
        org: "Kochava",
        category: "analytics",
        domains: &["kochava.com"],
    },
    Tracker {
        org: "Singular",
        category: "analytics",
        domains: &["singular.net"],
    },
    Tracker {
        org: "Braze",
        category: "analytics",
        domains: &["braze.com", "appboy.com"],
    },
    Tracker {
        org: "OneSignal",
        category: "analytics",
        domains: &["onesignal.com"],
    },
    Tracker {
        org: "Localytics",
        category: "analytics",
        domains: &["localytics.com"],
    },
    Tracker {
        org: "Leanplum",
        category: "analytics",
        domains: &["leanplum.com"],
    },
    Tracker {
        org: "Airship",
        category: "analytics",
        domains: &["urbanairship.com"],
    },
    // ----------------------------------------------------------------------
    // Fingerprinting & session replay — record the screen, mouse, device
    // ----------------------------------------------------------------------
    Tracker {
        org: "Microsoft Clarity",
        category: "fingerprinting",
        domains: &["clarity.ms"],
    },
    Tracker {
        org: "FullStory",
        category: "fingerprinting",
        domains: &["fullstory.com"],
    },
    Tracker {
        org: "Mouseflow",
        category: "fingerprinting",
        domains: &["mouseflow.com"],
    },
    Tracker {
        org: "Inspectlet",
        category: "fingerprinting",
        domains: &["inspectlet.com"],
    },
    Tracker {
        org: "Crazy Egg",
        category: "fingerprinting",
        domains: &["crazyegg.com"],
    },
    Tracker {
        org: "Lucky Orange",
        category: "fingerprinting",
        domains: &["luckyorange.com"],
    },
    Tracker {
        org: "SessionCam",
        category: "fingerprinting",
        domains: &["sessioncam.com"],
    },
    Tracker {
        org: "Contentsquare",
        category: "fingerprinting",
        domains: &["clicktale.net", "contentsquare.net"],
    },
    // ----------------------------------------------------------------------
    // Social — the like-button / pixel / SDK surfaces of the big networks
    // ----------------------------------------------------------------------
    Tracker {
        org: "Meta",
        category: "social",
        domains: &[
            "facebook.com",
            "facebook.net",
            "fbcdn.net",
            "fbsbx.com",
            "instagram.com",
            "whatsapp.net",
        ],
    },
    Tracker {
        org: "X (Twitter)",
        category: "social",
        domains: &[
            "twitter.com",
            "x.com",
            "t.co",
            "ads-twitter.com",
            "twimg.com",
        ],
    },
    Tracker {
        org: "LinkedIn",
        category: "social",
        domains: &["linkedin.com", "licdn.com", "bizographics.com"],
    },
    Tracker {
        org: "Pinterest",
        category: "social",
        domains: &["pinterest.com", "pinimg.com"],
    },
    Tracker {
        org: "Snap",
        category: "social",
        domains: &["snapchat.com", "sc-static.net", "sc-cdn.net"],
    },
    Tracker {
        org: "Reddit",
        category: "social",
        domains: &["redditstatic.com", "redditmedia.com"],
    },
    Tracker {
        org: "ByteDance (TikTok)",
        category: "social",
        domains: &[
            "tiktok.com",
            "tiktokcdn.com",
            "byteoversea.com",
            "ibytedtos.com",
        ],
    },
    Tracker {
        org: "VK",
        category: "social",
        domains: &["vk.com", "userapi.com"],
    },
    // ----------------------------------------------------------------------
    // Additional advertising & affiliate networks
    // ----------------------------------------------------------------------
    Tracker {
        org: "AdRoll",
        category: "advertising",
        domains: &["adroll.com", "adroll.io"],
    },
    Tracker {
        org: "RTB House",
        category: "advertising",
        domains: &["rtbhouse.com", "creativecdn.com"],
    },
    Tracker {
        org: "Yieldmo",
        category: "advertising",
        domains: &["yieldmo.com"],
    },
    Tracker {
        org: "Nativo",
        category: "advertising",
        domains: &["nativo.com", "postrelease.com"],
    },
    Tracker {
        org: "StackAdapt",
        category: "advertising",
        domains: &["stackadapt.com"],
    },
    Tracker {
        org: "Simpli.fi",
        category: "advertising",
        domains: &["simpli.fi"],
    },
    Tracker {
        org: "Basis",
        category: "advertising",
        domains: &["sitescout.com", "basis.net"],
    },
    Tracker {
        org: "LoopMe",
        category: "advertising",
        domains: &["loopme.com"],
    },
    Tracker {
        org: "Fyber",
        category: "advertising",
        domains: &["fyber.com", "inner-active.mobi"],
    },
    Tracker {
        org: "Moloco",
        category: "advertising",
        domains: &["moloco.com"],
    },
    Tracker {
        org: "Digital Turbine",
        category: "advertising",
        domains: &["digitalturbine.com"],
    },
    Tracker {
        org: "Undertone",
        category: "advertising",
        domains: &["undertone.com"],
    },
    Tracker {
        org: "Sonobi",
        category: "advertising",
        domains: &["sonobi.com"],
    },
    Tracker {
        org: "Improve Digital",
        category: "advertising",
        domains: &["360yield.com"],
    },
    Tracker {
        org: "Yieldlab",
        category: "advertising",
        domains: &["yieldlab.net"],
    },
    Tracker {
        org: "ShareThis",
        category: "advertising",
        domains: &["sharethis.com"],
    },
    Tracker {
        org: "Skimlinks",
        category: "advertising",
        domains: &["skimresources.com", "skimlinks.com"],
    },
    Tracker {
        org: "VigLink",
        category: "advertising",
        domains: &["viglink.com"],
    },
    Tracker {
        org: "Rakuten",
        category: "advertising",
        domains: &["linksynergy.com", "rakuten-static.com"],
    },
    Tracker {
        org: "CJ Affiliate",
        category: "advertising",
        domains: &["cj.com", "dpbolvw.net", "anrdoezrs.net", "tkqlhce.com"],
    },
    Tracker {
        org: "Awin",
        category: "advertising",
        domains: &["dwin1.com", "awin1.com"],
    },
    Tracker {
        org: "Impact",
        category: "advertising",
        domains: &["impact.com", "impactradius-event.com"],
    },
    Tracker {
        org: "Pepperjam",
        category: "advertising",
        domains: &["pepperjamnetwork.com"],
    },
    Tracker {
        org: "ShareASale",
        category: "advertising",
        domains: &["shareasale.com"],
    },
    Tracker {
        org: "Raptive",
        category: "advertising",
        domains: &["adthrive.com"],
    },
    Tracker {
        org: "Mediavine",
        category: "advertising",
        domains: &["mediavine.com", "grow.me"],
    },
    Tracker {
        org: "Ezoic",
        category: "advertising",
        domains: &["ezoic.com", "ezoic.net"],
    },
    Tracker {
        org: "Unruly",
        category: "advertising",
        domains: &["1rx.io"],
    },
    Tracker {
        org: "Bidtellect",
        category: "advertising",
        domains: &["bttrack.com"],
    },
    Tracker {
        org: "Triton Digital",
        category: "advertising",
        domains: &["tritondigital.com"],
    },
    Tracker {
        org: "Kevel",
        category: "advertising",
        domains: &["adzerk.net"],
    },
    Tracker {
        org: "Apple",
        category: "advertising",
        domains: &["iadsdk.apple.com", "search.ads.apple.com"],
    },
    // ----------------------------------------------------------------------
    // Additional analytics, personalisation & A/B testing
    // ----------------------------------------------------------------------
    Tracker {
        org: "StatCounter",
        category: "analytics",
        domains: &["statcounter.com"],
    },
    Tracker {
        org: "Histats",
        category: "analytics",
        domains: &["histats.com"],
    },
    Tracker {
        org: "GoSquared",
        category: "analytics",
        domains: &["gosquared.com"],
    },
    Tracker {
        org: "Kissmetrics",
        category: "analytics",
        domains: &["kissmetrics.io", "kissmetrics.com"],
    },
    Tracker {
        org: "Clicky",
        category: "analytics",
        domains: &["getclicky.com"],
    },
    Tracker {
        org: "Woopra",
        category: "analytics",
        domains: &["woopra.com"],
    },
    Tracker {
        org: "AB Tasty",
        category: "analytics",
        domains: &["abtasty.com"],
    },
    Tracker {
        org: "Convert",
        category: "analytics",
        domains: &["convertexperiments.com"],
    },
    Tracker {
        org: "Dynamic Yield",
        category: "analytics",
        domains: &["dynamicyield.com"],
    },
    Tracker {
        org: "Monetate",
        category: "analytics",
        domains: &["monetate.net"],
    },
    Tracker {
        org: "Qualtrics",
        category: "analytics",
        domains: &["qualtrics.com"],
    },
    Tracker {
        org: "Medallia",
        category: "analytics",
        domains: &["medallia.com"],
    },
    Tracker {
        org: "mParticle",
        category: "analytics",
        domains: &["mparticle.com"],
    },
    Tracker {
        org: "Permutive",
        category: "analytics",
        domains: &["permutive.com", "permutive.app"],
    },
    Tracker {
        org: "Cxense",
        category: "analytics",
        domains: &["cxense.com", "cxpublic.com"],
    },
    Tracker {
        org: "Piano",
        category: "analytics",
        domains: &["tinypass.com", "npttech.com"],
    },
    Tracker {
        org: "BlueConic",
        category: "analytics",
        domains: &["blueconic.net"],
    },
    Tracker {
        org: "Baidu Analytics",
        category: "analytics",
        domains: &["hm.baidu.com"],
    },
    // ----------------------------------------------------------------------
    // Additional session replay & anti-fraud fingerprinting
    // ----------------------------------------------------------------------
    Tracker {
        org: "Smartlook",
        category: "fingerprinting",
        domains: &["smartlook.com"],
    },
    Tracker {
        org: "LogRocket",
        category: "fingerprinting",
        domains: &["logrocket.io", "logrocket.com", "lr-in.com"],
    },
    Tracker {
        org: "Quantum Metric",
        category: "fingerprinting",
        domains: &["quantummetric.com"],
    },
    Tracker {
        org: "Decibel",
        category: "fingerprinting",
        domains: &["decibelinsight.net"],
    },
    Tracker {
        org: "Glassbox",
        category: "fingerprinting",
        domains: &["glassbox.com"],
    },
    Tracker {
        org: "Sift",
        category: "fingerprinting",
        domains: &["siftscience.com"],
    },
    Tracker {
        org: "Riskified",
        category: "fingerprinting",
        domains: &["riskified.com"],
    },
    Tracker {
        org: "Forter",
        category: "fingerprinting",
        domains: &["forter.com"],
    },
    Tracker {
        org: "Signifyd",
        category: "fingerprinting",
        domains: &["signifyd.com"],
    },
    Tracker {
        org: "LexisNexis",
        category: "fingerprinting",
        domains: &["online-metrix.net", "iovation.com"],
    },
    Tracker {
        org: "HUMAN",
        category: "fingerprinting",
        domains: &["perimeterx.net", "px-cloud.net"],
    },
    Tracker {
        org: "DataDome",
        category: "fingerprinting",
        domains: &["datadome.co"],
    },
    Tracker {
        org: "Fingerprint",
        category: "fingerprinting",
        domains: &["fpjs.io", "fingerprint.com"],
    },
    // ----------------------------------------------------------------------
    // Additional social & regional platforms (pixel / SDK surfaces)
    // ----------------------------------------------------------------------
    Tracker {
        org: "Disqus",
        category: "social",
        domains: &["disqus.com", "disquscdn.com"],
    },
    Tracker {
        org: "Automattic",
        category: "social",
        domains: &["tumblr.com", "wp.com", "gravatar.com"],
    },
    Tracker {
        org: "Quora",
        category: "social",
        domains: &["quora.com", "quoracdn.net"],
    },
    Tracker {
        org: "Weibo",
        category: "social",
        domains: &["weibo.com", "sinajs.cn"],
    },
    Tracker {
        org: "LINE",
        category: "social",
        domains: &["line.me", "line-scdn.net"],
    },
    Tracker {
        org: "Kakao",
        category: "social",
        domains: &["kakao.com", "daumcdn.net"],
    },
    Tracker {
        org: "Baidu",
        category: "social",
        domains: &["baidu.com", "bdstatic.com"],
    },
    Tracker {
        org: "Naver",
        category: "social",
        domains: &["naver.com", "pstatic.net"],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_known_domains_and_subdomains() {
        assert_eq!(classify("doubleclick.net").map(|t| t.org), Some("Google"));
        // A sub-domain resolves to the registrable root's org.
        assert_eq!(
            classify("ad.g.doubleclick.net").map(|t| t.org),
            Some("Google")
        );
        assert_eq!(
            classify("connect.facebook.net").map(|t| t.org),
            Some("Meta")
        );
        assert_eq!(
            classify("www.google-analytics.com").map(|t| t.category),
            Some("analytics")
        );
    }

    #[test]
    fn is_case_and_trailing_dot_insensitive() {
        assert_eq!(classify("DoubleClick.NET.").map(|t| t.org), Some("Google"));
        assert_eq!(classify("*.criteo.com").map(|t| t.org), Some("Criteo"));
    }

    #[test]
    fn unknown_domains_are_not_attributed() {
        assert_eq!(classify("example.com"), None);
        assert_eq!(classify("net"), None);
        assert_eq!(classify(""), None);
        // Ends with a tracker domain's text but not on a label boundary.
        assert_eq!(classify("notdoubleclick.net"), None);
    }

    #[test]
    fn every_domain_is_attributed_to_exactly_one_org() {
        let mut seen = std::collections::HashMap::new();
        for tracker in TRACKERS {
            for domain in tracker.domains {
                if let Some(other) = seen.insert(*domain, tracker.org) {
                    panic!(
                        "domain {domain} is listed under both {other} and {}",
                        tracker.org
                    );
                }
            }
        }
    }

    #[test]
    fn categories_are_all_known() {
        for tracker in TRACKERS {
            assert!(
                CATEGORIES.contains(&tracker.category),
                "unknown category {} for {}",
                tracker.category,
                tracker.org
            );
        }
    }

    #[test]
    fn catalogue_is_reasonably_broad() {
        let domains: usize = TRACKERS.iter().map(|t| t.domains.len()).sum();
        assert!(domains >= 250, "expected a broad catalogue, got {domains}");
    }
}
