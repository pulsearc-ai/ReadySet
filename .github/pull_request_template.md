<!--
Thanks for the PR. Read CONTRIBUTING.md and AGENTS.md if you haven't.
-->

## Summary

<!-- One paragraph: what does this PR do, and why? -->

## Where it lands

- [ ] Dispatcher core (`ready-set/`)
- [ ] SDK (`ready-set-sdk/`)
- [ ] Rust provider (`ready-set-rust/`)
- [ ] Contracts (`docs/contracts/`)
- [ ] Tests / fixtures only
- [ ] Documentation only

## Checklist

- [ ] `cargo fmt --check` clean
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean
- [ ] `cargo test --workspace --all-features` passes
- [ ] `cargo doc --workspace --no-deps --all-features` clean (with `RUSTDOCFLAGS="-D warnings"`)
- [ ] Public SDK items added or changed have doc comments
- [ ] If this touches a contract, the spec under `docs/contracts/`, the SDK type, the JSON schema, and the tests all moved together
- [ ] `set` mutations record to the change log; `ready` and `go` don't write files
- [ ] No new top-level dependency without rationale in the PR body

## Cross-platform notes

<!-- If this touches paths, processes, or anything OS-specific, note
which platforms you verified locally and which you're relying on CI
for. The matrix runs Linux, macOS, Windows on stable + beta. -->

## Related issues

<!-- `Closes #N` / `Refs #N` -->
