// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The curated catalogue of blockable services.
//!
//! A "service" is a well-known product — a social network, a streaming or music
//! service, a messenger, a game store — bundled as the small set of domains it
//! relies on, so the whole thing can be blocked with a single switch instead of
//! hand-copying domains. This mirrors AdGuard Home's "Blocked Services".
//!
//! The catalogue is compiled in (like the bundled ads/malware starter lists), so
//! it works with no network access. Each service becomes one block source in a
//! resolved [`Policy`](crate::Policy): the domains match exactly or by sub-domain
//! (see [`DomainSet`]), and the user's allow list still wins over them. A service
//! is referenced as a list object with the id `svc:<id>`, so it toggles through
//! the very same per-scope mechanism as every other list.

use crate::DomainSet;

/// The prefix that turns a catalogue service id (`tiktok`) into the list-object
/// id used across policies, deltas and the API (`svc:tiktok`).
pub const SERVICE_PREFIX: &str = "svc:";

/// One entry in the blockable-services catalogue: a product and the domains it
/// needs, grouped for the dashboard's category filter.
#[derive(Clone, Copy, Debug)]
pub struct Service {
    /// Stable machine id, e.g. `tiktok`. The list-object id is `svc:<id>`.
    pub id: &'static str,
    /// Display name shown in the dashboard and reported when a query is blocked.
    pub name: &'static str,
    /// Coarse grouping for the catalogue filter: one of `social`, `video`,
    /// `messaging`, `gaming` or `music`.
    pub category: &'static str,
    /// The domains this service relies on. Matched exactly or by sub-domain, so
    /// only the registrable roots are listed.
    pub domains: &'static [&'static str],
}

impl Service {
    /// The list-object id for this service (`svc:<id>`), as used in policies,
    /// deltas and the API.
    pub fn object_id(&self) -> String {
        format!("{SERVICE_PREFIX}{}", self.id)
    }

    /// Build the domain set this service blocks. Cheap — each catalogue entry is
    /// a handful of domains.
    pub fn domain_set(&self) -> DomainSet {
        let mut set = DomainSet::new();
        for domain in self.domains {
            set.add_rules(domain);
        }
        set
    }
}

/// The whole catalogue, grouped by category.
pub fn services() -> &'static [Service] {
    SERVICES
}

/// Look up a service by its machine id (the bare id, without the `svc:` prefix).
pub fn service(id: &str) -> Option<&'static Service> {
    SERVICES.iter().find(|service| service.id == id)
}

static SERVICES: &[Service] = &[
    // --- Social networks ---
    Service {
        id: "facebook",
        name: "Facebook",
        category: "social",
        domains: &[
            "facebook.com",
            "facebook.net",
            "fbcdn.net",
            "fbsbx.com",
            "fb.com",
            "fb.me",
        ],
    },
    Service {
        id: "instagram",
        name: "Instagram",
        category: "social",
        domains: &["instagram.com", "cdninstagram.com", "ig.me"],
    },
    Service {
        id: "tiktok",
        name: "TikTok",
        category: "social",
        domains: &[
            "tiktok.com",
            "tiktokcdn.com",
            "tiktokv.com",
            "byteoversea.com",
            "ibytedtos.com",
            "musical.ly",
        ],
    },
    Service {
        id: "x",
        name: "X (Twitter)",
        category: "social",
        domains: &["twitter.com", "x.com", "t.co", "twimg.com"],
    },
    Service {
        id: "snapchat",
        name: "Snapchat",
        category: "social",
        domains: &["snapchat.com", "sc-cdn.net", "snap.com", "snapkit.co"],
    },
    Service {
        id: "reddit",
        name: "Reddit",
        category: "social",
        domains: &[
            "reddit.com",
            "redd.it",
            "redditstatic.com",
            "redditmedia.com",
        ],
    },
    Service {
        id: "pinterest",
        name: "Pinterest",
        category: "social",
        domains: &["pinterest.com", "pinimg.com"],
    },
    // --- Video ---
    Service {
        id: "youtube",
        name: "YouTube",
        category: "video",
        domains: &[
            "youtube.com",
            "youtu.be",
            "ytimg.com",
            "youtube-nocookie.com",
            "googlevideo.com",
        ],
    },
    Service {
        id: "netflix",
        name: "Netflix",
        category: "video",
        domains: &[
            "netflix.com",
            "nflxvideo.net",
            "nflximg.net",
            "nflxext.com",
            "nflxso.net",
        ],
    },
    Service {
        id: "twitch",
        name: "Twitch",
        category: "video",
        domains: &["twitch.tv", "ttvnw.net", "jtvnw.net", "twitchcdn.net"],
    },
    Service {
        id: "disneyplus",
        name: "Disney+",
        category: "video",
        domains: &[
            "disneyplus.com",
            "disney-plus.net",
            "dssott.com",
            "bamgrid.com",
        ],
    },
    // --- Messaging ---
    Service {
        id: "whatsapp",
        name: "WhatsApp",
        category: "messaging",
        domains: &["whatsapp.com", "whatsapp.net", "wa.me"],
    },
    Service {
        id: "telegram",
        name: "Telegram",
        category: "messaging",
        domains: &[
            "telegram.org",
            "telegram.me",
            "t.me",
            "telegra.ph",
            "tdesktop.com",
        ],
    },
    Service {
        id: "discord",
        name: "Discord",
        category: "messaging",
        domains: &[
            "discord.com",
            "discord.gg",
            "discordapp.com",
            "discordapp.net",
            "discord.media",
        ],
    },
    // --- Gaming ---
    Service {
        id: "steam",
        name: "Steam",
        category: "gaming",
        domains: &[
            "steampowered.com",
            "steamcommunity.com",
            "steamstatic.com",
            "steamcontent.com",
            "steamusercontent.com",
        ],
    },
    Service {
        id: "epicgames",
        name: "Epic Games",
        category: "gaming",
        domains: &[
            "epicgames.com",
            "unrealengine.com",
            "fortnite.com",
            "easyanticheat.net",
        ],
    },
    // --- Music ---
    Service {
        id: "spotify",
        name: "Spotify",
        category: "music",
        domains: &[
            "spotify.com",
            "scdn.co",
            "spotifycdn.com",
            "spoti.fi",
            "pscdn.co",
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_is_well_formed() {
        let mut ids = std::collections::HashSet::new();
        for service in services() {
            assert!(
                ids.insert(service.id),
                "duplicate service id {}",
                service.id
            );
            assert!(!service.name.is_empty());
            assert!(
                ["social", "video", "messaging", "gaming", "music"].contains(&service.category),
                "{} has an unknown category {}",
                service.id,
                service.category
            );
            assert!(!service.domains.is_empty(), "{} has no domains", service.id);
            // Every listed domain must survive the rule parser, or it would be
            // silently dropped from the set.
            let set = service.domain_set();
            assert_eq!(
                set.len(),
                service.domains.len(),
                "{} has a domain the parser rejected",
                service.id
            );
        }
    }

    #[test]
    fn a_service_blocks_its_domains_and_their_subdomains() {
        let tiktok = service("tiktok").expect("tiktok in the catalogue");
        let set = tiktok.domain_set();
        assert!(set.blocks("tiktok.com"));
        assert!(set.blocks("www.tiktok.com"), "sub-domains too");
        assert!(!set.blocks("example.com"));
        assert_eq!(tiktok.object_id(), "svc:tiktok");
    }

    #[test]
    fn lookup_by_id() {
        assert!(service("youtube").is_some());
        assert!(service("svc:youtube").is_none(), "lookup takes the bare id");
        assert!(service("nope").is_none());
    }
}
