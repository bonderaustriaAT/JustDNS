# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial Cargo workspace: `justdns-dns`, `justdns-filter`, `justdns-store`,
  `justdns-web` and the `justdns` binary.
- Response cache (`justdns-dns`): per-entry TTL expiry via `moka`, record TTLs
  counted down on read, and negative-answer caching.
