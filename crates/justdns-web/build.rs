// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! `rust-embed` requires the asset folder to exist when the crate is compiled.
//!
//! In a release build the real `ui/dist` bundle is in place. For debug builds —
//! `cargo test`, `cargo clippy`, CI — the bundle may be absent, and the assets
//! are read from disk at runtime rather than embedded, so an empty placeholder
//! folder is enough to let the crate compile.

use std::fs;

fn main() {
    if fs::metadata("ui/dist").is_err() {
        let _ = fs::create_dir_all("ui/dist");
    }
}
