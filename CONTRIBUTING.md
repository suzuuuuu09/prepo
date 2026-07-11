# Contributing

Contributions are welcome.

Please keep changes focused, add tests for observable behavior, and run the following commands before submitting a pull request:

```sh
nix develop -c cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

By contributing, you agree that your contribution is licensed under the Apache License 2.0.
