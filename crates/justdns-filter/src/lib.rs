// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Blocking engine: decide whether a queried name should be refused.
//!
//! The engine groups blocked domains by category — advertising/tracking and
//! malware/phishing — and each category is gated by a live flag so toggling it
//! in the dashboard takes effect on the next query without a restart, the same
//! way the pipeline's IPv6 flag works.
//!
//! The lists themselves are kept small and compiled in (see `lists/`), so the
//! blocker works with no network access. A later milestone adds a list updater
//! that fetches the large community feeds and merges them on top of these.

mod domain;

pub use domain::{DomainSet, parse_rule};

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use arc_swap::ArcSwap;
use justdns_clients::ClientMatch;

/// The advertising and tracking starter list.
const ADS_LIST: &str = include_str!("../lists/ads.txt");
/// The malware and phishing starter list.
const MALWARE_LIST: &str = include_str!("../lists/malware.txt");

/// The compiled-in advertising/tracking domain set, as a standalone list object
/// the policy coordinator can switch on per scope. Built fresh on each call.
pub fn bundled_ads() -> DomainSet {
    DomainSet::from_rules(ADS_LIST)
}

/// The compiled-in malware/phishing domain set, as a standalone list object the
/// policy coordinator can switch on per scope. Built fresh on each call.
pub fn bundled_malware() -> DomainSet {
    DomainSet::from_rules(MALWARE_LIST)
}

/// Which category, if any, caused a name to be blocked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockCategory {
    /// Advertising or tracking.
    AdsTrackers,
    /// Malware, phishing or fraud.
    Malware,
    /// A domain the user added to their own block list.
    Custom,
}

impl BlockCategory {
    /// A stable machine key for the category, used by the API.
    pub fn as_str(self) -> &'static str {
        match self {
            BlockCategory::AdsTrackers => "ads",
            BlockCategory::Malware => "malware",
            BlockCategory::Custom => "custom",
        }
    }
}

/// Why a name is on a block list: the category whose list holds it and the
/// exact rule that matched. Returned by [`Blocker::explain`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockMatch {
    /// The broad category the rule belongs to.
    pub category: BlockCategory,
    /// The list entry that matched — the queried name or a parent domain of it.
    pub rule: String,
    /// For a [`BlockCategory::Custom`] hit, the display name of the exact list
    /// that matched (the user's own list or a subscribed feed). `None` for the
    /// bundled ads/malware categories, which already have fixed labels.
    pub list: Option<String>,
}

/// One named source in a resolved block set: a bundled category (ads/malware),
/// the user's own manual list, or a single subscribed feed. The sources are kept
/// separate (rather than merged into one set) so a block can name the exact list
/// that caught the query and report the right category.
#[derive(Clone, Debug)]
pub struct BlockSource {
    /// Which broad category this source belongs to. A [`Policy`] reports this as
    /// the reason a name was blocked; for [`BlockCategory::Custom`] the `label`
    /// is also surfaced as the exact list.
    pub category: BlockCategory,
    /// Display name shown in the dashboard, e.g. "Eigene Blockliste" or
    /// "Hagezi Normal".
    pub label: String,
    /// The domains this source blocks.
    pub set: DomainSet,
}

/// The blocking engine. Holds the bundled category lists (each gated by a live
/// flag) plus the user's own allow and block lists, which can be swapped at
/// runtime so a dashboard edit takes effect on the next query. The allow list
/// wins over every block list. Cheap to share: clone the [`Arc`] the binary
/// wraps it in.
#[derive(Debug)]
pub struct Blocker {
    ads: DomainSet,
    ads_on: Arc<AtomicBool>,
    malware: DomainSet,
    malware_on: Arc<AtomicBool>,
    /// The user's allow list: a match here is never blocked, whatever the block
    /// lists say. Replaced wholesale on every edit.
    allow: ArcSwap<DomainSet>,
    /// The user's own block set as an ordered list of named sources — their
    /// manual list plus each enabled block subscription. Always on (they turned
    /// them on). The first source that matches is the one a block reports.
    custom: ArcSwap<Vec<BlockSource>>,
}

impl Blocker {
    /// Build a blocker from explicit category sets and live flags, with empty
    /// user lists. Used by tests and by anything that loads its own lists.
    pub fn new(
        ads: DomainSet,
        ads_on: Arc<AtomicBool>,
        malware: DomainSet,
        malware_on: Arc<AtomicBool>,
    ) -> Self {
        Self {
            ads,
            ads_on,
            malware,
            malware_on,
            allow: ArcSwap::from_pointee(DomainSet::new()),
            custom: ArcSwap::from_pointee(Vec::new()),
        }
    }

    /// Build a blocker from the compiled-in starter lists, gated by the given
    /// live flags (typically taken from the configuration store).
    pub fn with_bundled_lists(ads_on: Arc<AtomicBool>, malware_on: Arc<AtomicBool>) -> Self {
        Self::new(
            DomainSet::from_rules(ADS_LIST),
            ads_on,
            DomainSet::from_rules(MALWARE_LIST),
            malware_on,
        )
    }

    /// Replace the user's allow list. Takes effect on the next query.
    pub fn set_allow(&self, allow: DomainSet) {
        self.allow.store(Arc::new(allow));
    }

    /// Replace the user's own block set: their manual list plus each enabled
    /// subscription, as an ordered list of named sources. The order is the
    /// search order, so the first source that matches is the one reported.
    /// Takes effect on the next query.
    pub fn set_block_sources(&self, sources: Vec<BlockSource>) {
        self.custom.store(Arc::new(sources));
    }

    /// The category that blocks `name`, or `None` if it is allowed. The allow
    /// list is checked first and overrides every block list; a bundled category
    /// whose flag is off never blocks.
    pub fn classify(&self, name: &str) -> Option<BlockCategory> {
        // The allow list wins over everything, so a name on it is never blocked.
        if self.allow.load().blocks(name) {
            return None;
        }
        if self.malware_on.load(Ordering::Relaxed) && self.malware.blocks(name) {
            return Some(BlockCategory::Malware);
        }
        for source in self.custom.load().iter() {
            if source.set.blocks(name) {
                return Some(BlockCategory::Custom);
            }
        }
        if self.ads_on.load(Ordering::Relaxed) && self.ads.blocks(name) {
            return Some(BlockCategory::AdsTrackers);
        }
        None
    }

    /// Whether `name` is blocked under the current flags and lists.
    pub fn blocks(&self, name: &str) -> bool {
        self.classify(name).is_some()
    }

    /// Explain why `name` would be blocked, *ignoring* the bundled on/off flags,
    /// so a query already logged as blocked can be explained even if a category
    /// was since switched off. The allow list still wins, so an allow-listed
    /// name explains to `None`. Order matches [`classify`]: malware, custom, ads.
    ///
    /// [`classify`]: Self::classify
    pub fn explain(&self, name: &str) -> Option<BlockMatch> {
        if self.allow.load().blocks(name) {
            return None;
        }
        if let Some(rule) = self.malware.matching(name) {
            return Some(BlockMatch {
                category: BlockCategory::Malware,
                rule,
                list: None,
            });
        }
        for source in self.custom.load().iter() {
            if let Some(rule) = source.set.matching(name) {
                return Some(BlockMatch {
                    category: BlockCategory::Custom,
                    rule,
                    list: Some(source.label.clone()),
                });
            }
        }
        if let Some(rule) = self.ads.matching(name) {
            return Some(BlockMatch {
                category: BlockCategory::AdsTrackers,
                rule,
                list: None,
            });
        }
        None
    }

    /// Number of domains in the advertising/tracking list.
    pub fn ads_len(&self) -> usize {
        self.ads.len()
    }

    /// Number of domains in the malware/phishing list.
    pub fn malware_len(&self) -> usize {
        self.malware.len()
    }

    /// Number of domains on the user's allow list.
    pub fn allow_len(&self) -> usize {
        self.allow.load().len()
    }

    /// Total number of domains across the user's own block sources (manual list
    /// plus enabled subscriptions). Overlaps across sources are counted once per
    /// source, so this is an upper bound on the distinct blocked names.
    pub fn custom_len(&self) -> usize {
        self.custom
            .load()
            .iter()
            .map(|source| source.set.len())
            .sum()
    }
}

/// A fully resolved filtering policy for one scope — global, a group, or a
/// device. Holds the allow set that wins over everything plus an ordered list
/// of block sources. The policy coordinator builds it by folding the global
/// lists with a group's and a device's deltas, so matching on the hot path is a
/// plain set lookup with no inheritance to walk at query time. The source order
/// is the search order: the first source that matches is the one a block
/// reports, so the coordinator orders them malware, custom, ads to keep the
/// historical precedence.
#[derive(Clone, Debug, Default)]
pub struct Policy {
    allow: DomainSet,
    block_sources: Vec<BlockSource>,
}

impl Policy {
    /// Build a policy from an allow set and already-ordered block sources.
    pub fn new(allow: DomainSet, block_sources: Vec<BlockSource>) -> Self {
        Self {
            allow,
            block_sources,
        }
    }

    /// The category that blocks `name`, or `None` if it is allowed or on no
    /// list. The allow set is checked first and overrides every block source.
    pub fn classify(&self, name: &str) -> Option<BlockCategory> {
        if self.allow.blocks(name) {
            return None;
        }
        self.block_sources
            .iter()
            .find(|source| source.set.blocks(name))
            .map(|source| source.category)
    }

    /// Whether `name` is blocked under this policy.
    pub fn blocks(&self, name: &str) -> bool {
        self.classify(name).is_some()
    }

    /// Explain why `name` is blocked: the category, the exact rule that matched,
    /// and — for a custom source — the list's display name. The allow set still
    /// wins, so an allow-listed name explains to `None`.
    pub fn explain(&self, name: &str) -> Option<BlockMatch> {
        if self.allow.blocks(name) {
            return None;
        }
        for source in &self.block_sources {
            if let Some(rule) = source.set.matching(name) {
                return Some(BlockMatch {
                    category: source.category,
                    rule,
                    list: match source.category {
                        BlockCategory::Custom => Some(source.label.clone()),
                        _ => None,
                    },
                });
            }
        }
        None
    }
}

/// The live, client-scoped filtering engine the DNS pipeline consults. Holds a
/// global default policy plus a per-device policy for every named device, and
/// resolves a query's client address to the policy that applies — an exact
/// single-address device first, then the most specific range containing it,
/// otherwise the global default. Swapped wholesale on every change (lists,
/// groups, devices, flags) so a dashboard edit takes effect on the next query
/// without a restart.
pub struct PolicyEngine {
    resolved: ArcSwap<Resolved>,
}

/// The resolved snapshot behind the engine. Devices are split into an exact
/// single-address map and a prefix-sorted range list, so resolving a client is
/// a hash lookup and then at most a short scan of the ranges.
#[derive(Default)]
struct Resolved {
    global: Policy,
    by_ip: HashMap<IpAddr, Arc<Policy>>,
    nets: Vec<(ClientMatch, Arc<Policy>)>,
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyEngine {
    /// An engine that blocks nothing: an empty global policy and no devices.
    pub fn new() -> Self {
        Self {
            resolved: ArcSwap::from_pointee(Resolved::default()),
        }
    }

    /// Replace the whole resolved set. `devices` pairs each device's client
    /// matcher with its fully resolved policy; order does not matter, the engine
    /// indexes them by specificity. Among duplicate single addresses the first
    /// wins. Takes effect on the next query.
    pub fn publish(&self, global: Policy, devices: Vec<(ClientMatch, Policy)>) {
        let mut by_ip: HashMap<IpAddr, Arc<Policy>> = HashMap::new();
        let mut nets: Vec<(ClientMatch, Arc<Policy>)> = Vec::new();
        for (matcher, policy) in devices {
            let policy = Arc::new(policy);
            match matcher {
                ClientMatch::Ip(addr) => {
                    by_ip.entry(addr).or_insert(policy);
                }
                net @ ClientMatch::Net { .. } => nets.push((net, policy)),
            }
        }
        // Longest prefix first, so resolution takes the first containing range.
        nets.sort_by_key(|(matcher, _)| std::cmp::Reverse(matcher.prefix_len()));
        self.resolved.store(Arc::new(Resolved {
            global,
            by_ip,
            nets,
        }));
    }

    /// The category that blocks `name` for `client`, or `None`.
    pub fn classify(&self, client: IpAddr, name: &str) -> Option<BlockCategory> {
        self.resolved.load().policy_for(client).classify(name)
    }

    /// Whether `name` is blocked for `client`.
    pub fn blocks(&self, client: IpAddr, name: &str) -> bool {
        self.classify(client, name).is_some()
    }

    /// Explain why `name` is blocked for `client`, ignoring nothing — the
    /// resolved policy already reflects every active flag and list.
    pub fn explain(&self, client: IpAddr, name: &str) -> Option<BlockMatch> {
        self.resolved.load().policy_for(client).explain(name)
    }
}

impl Resolved {
    /// The policy that governs `client`: an exact device match first, then the
    /// most specific range containing it, otherwise the global default.
    fn policy_for(&self, client: IpAddr) -> &Policy {
        if let Some(policy) = self.by_ip.get(&client) {
            return policy;
        }
        self.nets
            .iter()
            .find(|(matcher, _)| matcher.contains(client))
            .map(|(_, policy)| policy.as_ref())
            .unwrap_or(&self.global)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flag(on: bool) -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(on))
    }

    fn blocker(ads_on: bool, malware_on: bool) -> Blocker {
        Blocker::new(
            DomainSet::from_rules("ads.example\n"),
            flag(ads_on),
            DomainSet::from_rules("malware.example\n"),
            flag(malware_on),
        )
    }

    #[test]
    fn blocks_only_enabled_categories() {
        let b = blocker(true, false);
        assert_eq!(b.classify("ads.example"), Some(BlockCategory::AdsTrackers));
        assert_eq!(
            b.classify("sub.ads.example"),
            Some(BlockCategory::AdsTrackers)
        );
        assert_eq!(b.classify("malware.example"), None, "malware flag is off");
        assert_eq!(b.classify("allowed.example"), None);
    }

    #[test]
    fn flags_take_effect_live() {
        let ads_on = flag(false);
        let b = Blocker::new(
            DomainSet::from_rules("ads.example\n"),
            ads_on.clone(),
            DomainSet::new(),
            flag(false),
        );
        assert!(!b.blocks("ads.example"));
        ads_on.store(true, Ordering::Relaxed);
        assert!(
            b.blocks("ads.example"),
            "flip without rebuilding the blocker"
        );
    }

    #[test]
    fn malware_is_reported_ahead_of_ads() {
        let b = Blocker::new(
            DomainSet::from_rules("both.example\n"),
            flag(true),
            DomainSet::from_rules("both.example\n"),
            flag(true),
        );
        assert_eq!(b.classify("both.example"), Some(BlockCategory::Malware));
    }

    #[test]
    fn bundled_lists_load_and_match() {
        let b = Blocker::with_bundled_lists(flag(true), flag(true));
        assert!(b.ads_len() > 10, "ads starter list should be non-trivial");
        assert!(b.malware_len() >= 1);
        // A domain from the shipped ads list and a sub-domain of it.
        assert!(b.blocks("doubleclick.net"));
        assert!(b.blocks("ad.doubleclick.net"));
        // An ordinary name is left alone.
        assert!(!b.blocks("example.com"));
    }

    #[test]
    fn disabled_blocker_allows_everything() {
        let b = Blocker::with_bundled_lists(flag(false), flag(false));
        assert!(!b.blocks("doubleclick.net"));
    }

    #[test]
    fn allow_list_overrides_every_block_list() {
        let b = Blocker::with_bundled_lists(flag(true), flag(true));
        assert!(b.blocks("doubleclick.net"), "blocked before allow-listing");

        b.set_allow(DomainSet::from_rules("doubleclick.net\n"));
        assert!(!b.blocks("doubleclick.net"), "allow list wins");
        assert!(!b.blocks("ad.doubleclick.net"), "and covers sub-domains");
        assert_eq!(b.classify("doubleclick.net"), None);
        // explain also respects the allow list.
        assert_eq!(b.explain("doubleclick.net"), None);
    }

    #[test]
    fn custom_block_list_is_always_on_and_live() {
        // Both bundled categories off, so only the custom list can block.
        let b = blocker(false, false);
        assert!(!b.blocks("tracker.test"));

        b.set_block_sources(vec![BlockSource {
            category: BlockCategory::Custom,
            label: "Eigene Blockliste".into(),
            set: DomainSet::from_rules("tracker.test\n"),
        }]);
        assert_eq!(b.classify("tracker.test"), Some(BlockCategory::Custom));
        assert_eq!(b.classify("a.tracker.test"), Some(BlockCategory::Custom));
        assert_eq!(BlockCategory::Custom.as_str(), "custom");

        // Clearing it stops blocking, live.
        b.set_block_sources(vec![]);
        assert!(!b.blocks("tracker.test"));
    }

    #[test]
    fn explain_names_the_specific_block_source() {
        let b = blocker(false, false);
        b.set_block_sources(vec![
            BlockSource {
                category: BlockCategory::Custom,
                label: "Eigene Blockliste".into(),
                set: DomainSet::from_rules("mine.test\n"),
            },
            BlockSource {
                category: BlockCategory::Custom,
                label: "Hagezi Normal".into(),
                set: DomainSet::from_rules("ads.feed.test\n"),
            },
        ]);

        let m = b.explain("x.ads.feed.test").expect("on the feed");
        assert_eq!(m.category, BlockCategory::Custom);
        assert_eq!(m.list.as_deref(), Some("Hagezi Normal"));
        assert_eq!(m.rule, "ads.feed.test", "reports the parent rule that hit");

        let m = b.explain("mine.test").expect("on the own list");
        assert_eq!(m.list.as_deref(), Some("Eigene Blockliste"));
    }

    #[test]
    fn allow_list_beats_custom_block_for_the_same_name() {
        let b = blocker(false, false);
        b.set_block_sources(vec![BlockSource {
            category: BlockCategory::Custom,
            label: "Eigene Blockliste".into(),
            set: DomainSet::from_rules("intra.test\n"),
        }]);
        b.set_allow(DomainSet::from_rules("intra.test\n"));
        assert!(!b.blocks("intra.test"), "allow wins over custom block");
    }

    #[test]
    fn explain_names_the_list_and_rule_regardless_of_flags() {
        // Both flags off, yet explain still reports why a name is on a list.
        let b = blocker(false, false);
        let m = b.explain("sub.ads.example").expect("on the ads list");
        assert_eq!(m.category, BlockCategory::AdsTrackers);
        assert_eq!(m.category.as_str(), "ads");
        assert_eq!(m.rule, "ads.example", "reports the parent rule that hit");

        let m = b.explain("malware.example").expect("on the malware list");
        assert_eq!(m.category, BlockCategory::Malware);

        assert_eq!(b.explain("allowed.example"), None);
    }

    // --- Policy and the client-scoped PolicyEngine ---

    fn source(category: BlockCategory, label: &str, rules: &str) -> BlockSource {
        BlockSource {
            category,
            label: label.to_owned(),
            set: DomainSet::from_rules(rules),
        }
    }

    #[test]
    fn policy_allow_wins_and_reports_the_source_category() {
        let policy = Policy::new(
            DomainSet::from_rules("safe.example\n"),
            vec![
                source(BlockCategory::Malware, "Malware", "evil.example\n"),
                source(BlockCategory::Custom, "Eigene Blockliste", "ads.example\n"),
            ],
        );
        assert_eq!(
            policy.classify("evil.example"),
            Some(BlockCategory::Malware)
        );
        assert_eq!(
            policy.classify("x.ads.example"),
            Some(BlockCategory::Custom)
        );
        // The allow set wins even when a block source would catch the name.
        let policy = Policy::new(
            DomainSet::from_rules("ads.example\n"),
            vec![source(
                BlockCategory::Custom,
                "Eigene Blockliste",
                "ads.example\n",
            )],
        );
        assert_eq!(policy.classify("ads.example"), None, "allow wins");
        assert!(policy.explain("ads.example").is_none());
    }

    #[test]
    fn policy_explain_names_the_list_only_for_custom() {
        let policy = Policy::new(
            DomainSet::new(),
            vec![
                source(BlockCategory::Malware, "Malware", "evil.example\n"),
                source(BlockCategory::Custom, "Hagezi", "ads.feed\n"),
            ],
        );
        assert_eq!(policy.explain("evil.example").unwrap().list, None);
        assert_eq!(
            policy.explain("x.ads.feed").unwrap().list.as_deref(),
            Some("Hagezi")
        );
    }

    #[test]
    fn engine_unknown_client_falls_back_to_global() {
        let engine = PolicyEngine::new();
        let global = Policy::new(
            DomainSet::new(),
            vec![source(BlockCategory::Custom, "Global", "blocked.example\n")],
        );
        engine.publish(global, vec![]);
        let client = "10.0.0.5".parse().unwrap();
        assert!(engine.blocks(client, "blocked.example"));
        assert!(!engine.blocks(client, "open.example"));
    }

    #[test]
    fn engine_device_policy_overrides_global_for_that_client() {
        let engine = PolicyEngine::new();
        let global = Policy::new(
            DomainSet::new(),
            vec![source(BlockCategory::Custom, "Global", "ads.example\n")],
        );
        // The device both un-blocks ads (empty block set) by having its own
        // resolved policy and blocks a name of its own instead.
        let device = Policy::new(
            DomainSet::new(),
            vec![source(BlockCategory::Custom, "Kids", "games.example\n")],
        );
        let matcher = justdns_clients::parse_target("192.168.20.10").unwrap();
        engine.publish(global, vec![(matcher, device)]);

        let kid = "192.168.20.10".parse().unwrap();
        let other = "192.168.20.11".parse().unwrap();
        // The named device gets its own policy ...
        assert!(engine.blocks(kid, "games.example"));
        assert!(!engine.blocks(kid, "ads.example"));
        // ... while everyone else still falls to the global default.
        assert!(engine.blocks(other, "ads.example"));
        assert!(!engine.blocks(other, "games.example"));
    }

    #[test]
    fn engine_prefers_the_most_specific_device_match() {
        let engine = PolicyEngine::new();
        let global = Policy::new(DomainSet::new(), vec![]);
        let range = Policy::new(
            DomainSet::new(),
            vec![source(BlockCategory::Custom, "Guest range", "r.example\n")],
        );
        let host = Policy::new(
            DomainSet::new(),
            vec![source(BlockCategory::Custom, "Guest host", "h.example\n")],
        );
        engine.publish(
            global,
            vec![
                (
                    justdns_clients::parse_target("192.168.30.0/24").unwrap(),
                    range,
                ),
                (
                    justdns_clients::parse_target("192.168.30.10").unwrap(),
                    host,
                ),
            ],
        );
        let host_ip = "192.168.30.10".parse().unwrap();
        let range_ip = "192.168.30.20".parse().unwrap();
        // The exact host uses the host policy, not the enclosing range.
        assert!(engine.blocks(host_ip, "h.example"));
        assert!(!engine.blocks(host_ip, "r.example"));
        // Another address in the range uses the range policy.
        assert!(engine.blocks(range_ip, "r.example"));
    }
}
