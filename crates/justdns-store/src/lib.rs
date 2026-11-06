// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Persistence for the query log: the durable DuckDB store and the in-memory
//! hot view that feeds the dashboard.

mod duck;
mod event;
mod recorder;
mod store;

pub use duck::{DuckStore, StoreError};
pub use event::{QueryEvent, QuerySource};
pub use recorder::{
    ActivityBucket, ClientActivity, ClientCount, DayStats, DomainCount, Recorder, Stats,
};
pub use store::Store;
