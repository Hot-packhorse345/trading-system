# Contributing

Thanks for your interest in improving this project. This is a personal
algorithmic trading system, so contributions are welcome but will be
reviewed with a bias toward correctness and safety over speed of merging —
bugs here can cost real money.

## Before you start

- **Open an issue first** for anything beyond a small fix (new strategy,
  new broker adapter, new indicator, architectural change). This avoids
  wasted work if the direction doesn't fit the project.
- For bug reports, include: OS, Rust version (`rustc --version`), the
  command you ran, the config file (with any keys redacted), and the
  full error/log output.
- Never include real API keys, account numbers, or `.env` contents in an
  issue, PR description, commit, or log snippet. Redact them first.

## Development setup

```bash
git clone https://github.com/0xbarss/trading-system.git
cd trading-system
cp .env.example .env   # fill in with paper/testnet credentials only
cargo build
cargo test
```

Use `BINANCE_TESTNET=true` and paper/demo broker configs for all local
development. There is no need to touch a live account to contribute.

## Workflow

```bash
# 1. Create a feature branch
git checkout -b feature/short-description

# 2. Make your changes

# 3. Run the full check suite before committing
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test

# 4. Commit with a conventional, descriptive message
git commit -m "feat: add donchian channel indicator"

# 5. Push and open a PR against main
git push origin feature/short-description
```

### Commit message style

Prefix commits with a type where it's clear-cut:

- `feat:` — new functionality (indicator, strategy, broker adapter, CLI command)
- `fix:` — bug fix
- `refactor:` — internal change with no behavior change
- `test:` — test-only changes
- `docs:` — documentation only
- `perf:` — performance improvement with no behavior change

## Code standards

- **Formatting:** `cargo fmt` must be run before committing; CI (or reviewers)
  will reject unformatted code.
- **Linting:** `cargo clippy --all-targets --all-features` must pass without
  warnings. Don't silence a lint with `#[allow(...)]` without a comment
  explaining why.
- **Docs:** all public functions, structs, and traits need doc comments
  (`///`) explaining what they do and any non-obvious invariants (e.g.
  whether an indicator repaints, whether a function assumes sorted bars).
- **Tests:** new indicators, strategies, and risk/stop logic need unit
  tests. Bug fixes should include a regression test that fails before the
  fix and passes after.
- **No panics in the live path:** code reachable from `live` should return
  `Result` and handle errors explicitly rather than `.unwrap()`/`.expect()`
  on anything that depends on external input (network responses, broker
  data, config values). `unwrap()` is fine in tests and one-off `tools`
  scripts, not in `live`/`broker`/`risk`.
- **Backward compatibility:** avoid breaking existing config file formats
  where possible. If a breaking config change is unavoidable, call it out
  clearly in the PR description and update the relevant docs/examples.

## Adding a new indicator or strategy

Follow the patterns already documented in the README's
[Development](README.md#-development) section (`crates/indicators` /
`crates/strategy`, registry registration, config wiring). Keep the PR
scoped to the new indicator/strategy plus its tests — avoid bundling
unrelated refactors.

## Adding a new broker adapter

Broker adapters are one of the higher-risk areas of this codebase, since
bugs here can lead to unintended orders or missed risk controls. For a new
adapter PR, please include:

- Coverage for both paper/testnet and (if applicable) live order paths
- Explicit handling of connection loss / reconnect behavior
- Tests around order sizing, stop placement, and error responses from the
  broker API (not just the happy path)

## Reporting security issues

If you find something that could lead to unintended trades, leaked
credentials, or a way to bypass risk/drawdown limits, please **do not**
open a public issue. Instead, reach out privately to the maintainer
([@0xbarss](https://github.com/0xbarss)) so it can be fixed before being
disclosed publicly.

## Questions

Open a GitHub issue with the `question` label, or start a discussion if
the repo has GitHub Discussions enabled.
