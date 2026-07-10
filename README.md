# prepo

Preflight checks for development repositories.

`prepo` is a declarative CLI for checking whether a development repository is
ready to run in the current environment. It never modifies the environment.

```sh
prepo init
prepo validate
prepo check
```

Run active checks only with the corresponding explicit permission flag:
`--allow-command`, `--allow-network`, or `--allow-env`. In particular,
`--allow-command` allows configured commands to inherit the environment and
access the filesystem and network; only use it for trusted configurations.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion is dual-licensed under those same terms.
