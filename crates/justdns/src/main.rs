// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The JustDNS binary: read the environment, build the resolver and serve.

mod config;

use std::process::ExitCode;

use justdns_dns::cache::DnsCache;
use justdns_dns::pipeline::{Pipeline, PipelineOptions};
use justdns_dns::router::Router;
use justdns_dns::server::Server;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::config::Config;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            error!(%error, "invalid configuration");
            return ExitCode::FAILURE;
        }
    };

    info!(
        listen = %config.listen,
        ipv6 = config.ipv6_enabled,
        upstreams = config.upstreams.upstreams.len(),
        "starting JustDNS"
    );

    let router = Router::from_config(&config.upstreams);
    let cache = DnsCache::new(config.cache_capacity);
    let pipeline = Pipeline::new(
        cache,
        router,
        PipelineOptions {
            ipv6_enabled: config.ipv6_enabled,
        },
    );
    let server = Server::new(pipeline);

    tokio::select! {
        result = server.run(config.listen) => {
            if let Err(error) = result {
                error!(%error, "server stopped");
                return ExitCode::FAILURE;
            }
        }
        _ = shutdown() => {
            info!("shutting down");
        }
    }

    ExitCode::SUCCESS
}

/// Resolve when the process is asked to stop: SIGTERM from `docker stop`, or
/// Ctrl-C from an interactive run.
async fn shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut term = match signal(SignalKind::terminate()) {
            Ok(term) => term,
            Err(error) => {
                error!(%error, "cannot listen for SIGTERM");
                return;
            }
        };
        tokio::select! {
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
