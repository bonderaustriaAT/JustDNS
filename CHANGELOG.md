# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial Cargo workspace: `justdns-dns`, `justdns-filter`, `justdns-store`,
  `justdns-web` and the `justdns` binary.
- Web UI scaffold (React/Vite/TypeScript): overview with protection status and
  blocked-query activity, and settings with an expert toggle that reveals the
  technical options.
- Settings: a Network category with an IPv6 toggle, off by default, matching the
  DNS core's handling of AAAA queries.
