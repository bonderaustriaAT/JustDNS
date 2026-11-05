// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The policy coordinator: fold the global lists, the groups and the devices
//! into the live, client-scoped [`PolicyEngine`] the DNS pipeline consults.
//!
//! The pieces live in three stores — [`ListStore`] owns the lists and feeds,
//! [`ClientStore`] owns the groups and devices, and the [`Settings`] carry the
//! ads/malware flags — so none of them can build the whole picture alone. This
//! module is where they come together: on startup and after every change (each
//! store runs [`rebuild`] through its on-change hook), it resolves one effective
//! policy for the global default and one per device, and publishes them to the
//! engine. Inheritance is folded here, at rebuild time, so the DNS hot path only
//! ever matches against a ready-made policy.

use std::collections::HashSet;

use justdns_clients::{ClientStore, PolicyDelta, parse_target};
use justdns_config::Settings;
use justdns_filter::{
    BlockCategory, BlockSource, DomainSet, Policy, PolicyEngine, bundled_ads, bundled_malware,
};
use justdns_lists::{ListObject, ListStore};

/// The id of the built-in advertising/tracking list object.
const ADS_ID: &str = "ads";
/// The id of the built-in malware/phishing list object.
const MALWARE_ID: &str = "malware";

/// The building blocks every scope's policy is assembled from: the global allow
/// set and own block list, the two bundled categories with their global default,
/// and the user's block feeds. Built once per rebuild and shared across scopes.
struct Catalog {
    allow: DomainSet,
    own_block: DomainSet,
    ads: DomainSet,
    ads_default: bool,
    malware: DomainSet,
    malware_default: bool,
    feeds: Vec<ListObject>,
}

impl Catalog {
    fn build(lists: &ListStore, settings: &Settings) -> Self {
        let resolved = lists.resolved();
        Self {
            allow: resolved.allow,
            own_block: resolved.own_block,
            ads: bundled_ads(),
            ads_default: settings.block_ads_trackers,
            malware: bundled_malware(),
            malware_default: settings.block_malware,
            feeds: resolved.block_feeds,
        }
    }

    /// The list-object ids on in the global default: the bundled categories per
    /// their config flags, and every feed per its enabled flag.
    fn global_active(&self) -> HashSet<String> {
        let mut ids = HashSet::new();
        if self.ads_default {
            ids.insert(ADS_ID.to_owned());
        }
        if self.malware_default {
            ids.insert(MALWARE_ID.to_owned());
        }
        for feed in &self.feeds {
            if feed.enabled {
                ids.insert(feed.id.clone());
            }
        }
        ids
    }

    /// Assemble a resolved policy from the set of active list ids, extra ad-hoc
    /// allow domains (from the applicable scopes) and extra ad-hoc block sources
    /// (already ordered group-then-device). Block sources are ordered malware,
    /// custom (own list, ad-hoc, feeds), ads, to keep the historical precedence
    /// and so a block reports the most meaningful source first.
    fn policy(
        &self,
        active: &HashSet<String>,
        extra_allow: &[&[String]],
        extra_block: Vec<BlockSource>,
    ) -> Policy {
        let mut allow = self.allow.clone();
        for list in extra_allow {
            for domain in *list {
                allow.add_rules(domain);
            }
        }

        let mut sources = Vec::new();
        if active.contains(MALWARE_ID) {
            sources.push(BlockSource {
                category: BlockCategory::Malware,
                label: "Schadsoftware".to_owned(),
                set: self.malware.clone(),
            });
        }
        if !self.own_block.is_empty() {
            sources.push(BlockSource {
                category: BlockCategory::Custom,
                label: "Eigene Blockliste".to_owned(),
                set: self.own_block.clone(),
            });
        }
        sources.extend(extra_block);
        for feed in &self.feeds {
            if active.contains(&feed.id) {
                sources.push(BlockSource {
                    category: BlockCategory::Custom,
                    label: feed.label.clone(),
                    set: feed.set.clone(),
                });
            }
        }
        if active.contains(ADS_ID) {
            sources.push(BlockSource {
                category: BlockCategory::AdsTrackers,
                label: "Werbung & Tracker".to_owned(),
                set: self.ads.clone(),
            });
        }

        Policy::new(allow, sources)
    }
}

/// Apply a scope's list delta to the running set of active ids: switch on the
/// ones it adds, switch off the ones it disables.
fn apply_delta(active: &mut HashSet<String>, delta: &PolicyDelta) {
    for id in &delta.lists_added {
        active.insert(id.clone());
    }
    for id in &delta.lists_disabled {
        active.remove(id);
    }
}

/// A block source from a scope's ad-hoc block domains, or `None` when it has
/// none. Labelled so the log can name the scope that blocked a query.
fn adhoc_block(label: String, domains: &[String]) -> Option<BlockSource> {
    if domains.is_empty() {
        return None;
    }
    let mut set = DomainSet::new();
    for domain in domains {
        set.add_rules(domain);
    }
    Some(BlockSource {
        category: BlockCategory::Custom,
        label,
        set,
    })
}

/// Rebuild every policy from the current lists, groups, devices and flags, and
/// publish them to `engine`. The global default applies to any client without a
/// device entry; each device that has a group or a delta of its own gets a
/// resolved policy that folds global → group → device. Devices that only carry a
/// name (no group, no delta) are left out — they resolve to the global default.
pub fn rebuild(
    engine: &PolicyEngine,
    lists: &ListStore,
    clients: &ClientStore,
    settings: &Settings,
) {
    let catalog = Catalog::build(lists, settings);
    let global_active = catalog.global_active();
    let global = catalog.policy(&global_active, &[], Vec::new());

    let groups = clients.groups();
    let mut devices = Vec::new();
    for device in clients.view() {
        if device.group.is_none() && device.policy.is_empty() {
            continue;
        }
        let Some(matcher) = parse_target(&device.target) else {
            continue;
        };

        let mut active = global_active.clone();
        let mut extra_allow: Vec<&[String]> = Vec::new();
        let mut extra_block: Vec<BlockSource> = Vec::new();

        if let Some(group_id) = &device.group {
            if let Some(group) = groups.iter().find(|g| &g.id == group_id) {
                apply_delta(&mut active, &group.policy);
                extra_allow.push(&group.policy.allow);
                if let Some(source) =
                    adhoc_block(format!("Gruppe: {}", group.name), &group.policy.block)
                {
                    extra_block.push(source);
                }
            }
        }
        apply_delta(&mut active, &device.policy);
        extra_allow.push(&device.policy.allow);
        if let Some(source) = adhoc_block(format!("Gerät: {}", device.name), &device.policy.block)
        {
            extra_block.push(source);
        }

        devices.push((matcher, catalog.policy(&active, &extra_allow, extra_block)));
    }

    engine.publish(global, devices);
}
