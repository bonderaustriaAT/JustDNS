// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Forwarding resolver: upstream selection, caching and DNS message handling.

pub mod blockpage;
pub mod cache;
pub mod cert_store;
pub mod client;
pub mod doh;
pub mod observer;
pub mod pipeline;
pub mod router;
pub mod safesearch;
pub mod server;
pub mod tls;
pub mod upstream;
