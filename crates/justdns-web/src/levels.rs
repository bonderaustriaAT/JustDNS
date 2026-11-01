// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The plain "block level" the setup wizard and settings page offer, mapped to
//! the concrete feeds and switches it stands for.
//!
//! A level (`Off`/`Minimal`/`Standard`/`Extreme`) is just a named selection out
//! of the curated catalogue plus the ads/malware switches. Picking one turns its
//! block feeds on and the other level feeds off; changing a list by hand
//! afterwards no longer matches any level, so the level reads back as `Custom`.
//! The orchestration that applies a level lives with the handlers in `lib.rs`;
//! this module holds the pure mapping and the "which level are we at" check so
//! both can be unit-tested without a server.

use std::collections::HashSet;

use justdns_config::{BlockLevel, Settings};
use justdns_lists::{ListKind, ListsView};

/// The block feeds and coarse switches one concrete level stands for.
pub struct LevelTargets {
    /// Catalogue keys of the block feeds this level turns on (and, by omission,
    /// the ones it turns off — every other level feed).
    pub presets: &'static [&'static str],
    /// Whether the built-in ads/trackers switch is on at this level.
    pub block_ads: bool,
    /// Whether the built-in malware switch is on at this level.
    pub block_malware: bool,
}

/// What a concrete level maps to, or `None` for `Custom` (which is not a target
/// one can apply — it only ever results from hand-tuning).
pub fn level_targets(level: BlockLevel) -> Option<LevelTargets> {
    Some(match level {
        BlockLevel::Off => LevelTargets {
            presets: &[],
            block_ads: false,
            block_malware: false,
        },
        BlockLevel::Minimal => LevelTargets {
            presets: &["hagezi-multi"],
            block_ads: true,
            block_malware: false,
        },
        BlockLevel::Standard => LevelTargets {
            presets: &["hagezi-multi", "hagezi-tif"],
            block_ads: true,
            block_malware: true,
        },
        BlockLevel::Extreme => LevelTargets {
            presets: &["hagezi-pro-plus", "hagezi-tif", "hagezi-doh-vpn"],
            block_ads: true,
            block_malware: true,
        },
        BlockLevel::Custom => return None,
    })
}

/// The concrete levels, in a fixed order. Their feed sets are distinct, so at
/// most one ever matches the live lists.
const CONCRETE: [BlockLevel; 4] = [
    BlockLevel::Off,
    BlockLevel::Minimal,
    BlockLevel::Standard,
    BlockLevel::Extreme,
];

/// The keys of the block feeds currently switched on in a lists view.
fn enabled_block_keys(view: &ListsView) -> HashSet<&str> {
    view.presets
        .iter()
        .filter(|pv| pv.enabled && pv.preset.kind == ListKind::Block)
        .map(|pv| pv.preset.key)
        .collect()
}

/// Which level the live block feeds and switches currently represent, or
/// `Custom` if they match none. Allow feeds are ignored — the block level is
/// about blocking, so switching an allow list on never changes it.
pub fn effective_level(view: &ListsView, settings: &Settings) -> BlockLevel {
    let enabled = enabled_block_keys(view);
    for level in CONCRETE {
        let targets = level_targets(level).expect("a concrete level has targets");
        let want: HashSet<&str> = targets.presets.iter().copied().collect();
        if want == enabled
            && targets.block_ads == settings.block_ads_trackers
            && targets.block_malware == settings.block_malware
        {
            return level;
        }
    }
    BlockLevel::Custom
}

#[cfg(test)]
mod tests {
    use super::*;
    use justdns_lists::PresetView;

    /// Build a lists view with the given block-preset keys switched on. Only the
    /// fields `effective_level` reads are filled.
    fn view_with(enabled_block: &[&str]) -> ListsView {
        let presets = justdns_lists::presets()
            .iter()
            .map(|preset| PresetView {
                preset: *preset,
                active: enabled_block.contains(&preset.key),
                enabled: enabled_block.contains(&preset.key),
            })
            .collect();
        ListsView {
            allow: Vec::new(),
            block: Vec::new(),
            subscriptions: Vec::new(),
            presets,
            allow_active: 0,
            block_active: 0,
        }
    }

    fn settings_with(ads: bool, malware: bool) -> Settings {
        Settings {
            block_ads_trackers: ads,
            block_malware: malware,
            ..Default::default()
        }
    }

    #[test]
    fn standard_set_reads_back_as_standard() {
        let view = view_with(&["hagezi-multi", "hagezi-tif"]);
        assert_eq!(
            effective_level(&view, &settings_with(true, true)),
            BlockLevel::Standard
        );
    }

    #[test]
    fn off_is_no_feeds_and_no_switches() {
        let view = view_with(&[]);
        assert_eq!(
            effective_level(&view, &settings_with(false, false)),
            BlockLevel::Off
        );
        // Feeds off but a switch still on is not "Off" — it is custom.
        assert_eq!(
            effective_level(&view, &settings_with(true, false)),
            BlockLevel::Custom
        );
    }

    #[test]
    fn an_extra_block_feed_makes_it_custom() {
        let view = view_with(&["hagezi-multi", "hagezi-tif", "hagezi-pro"]);
        assert_eq!(
            effective_level(&view, &settings_with(true, true)),
            BlockLevel::Custom
        );
    }

    #[test]
    fn an_allow_feed_does_not_change_the_level() {
        // referral-allow is an allow list; switching it on leaves Standard intact.
        let view = view_with(&["hagezi-multi", "hagezi-tif", "hagezi-referral-allow"]);
        assert_eq!(
            effective_level(&view, &settings_with(true, true)),
            BlockLevel::Standard
        );
    }

    #[test]
    fn custom_has_no_targets() {
        assert!(level_targets(BlockLevel::Custom).is_none());
        assert!(level_targets(BlockLevel::Standard).is_some());
    }

    /// Every preset key a level references must exist in the catalogue, or
    /// applying the level would silently skip it.
    #[test]
    fn level_preset_keys_exist_in_the_catalogue() {
        for level in CONCRETE {
            for key in level_targets(level).unwrap().presets {
                assert!(
                    justdns_lists::preset(key).is_some(),
                    "level {level:?} references unknown preset {key}"
                );
            }
        }
    }
}
