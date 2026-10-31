// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Forwarding resolver: upstream selection, caching and DNS message handling.

pub mod cache;
pub mod client;
pub mod pipeline;
pub mod router;
pub mod server;
pub mod upstream;
