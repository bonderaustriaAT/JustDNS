// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Schedules: recurring weekly time windows that tighten filtering for a scope.
//!
//! A schedule names a set of weekdays and a start/end time, a target (the global
//! default, a group, or a single device) and an effect that applies *while the
//! window is open*: either a **pause** (block everything for the target, save the
//! allow list) or an extra filtering **delta** (switch on more lists, block or
//! allow more domains). Outside the window the schedule does nothing.
//!
//! This crate owns the data model, the [`ScheduleStore`] (which persists the
//! user's schedules to a JSON file on the data volume) and the time logic that
//! decides which schedules are active now. The binary's policy coordinator folds
//! the active schedules into the resolved per-scope policies at rebuild time, so
//! the DNS hot path still only ever matches a ready-made policy. Evaluation is in
//! a configured IANA time zone so "22:00–07:00" means local bedtime, across DST.

mod store;

pub use store::{ScheduleDraft, ScheduleError, ScheduleStore};

use chrono::{Datelike, Timelike, Utc, Weekday};
use chrono_tz::Tz;
use justdns_clients::PolicyDelta;
use serde::{Deserialize, Serialize};

/// A day of the week, serialised lowercase (`"mon"`..`"sun"`) to match the UI's
/// JSON. A small closed set mapped to [`chrono::Weekday`] for the time maths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Day {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Day {
    /// The matching weekday, for comparing against a local timestamp.
    fn from_weekday(weekday: Weekday) -> Self {
        match weekday {
            Weekday::Mon => Day::Mon,
            Weekday::Tue => Day::Tue,
            Weekday::Wed => Day::Wed,
            Weekday::Thu => Day::Thu,
            Weekday::Fri => Day::Fri,
            Weekday::Sat => Day::Sat,
            Weekday::Sun => Day::Sun,
        }
    }
}

/// Which scope a schedule applies to. `id` names the group or device; it is unset
/// (and ignored) for the global scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetScope {
    /// The global default: everyone who does not get a more specific policy.
    Global,
    /// One device group, by its group id.
    Group,
    /// One device, by its (normalised) client target.
    Device,
}

/// The target of a schedule: a scope plus the id/target it points at. Serialised
/// as `{ "scope": "group", "id": "kids" }`; the global scope carries no id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleTarget {
    pub scope: TargetScope,
    /// The group id or device target this points at. Absent for the global scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// What a schedule does while its window is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffectMode {
    /// Block everything for the target (an "internet pause"); the allow list still
    /// wins, so allow-listed names stay reachable.
    Pause,
    /// Apply the schedule's [`PolicyDelta`] on top of the target's normal policy.
    Block,
}

/// A schedule's effect: a mode plus, for [`EffectMode::Block`], the delta to layer
/// on. The delta is always present in JSON (defaults empty); it is ignored for a
/// pause.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleEffect {
    pub mode: EffectMode,
    #[serde(default)]
    pub delta: PolicyDelta,
}

/// One schedule. `start`/`end` are `"HH:MM"` local times; `end <= start` means the
/// window runs past midnight (the weekday set is read on the day the window
/// *starts*). Serialised camelCase to match the dashboard's JSON directly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    /// Stable id, kept across renames; generated from the name on creation.
    pub id: String,
    /// Display name shown in the dashboard.
    pub name: String,
    /// Master switch. A disabled schedule never applies. Defaults on.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// The weekdays the window opens on (the day it *starts*, for overnight ones).
    pub days: Vec<Day>,
    /// Window start, `"HH:MM"` local.
    pub start: String,
    /// Window end, `"HH:MM"` local. `end <= start` ⇒ the window crosses midnight.
    pub end: String,
    /// The scope this schedule tightens.
    pub target: ScheduleTarget,
    /// What it does while open.
    pub effect: ScheduleEffect,
}

fn default_true() -> bool {
    true
}

impl Schedule {
    /// Whether this schedule is open at a given local weekday and minute-of-day
    /// (minutes since midnight, 0–1439). Honours the `enabled` flag, the weekday
    /// set and the window, including windows that cross midnight. A schedule whose
    /// times do not parse is treated as closed rather than panicking.
    pub fn active_at(&self, weekday: Weekday, minute_of_day: u16) -> bool {
        if !self.enabled || self.days.is_empty() {
            return false;
        }
        let (Some(start), Some(end)) = (parse_hhmm(&self.start), parse_hhmm(&self.end)) else {
            return false;
        };
        let today = Day::from_weekday(weekday);
        let yesterday = Day::from_weekday(weekday.pred());
        if start < end {
            // A plain same-day window: open on a listed day, inside [start, end).
            self.days.contains(&today) && minute_of_day >= start && minute_of_day < end
        } else if start > end {
            // Overnight: the window belongs to the day it starts on. It is open in
            // the evening of a listed day (t >= start) and in the small hours of
            // the following day (t < end, the listed day being yesterday).
            (self.days.contains(&today) && minute_of_day >= start)
                || (self.days.contains(&yesterday) && minute_of_day < end)
        } else {
            // start == end: the whole day, on every listed weekday.
            self.days.contains(&today)
        }
    }
}

/// Parse an IANA time-zone name (e.g. `"Europe/Vienna"`) into a [`Tz`], falling
/// back to UTC for an empty or unknown name so evaluation never fails.
pub fn parse_tz(name: &str) -> Tz {
    name.trim().parse().unwrap_or(chrono_tz::UTC)
}

/// The schedules that are open right now, evaluated in `tz`. The DNS policy
/// coordinator folds these into the resolved policies on each rebuild.
pub fn active_now(schedules: &[Schedule], tz: Tz) -> Vec<&Schedule> {
    let now = Utc::now().with_timezone(&tz);
    let weekday = now.weekday();
    let minute = now.hour() as u16 * 60 + now.minute() as u16;
    schedules
        .iter()
        .filter(|schedule| schedule.active_at(weekday, minute))
        .collect()
}

/// The sorted ids of the schedules open right now, in `tz`. Cheap to compare tick
/// to tick, so the ticker only rebuilds when the open set actually changes.
pub fn active_ids_now(schedules: &[Schedule], tz: Tz) -> Vec<String> {
    let mut ids: Vec<String> = active_now(schedules, tz)
        .into_iter()
        .map(|schedule| schedule.id.clone())
        .collect();
    ids.sort();
    ids
}

/// Validate and normalise a `"HH:MM"` time into minutes since midnight (0–1439),
/// or `None` if it is not a valid 24-hour time. Accepts a single-digit hour.
pub fn parse_hhmm(value: &str) -> Option<u16> {
    let (hh, mm) = value.trim().split_once(':')?;
    let hours: u16 = hh.trim().parse().ok()?;
    let minutes: u16 = mm.trim().parse().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(hours * 60 + minutes)
}

/// Whether `value` is a valid `"HH:MM"` 24-hour time.
pub fn valid_hhmm(value: &str) -> bool {
    parse_hhmm(value).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sched(days: &[Day], start: &str, end: &str) -> Schedule {
        Schedule {
            id: "s".to_owned(),
            name: "Test".to_owned(),
            enabled: true,
            days: days.to_vec(),
            start: start.to_owned(),
            end: end.to_owned(),
            target: ScheduleTarget {
                scope: TargetScope::Global,
                id: None,
            },
            effect: ScheduleEffect {
                mode: EffectMode::Pause,
                delta: PolicyDelta::default(),
            },
        }
    }

    #[test]
    fn same_day_window_matches_inside_only() {
        let s = sched(&[Day::Mon], "09:00", "17:00");
        assert!(!s.active_at(Weekday::Mon, 8 * 60 + 59));
        assert!(s.active_at(Weekday::Mon, 9 * 60), "start is inclusive");
        assert!(s.active_at(Weekday::Mon, 16 * 60));
        assert!(!s.active_at(Weekday::Mon, 17 * 60), "end is exclusive");
        assert!(!s.active_at(Weekday::Tue, 10 * 60), "wrong weekday");
    }

    #[test]
    fn overnight_window_belongs_to_its_start_day() {
        // Bedtime 22:00–07:00 scheduled for Monday.
        let s = sched(&[Day::Mon], "22:00", "07:00");
        // Monday evening is open.
        assert!(s.active_at(Weekday::Mon, 22 * 60));
        assert!(s.active_at(Weekday::Mon, 23 * 60 + 59));
        // The early Tuesday hours are the tail of Monday's window.
        assert!(s.active_at(Weekday::Tue, 0));
        assert!(s.active_at(Weekday::Tue, 6 * 60 + 59));
        assert!(!s.active_at(Weekday::Tue, 7 * 60), "window has closed");
        // Monday afternoon (before the window) is closed.
        assert!(!s.active_at(Weekday::Mon, 12 * 60));
        // Tuesday evening is NOT open — only Monday is listed.
        assert!(!s.active_at(Weekday::Tue, 23 * 60));
    }

    #[test]
    fn disabled_or_no_days_is_never_active() {
        let mut s = sched(&[Day::Mon], "09:00", "17:00");
        s.enabled = false;
        assert!(!s.active_at(Weekday::Mon, 10 * 60));
        s.enabled = true;
        s.days.clear();
        assert!(!s.active_at(Weekday::Mon, 10 * 60));
    }

    #[test]
    fn equal_times_mean_the_whole_listed_day() {
        let s = sched(&[Day::Sat, Day::Sun], "00:00", "00:00");
        assert!(s.active_at(Weekday::Sat, 0));
        assert!(s.active_at(Weekday::Sun, 23 * 60 + 59));
        assert!(!s.active_at(Weekday::Mon, 12 * 60));
    }

    #[test]
    fn bad_times_are_treated_as_closed() {
        let s = sched(&[Day::Mon], "9", "25:00");
        assert!(!s.active_at(Weekday::Mon, 10 * 60));
    }

    #[test]
    fn hhmm_parsing_bounds() {
        assert_eq!(parse_hhmm("00:00"), Some(0));
        assert_eq!(parse_hhmm("23:59"), Some(23 * 60 + 59));
        assert_eq!(parse_hhmm("7:05"), Some(7 * 60 + 5));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("12:60"), None);
        assert_eq!(parse_hhmm("noon"), None);
    }

    #[test]
    fn day_serialises_lowercase() {
        assert_eq!(serde_json::to_string(&Day::Mon).unwrap(), "\"mon\"");
        assert_eq!(serde_json::from_str::<Day>("\"sun\"").unwrap(), Day::Sun);
    }

    #[test]
    fn target_omits_id_for_global() {
        let t = ScheduleTarget {
            scope: TargetScope::Global,
            id: None,
        };
        assert_eq!(serde_json::to_string(&t).unwrap(), r#"{"scope":"global"}"#);
    }

    #[test]
    fn unknown_timezone_falls_back_to_utc() {
        assert_eq!(parse_tz("Europe/Vienna"), chrono_tz::Europe::Vienna);
        assert_eq!(parse_tz("not-a-zone"), chrono_tz::UTC);
        assert_eq!(parse_tz(""), chrono_tz::UTC);
    }

    #[test]
    fn active_now_filters_by_clock() {
        // A schedule covering the whole week, all day, is always open; an empty
        // one never is. (We avoid asserting on wall-clock specifics.)
        let all = Schedule {
            days: vec![
                Day::Mon,
                Day::Tue,
                Day::Wed,
                Day::Thu,
                Day::Fri,
                Day::Sat,
                Day::Sun,
            ],
            ..sched(&[], "00:00", "00:00")
        };
        let active = active_now(std::slice::from_ref(&all), chrono_tz::UTC);
        assert_eq!(active.len(), 1);
        assert_eq!(
            active_ids_now(std::slice::from_ref(&all), chrono_tz::UTC),
            vec!["s".to_owned()]
        );
    }
}
