# Contributing to ReadySet

Thanks for your interest in contributing to ReadySet. This document is
the practical guide: how to set up, what to run, and what we expect from
a PR.

The product spec lives in [`README.md`](README.md). The contracts that
bind every plugin live under [`docs/contracts/`](docs/contracts/). The
non-negotiable rules for code changes live in [`AGENTS.md`](AGENTS.md).
**Read those before opening a non-trivial PR.**

## Ground rules

- The repo is pre-`0.1.0`. Contracts are locked carefully; everything
  else is iterating.
- Domain knowledge belongs in provider plugins, not the dispatcher core.
  See the Built-in vs. plugin bar in `README.md`.
- We don't add features speculatively. If you're not sure whether
  something fits, open an issue first.

## Development setup

Requirements:

- Rust toolchain (`rustup`), edition 2024.
- The pinned toolchain in `rust-toolchain.toml` is selected automatically.

Build, test, and lint the workspace:

```sh
cargo build --workspace
cargo test --workspace --all-features
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```

The CI matrix runs the same commands on Linux, macOS, and Windows on
both `stable` and `beta`. Code that passes locally on one OS still has
to pass on the other two — especially path handling.

## What "done" means

Before opening or updating a PR:

- [ ] `cargo fmt --check` is clean.
- [ ] `cargo clippy -D warnings` is clean.
- [ ] `cargo test --workspace` passes locally.
- [ ] `cargo doc -D warnings` is clean.
- [ ] If the change touches a contract under `docs/contracts/`, the
      spec, the SDK type, the JSON schema, and the tests all move
      together.
- [ ] `set` mutations record to the change log; `ready` and `go` do not
      write files.
- [ ] No new top-level dependencies without a clear reason — `cargo deny`
      runs on CI.
- [ ] Public SDK items have a doc comment.

## Pull request workflow

1. Fork and branch from `main`.
2. Make a focused change. One change per PR; bundle reasoning in the PR
   description.
3. Keep the diff small. Big mechanical sweeps go in their own PR with
   "no behavior change" in the title.
4. Reference the issue you're closing (`Closes #N`) when applicable.
5. Wait for CI green before requesting review.
6. Be ready to rebase on `main` — we keep history linear.

## Commit style

- Imperative present tense ("Add X", "Fix Y", not "Added X" or "Fixes Y").
- One subject line under ~72 characters.
- A body paragraph if the *why* isn't obvious from the diff.
- Don't append "(closes #N)" to the subject — keep that in the PR body.

## Reporting issues

Use the issue templates. For bugs, include:

- ReadySet version (`ready-set --version`).
- Operating system + architecture.
- The exact command and the output you got.
- What you expected instead.

For feature requests, lead with the *problem*, not the solution. We want
to understand the use case before discussing implementation.

For security issues, **do not open a public issue.** See
[`SECURITY.md`](SECURITY.md).

## License

By contributing, you agree that your contributions will be dual-licensed
under [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE) at the user's
option, consistent with the rest of the workspace.

## Code of Conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md).
Participation requires adherence.
