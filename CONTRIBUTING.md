# Contributing to Riku

Thanks for your interest in Riku. Contributions of every size are welcome, from a typo fix to a new format module.

## Ways to contribute

- **Report a bug** or a confusing result (see [Reporting bugs](#reporting-bugs)).
- **Try Riku on your own designs** and tell us what it gets wrong or misses: real layouts, schematics and PDKs are the best test data.
- **Improve the documentation** in `docs/`.
- **Add a translation** of the CLI and viewer (see [Translations](docs/dev/development.md#translations)).
- **Fix a bug or implement a feature.** For larger changes, open an issue first so the approach can be discussed. Open work is listed in [docs/dev/roadmap.md](docs/dev/roadmap.md).

## Getting the code

Riku uses Git submodules for its engines, so clone recursively:

```bash
git clone --recurse-submodules https://github.com/riku-chip/riku_chip
cd riku_chip
```

If you already cloned without submodules, or after pulling changes:

```bash
git submodule update --init --recursive
```

Development happens on **Linux x86_64**. You need Rust stable, a C++ toolchain, zlib and qhull, plus a few GUI libraries for the viewer. The full list is in [docs/dev/development.md](docs/dev/development.md#platform-and-prerequisites).

## Building and testing

```bash
cargo build --release                       # target/release/riku
cargo build -p riku --no-default-features   # terminal-only build, no viewer or format modules
cargo test --workspace                      # all tests
cargo test -p riku-mod-layout               # one crate
```

Changes to layout reading, rendering, devices or nets should also be checked with the scripts in `tools/verify/` against KLayout, Magic and Netgen (see [Verification](docs/dev/development.md#verification)).

## Before opening a pull request

- [ ] Code is formatted, per package (never `cargo fmt --all`, which touches the submodules):
  `cargo fmt -p viewer-core -p riku-kernel -p riku-mod-layout -p riku`
- [ ] `cargo clippy --workspace --all-targets --locked` adds no new warnings.
- [ ] `cargo test --workspace` passes, ideally with `RUSTFLAGS="-D warnings"` as in CI.
- [ ] New behavior has tests.
- [ ] Documentation is updated when behavior, commands, flags or output change.
- [ ] CLI text and JSON output stay stable unless the change is intentional. JSON changes follow the schema rules: a new optional field is fine; an incompatible change bumps the schema version.
- [ ] New user-visible strings use `tr!` and are added to every file in `riku/locales/`.
- [ ] If the layout diff output changes, the `riku-mod-layout` version is bumped (it is part of the cache key).

Please read the [project rules](docs/dev/architecture.md#project-rules) before changing contracts between crates. In particular, `external/xschem-viewer-rust` is maintained upstream as a separate crate: changes to it go upstream, not into this repository.

## Commit style

Use [Conventional Commits](https://www.conventionalcommits.org/): `type(scope): description`, in the imperative and lower case.

| Type | Use for |
|---|---|
| `feat` | A new feature |
| `fix` | A bug fix |
| `perf` | A performance improvement |
| `refactor` | A code change that neither fixes a bug nor adds a feature |
| `docs` | Documentation only |
| `test` | Adding or fixing tests |
| `style` | Formatting only |
| `build`, `ci` | Build system, dependencies, workflows |
| `chore` | Anything else (release bumps, housekeeping) |

Examples: `fix(layout): keep labels of empty cells`, `perf(diff): skip identical layers before XOR`, `docs(lvs): explain --update`.

## Where the docs live

| Path | Content |
|---|---|
| `README.md` | Overview and quick start |
| `docs/` | User guides: getting started, CLI, scripting, configuration, viewer, formats, LVS |
| `docs/dev/` | Contributor docs: [architecture](docs/dev/architecture.md), [development](docs/dev/development.md), [design notes](docs/dev/design-notes.md), [roadmap](docs/dev/roadmap.md) |
| `CHANGELOG.md` | Release history |

## Reporting bugs

Open an issue on [GitHub](https://github.com/riku-chip/riku_chip/issues) and include:

- The output of `riku --version` and `riku doctor` (run from your project).
- The exact command you ran and its full output.
- What you expected instead.
- If possible, a small repository or files that reproduce the problem. Public PDK cells and the bundled demos (`riku demo`) make good reproductions.

For crashes, `RUST_BACKTRACE=1` adds a backtrace to the output.

## License

Riku is licensed under the [Apache License 2.0](LICENSE). By contributing, you agree that your contributions are licensed under the same terms. The bundled engines keep their own licenses: `xschem-viewer-rust` is Apache-2.0 and `gdstk_rust` keeps gdstk's Boost Software License 1.0.
