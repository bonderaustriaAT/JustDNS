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

use std::collections::{HashMap, HashSet};

use justdns_clients::{ClientStore, PolicyDelta, parse_target};
use justdns_config::Settings;
use justdns_filter::{
    BlockCategory, BlockSource, DomainSet, Policy, PolicyEngine, bundled_ads, bundled_malware,
};
use justdns_lists::{ListObject, ListStore};
use justdns_schedule::{EffectMode, Schedule, TargetScope};

/// The id of the built-in advertising/tracking list object.
const ADS_ID: &str = "ads";
/// The id of the built-in malware/phishing list object.
const MALWARE_ID: &str = "malware";

/// One blockable service resolved into a ready-to-use block source: its
/// list-object id (`svc:<id>`), its display name and the domains it blocks.
struct ServiceObject {
    id: String,
    name: String,
    set: DomainSet,
}

/// The building blocks every scope's policy is assembled from: the global allow
/// set and own block list, the two bundled categories with their global default,
/// the user's block feeds, and the blockable-services catalogue. Built once per
/// rebuild and shared across scopes.
struct Catalog {
    allow: DomainSet,
    own_block: DomainSet,
    ads: DomainSet,
    ads_default: bool,
    malware: DomainSet,
    malware_default: bool,
    feeds: Vec<ListObject>,
    /// The whole compiled-in services catalogue, resolved to domain sets.
    services: Vec<ServiceObject>,
    /// The service ids (bare, e.g. `tiktok`) switched on in the global default.
    blocked_services: Vec<String>,
}

impl Catalog {
    fn build(lists: &ListStore, settings: &Settings) -> Self {
        let resolved = lists.resolved();
        let services = justdns_filter::services()
            .iter()
            .map(|service| ServiceObject {
                id: service.object_id(),
                name: service.name.to_owned(),
                set: service.domain_set(),
            })
            .collect();
        Self {
            allow: resolved.allow,
            own_block: resolved.own_block,
            ads: bundled_ads(),
            ads_default: settings.block_ads_trackers,
            malware: bundled_malware(),
            malware_default: settings.block_malware,
            feeds: resolved.block_feeds,
            services,
            blocked_services: settings.blocked_services.clone(),
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
        for service in &self.services {
            let bare = service
                .id
                .strip_prefix(justdns_filter::SERVICE_PREFIX)
                .unwrap_or(&service.id);
            if self.blocked_services.iter().any(|on| on == bare) {
                ids.insert(service.id.clone());
            }
        }
        ids
    }

    /// Assemble a resolved policy from the set of active list ids, extra ad-hoc
    /// allow domains (from the applicable scopes) and extra ad-hoc block sources
    /// (already ordered group-then-device). Block sources are ordered malware,
    /// custom (own list, ad-hoc, feeds), ads, so a block reports the most
    /// meaningful source first.
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
        for service in &self.services {
            if active.contains(&service.id) {
                sources.push(BlockSource {
                    category: BlockCategory::Custom,
                    label: service.name.clone(),
                    set: service.set.clone(),
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

/// Rebuild every policy from the current lists, groups, devices, flags and the
/// schedules open right now, and publish them to `engine`. The global default
/// applies to any client without a device entry; each device that has a group, a
/// delta of its own, or a schedule aimed at it (or its group) gets a resolved
/// policy that folds global → group → device and then the active schedules.
///
/// A schedule in `active` is assumed open now — the caller filters by the clock in
/// the configured time zone (see [`justdns_schedule::active_now`]). Its effect is
/// either an extra [`PolicyDelta`] or a pause that blocks everything for the scope
/// (the allow set still wins). Devices that carry only a name and have no active
/// schedule are left out — they resolve to the global default.
pub fn rebuild(
    engine: &PolicyEngine,
    lists: &ListStore,
    clients: &ClientStore,
    settings: &Settings,
    active: &[&Schedule],
) {
    let mut catalog = Catalog::build(lists, settings);

    // Partition the open schedules by the scope they target. Device targets are
    // normalised to match the stored device targets; one that does not parse is
    // dropped.
    let mut global_scheds: Vec<&Schedule> = Vec::new();
    let mut group_scheds: HashMap<String, Vec<&Schedule>> = HashMap::new();
    let mut device_scheds: HashMap<String, Vec<&Schedule>> = HashMap::new();
    for &schedule in active {
        match schedule.target.scope {
            TargetScope::Global => global_scheds.push(schedule),
            TargetScope::Group => {
                if let Some(id) = &schedule.target.id {
                    group_scheds.entry(id.clone()).or_default().push(schedule);
                }
            }
            TargetScope::Device => {
                if let Some(raw) = &schedule.target.id {
                    if let Some(matcher) = parse_target(raw) {
                        device_scheds
                            .entry(matcher.to_string())
                            .or_default()
                            .push(schedule);
                    }
                }
            }
        }
    }

    // Fold the global schedules once: their list switches seed the global active
    // set (so every device inherits them), their allow domains join the shared base
    // allow set, their ad-hoc blocks become sources carried by every scope, and a
    // global pause marks every scope paused.
    let mut global_active = catalog.global_active();
    let mut global_block: Vec<BlockSource> = Vec::new();
    let mut global_pause: Option<String> = None;
    for &schedule in &global_scheds {
        fold_schedule(
            schedule,
            &mut global_active,
            &mut catalog.allow,
            &mut global_block,
            &mut global_pause,
        );
    }
    let mut global = catalog.policy(&global_active, &[], global_block.clone());
    if let Some(label) = &global_pause {
        global = global.into_paused(label.clone());
    }

    let groups = clients.groups();
    let devices_list = clients.view();
    let mut devices = Vec::new();
    for device in &devices_list {
        let in_group_sched = device
            .group
            .as_ref()
            .is_some_and(|g| group_scheds.contains_key(g));
        let in_device_sched = device_scheds.contains_key(&device.target);
        if device.group.is_none() && device.policy.is_empty() && !in_group_sched && !in_device_sched
        {
            continue;
        }
        let Some(matcher) = parse_target(&device.target) else {
            continue;
        };

        let mut active_ids = global_active.clone();
        let mut extra_allow: Vec<&[String]> = Vec::new();
        let mut extra_block: Vec<BlockSource> = global_block.clone();
        let mut pause = global_pause.clone();

        if let Some(group_id) = &device.group {
            if let Some(group) = groups.iter().find(|g| &g.id == group_id) {
                apply_delta(&mut active_ids, &group.policy);
                extra_allow.push(&group.policy.allow);
                if let Some(source) =
                    adhoc_block(format!("Gruppe: {}", group.name), &group.policy.block)
                {
                    extra_block.push(source);
                }
            }
            for &schedule in group_scheds.get(group_id).into_iter().flatten() {
                fold_scope_schedule(
                    schedule,
                    &mut active_ids,
                    &mut extra_allow,
                    &mut extra_block,
                    &mut pause,
                );
            }
        }
        apply_delta(&mut active_ids, &device.policy);
        extra_allow.push(&device.policy.allow);
        if let Some(source) = adhoc_block(format!("Gerät: {}", device.name), &device.policy.block)
        {
            extra_block.push(source);
        }
        for &schedule in device_scheds.get(&device.target).into_iter().flatten() {
            fold_scope_schedule(
                schedule,
                &mut active_ids,
                &mut extra_allow,
                &mut extra_block,
                &mut pause,
            );
        }

        let mut policy = catalog.policy(&active_ids, &extra_allow, extra_block);
        if let Some(label) = pause {
            policy = policy.into_paused(label);
        }
        devices.push((matcher, policy));
    }

    // Device schedules aimed at a client that is not a named device still need
    // their own resolved policy, built on the global base.
    for (target, scheds) in &device_scheds {
        if devices_list.iter().any(|d| &d.target == target) {
            continue;
        }
        let Some(matcher) = parse_target(target) else {
            continue;
        };
        let mut active_ids = global_active.clone();
        let mut extra_allow: Vec<&[String]> = Vec::new();
        let mut extra_block: Vec<BlockSource> = global_block.clone();
        let mut pause = global_pause.clone();
        for &schedule in scheds.iter() {
            fold_scope_schedule(
                schedule,
                &mut active_ids,
                &mut extra_allow,
                &mut extra_block,
                &mut pause,
            );
        }
        let mut policy = catalog.policy(&active_ids, &extra_allow, extra_block);
        if let Some(label) = pause {
            policy = policy.into_paused(label);
        }
        devices.push((matcher, policy));
    }

    engine.publish(global, devices);
}

/// Fold one active *global* schedule into the global base: list switches into
/// `active`, allow domains into the shared `allow` set, ad-hoc blocks into
/// `block`, and a pause into `pause` (first pause wins the label).
fn fold_schedule(
    schedule: &Schedule,
    active: &mut HashSet<String>,
    allow: &mut DomainSet,
    block: &mut Vec<BlockSource>,
    pause: &mut Option<String>,
) {
    match schedule.effect.mode {
        EffectMode::Pause => {
            pause.get_or_insert_with(|| schedule.name.clone());
        }
        EffectMode::Block => {
            apply_delta(active, &schedule.effect.delta);
            for domain in &schedule.effect.delta.allow {
                allow.add_rules(domain);
            }
            if let Some(source) = adhoc_block(
                format!("Zeitplan: {}", schedule.name),
                &schedule.effect.delta.block,
            ) {
                block.push(source);
            }
        }
    }
}

/// Fold one active group- or device-scoped schedule into a scope being resolved:
/// list switches into `active`, allow domains referenced into `extra_allow`,
/// ad-hoc blocks into `extra_block`, and a pause into `pause`.
fn fold_scope_schedule<'a>(
    schedule: &'a Schedule,
    active: &mut HashSet<String>,
    extra_allow: &mut Vec<&'a [String]>,
    extra_block: &mut Vec<BlockSource>,
    pause: &mut Option<String>,
) {
    match schedule.effect.mode {
        EffectMode::Pause => {
            pause.get_or_insert_with(|| schedule.name.clone());
        }
        EffectMode::Block => {
            apply_delta(active, &schedule.effect.delta);
            extra_allow.push(&schedule.effect.delta.allow);
            if let Some(source) = adhoc_block(
                format!("Zeitplan: {}", schedule.name),
                &schedule.effect.delta.block,
            ) {
                extra_block.push(source);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use justdns_clients::{ClientStore, PolicyDelta};
    use justdns_config::Settings;
    use justdns_filter::{Blocker, PolicyEngine};
    use justdns_lists::ListStore;
    use justdns_schedule::{
        Day, EffectMode, Schedule, ScheduleEffect, ScheduleTarget, TargetScope,
    };

    use super::rebuild;

    /// Empty stores on a throwaway data dir, plus the flags both off so only the
    /// schedules decide. Returns the engine and the stores to drive a rebuild.
    fn harness() -> (tempfile::TempDir, ListStore, ClientStore, Settings) {
        let dir = tempfile::tempdir().unwrap();
        let blocker = Arc::new(Blocker::with_bundled_lists(
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        ));
        let lists = ListStore::load(
            dir.path().join("lists.json"),
            dir.path().join("cache"),
            blocker,
        )
        .unwrap();
        let clients = ClientStore::load(dir.path().join("clients.json")).unwrap();
        let settings = Settings {
            block_ads_trackers: false,
            block_malware: false,
            ..Default::default()
        };
        (dir, lists, clients, settings)
    }

    fn all_days() -> Vec<Day> {
        vec![
            Day::Mon,
            Day::Tue,
            Day::Wed,
            Day::Thu,
            Day::Fri,
            Day::Sat,
            Day::Sun,
        ]
    }

    fn pause(id: &str, name: &str, scope: TargetScope, target_id: Option<&str>) -> Schedule {
        Schedule {
            id: id.to_owned(),
            name: name.to_owned(),
            enabled: true,
            days: all_days(),
            start: "00:00".to_owned(),
            end: "00:00".to_owned(),
            target: ScheduleTarget {
                scope,
                id: target_id.map(str::to_owned),
            },
            effect: ScheduleEffect {
                mode: EffectMode::Pause,
                delta: PolicyDelta::default(),
            },
        }
    }

    #[test]
    fn global_pause_blocks_every_client_when_active() {
        let (_dir, lists, clients, settings) = harness();
        let engine = PolicyEngine::new();
        let client: IpAddr = "10.0.0.9".parse().unwrap();

        // No schedules: nothing blocked (both flags off).
        rebuild(&engine, &lists, &clients, &settings, &[]);
        assert!(!engine.blocks(client, "example.com"));

        // A global pause blocks everyone.
        let sched = pause("p", "Schlafenszeit", TargetScope::Global, None);
        rebuild(&engine, &lists, &clients, &settings, &[&sched]);
        assert!(engine.blocks(client, "example.com"));
        assert!(engine.blocks(client, "anything.test"));
    }

    #[test]
    fn device_pause_blocks_only_its_target_even_if_unnamed() {
        let (_dir, lists, clients, settings) = harness();
        let engine = PolicyEngine::new();
        let target: IpAddr = "192.168.50.5".parse().unwrap();
        let other: IpAddr = "192.168.50.6".parse().unwrap();

        // The target is not a named device, yet a device-scoped pause still gets
        // its own resolved policy.
        let sched = pause("d", "Lernzeit", TargetScope::Device, Some("192.168.50.5"));
        rebuild(&engine, &lists, &clients, &settings, &[&sched]);
        assert!(
            engine.blocks(target, "example.com"),
            "the targeted client is paused"
        );
        assert!(
            !engine.blocks(other, "example.com"),
            "others fall to the open global"
        );
    }

    #[test]
    fn group_block_schedule_adds_a_block_for_members() {
        let (_dir, lists, clients, settings) = harness();
        let group = clients.add_group("Kids", PolicyDelta::default()).unwrap();
        clients
            .set_device_policy("192.168.60.10", Some(&group.id), PolicyDelta::default())
            .unwrap();
        let member: IpAddr = "192.168.60.10".parse().unwrap();
        let engine = PolicyEngine::new();

        let sched = Schedule {
            id: "g".to_owned(),
            name: "Fokus".to_owned(),
            enabled: true,
            days: all_days(),
            start: "00:00".to_owned(),
            end: "00:00".to_owned(),
            target: ScheduleTarget {
                scope: TargetScope::Group,
                id: Some(group.id.clone()),
            },
            effect: ScheduleEffect {
                mode: EffectMode::Block,
                delta: PolicyDelta {
                    block: vec!["youtube.com".to_owned()],
                    ..Default::default()
                },
            },
        };
        // Without the schedule the member is not blocked; with it, youtube is.
        rebuild(&engine, &lists, &clients, &settings, &[]);
        assert!(!engine.blocks(member, "youtube.com"));
        rebuild(&engine, &lists, &clients, &settings, &[&sched]);
        assert!(engine.blocks(member, "youtube.com"));
        assert!(engine.blocks(member, "sub.youtube.com"));
        assert!(
            !engine.blocks(member, "example.com"),
            "only the scheduled name"
        );
    }

    #[test]
    fn global_blocked_service_applies_and_a_device_can_override_it() {
        let (_dir, lists, clients, mut settings) = harness();
        // TikTok is blocked globally; a device blocks YouTube for itself and
        // un-blocks TikTok for itself.
        settings.blocked_services = vec!["tiktok".to_owned()];
        let device: IpAddr = "192.168.70.10".parse().unwrap();
        let other: IpAddr = "192.168.70.11".parse().unwrap();
        clients
            .set_device_policy(
                "192.168.70.10",
                None,
                PolicyDelta {
                    lists_added: vec!["svc:youtube".to_owned()],
                    lists_disabled: vec!["svc:tiktok".to_owned()],
                    ..Default::default()
                },
            )
            .unwrap();
        let engine = PolicyEngine::new();
        rebuild(&engine, &lists, &clients, &settings, &[]);

        // Global default: TikTok blocked, YouTube open, for an unnamed client.
        assert!(engine.blocks(other, "tiktok.com"));
        assert!(engine.blocks(other, "www.tiktok.com"), "sub-domains too");
        assert!(!engine.blocks(other, "youtube.com"));

        // The device flips both: YouTube blocked, TikTok allowed again.
        assert!(engine.blocks(device, "youtube.com"));
        assert!(!engine.blocks(device, "tiktok.com"));
        assert!(!engine.blocks(device, "example.com"));
    }

    #[test]
    fn allow_list_still_wins_over_a_blocked_service() {
        let (_dir, lists, clients, mut settings) = harness();
        settings.blocked_services = vec!["youtube".to_owned()];
        lists.add_allow("youtube.com", None).unwrap();
        let client: IpAddr = "10.1.2.3".parse().unwrap();
        let engine = PolicyEngine::new();
        rebuild(&engine, &lists, &clients, &settings, &[]);
        assert!(
            !engine.blocks(client, "youtube.com"),
            "an allow entry pierces a blocked service"
        );
    }
}
