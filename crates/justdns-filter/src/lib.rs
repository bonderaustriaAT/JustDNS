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

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The advertising and tracking starter list.
const ADS_LIST: &str = include_str!("../lists/ads.txt");
/// The malware and phishing starter list.
const MALWARE_LIST: &str = include_str!("../lists/malware.txt");

/// Which category, if any, caused a name to be blocked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockCategory {
    /// Advertising or tracking.
    AdsTrackers,
    /// Malware, phishing or fraud.
    Malware,
}

/// The blocking engine. Holds one domain set per category and a live flag for
/// each; a category only blocks while its flag is on. Cheap to share: clone the
/// [`Arc`] the binary wraps it in.
#[derive(Debug)]
pub struct Blocker {
    ads: DomainSet,
    ads_on: Arc<AtomicBool>,
    malware: DomainSet,
    malware_on: Arc<AtomicBool>,
}

impl Blocker {
    /// Build a blocker from explicit domain sets and live flags. Used by tests
    /// and by anything that loads its own lists.
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

    /// The category that blocks `name`, or `None` if it is allowed. A category
    /// whose flag is off never blocks, even if the name is on its list.
    pub fn classify(&self, name: &str) -> Option<BlockCategory> {
        if self.malware_on.load(Ordering::Relaxed) && self.malware.blocks(name) {
            return Some(BlockCategory::Malware);
        }
        if self.ads_on.load(Ordering::Relaxed) && self.ads.blocks(name) {
            return Some(BlockCategory::AdsTrackers);
        }
        None
    }

    /// Whether `name` is blocked under the current flags.
    pub fn blocks(&self, name: &str) -> bool {
        self.classify(name).is_some()
    }

    /// Number of domains in the advertising/tracking list.
    pub fn ads_len(&self) -> usize {
        self.ads.len()
    }

    /// Number of domains in the malware/phishing list.
    pub fn malware_len(&self) -> usize {
        self.malware.len()
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
}
