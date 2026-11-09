# Contributing

## License and sign-off

Contributions are accepted under the [AGPL-3.0-or-later](LICENSE) license.
The project uses the [Developer Certificate of Origin](https://developercertificate.org/):
sign off every commit with

    git commit -s

which appends a `Signed-off-by` trailer confirming you may submit the work
under the project license.

## Development

Requires a Rust toolchain (edition 2024, rustc >= 1.85) with `rustfmt` and
`clippy`, plus pnpm for the web UI.

Before opening a pull request:

    cargo fmt --all
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --all

Keep commits small and focused. Write tests first for the blocklist parser,
the policy resolver and the cache logic.
