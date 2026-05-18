# CLAUDE.md

Instructions for Claude (and any agent) working in this repo. Read this
before changing code or design.

---

## What this repo is

`ready-set` is a product-readiness CLI: a control surface that shows
what a project has, what is missing, what is blocked, and what the next
concrete action is. The brand is *ready, set, go* — preparation before
launch — and those three words are the product's verbs.

```text
ready-set ready [capability]   # read-only diagnosis
ready-set set   [capability]   # create or reconcile
ready-set go    [capability]   # execute the capability's main workflow
```

Capabilities (e.g. `workspace`, `toolchain`, `formatting`, `linting`,
`tests`, `ci`, `release`) are first-class objects contributed by
provider plugins. The central UX is the readiness matrix.

The repo is pre-v0.1.0. The capability lifecycle is implemented and
binding; the remaining work is documentation polish, the `undo`
built-in, and the v0.1.0 acceptance gate.

---

## Source-of-truth docs

Read these before making non-trivial changes. They are layered by how
binding they are.

| Doc               | Status                        | What it is                                                                                |
| ----------------- | ----------------------------- | ----------------------------------------------------------------------------------------- |
| `README.md`       | **Authoritative**             | Architecture, principles, plugin authoring, roadmap to v0.1.0, open questions. The spec.  |
| `docs/contracts/` | **Authoritative + versioned** | Protocol specs (env vars, capabilities, manifest, describe, change log…). Stable surface. |
| `AGENTS.md`       | **Authoritative**             | Working guidance for coding agents — non-negotiable rules and lifecycle dispatch contract. |

---

## Current state

| Area                                           | Status                                                                  |
| ---------------------------------------------- | ----------------------------------------------------------------------- |
| Dispatcher (`main.rs`, `cli.rs`, `discovery.rs`, `exec.rs`) | Implemented |
| Lifecycle built-ins: `ready`, `set`, `go`      | Implemented (`ready-set/src/builtins/{ready,set,go}.rs`) |
| Bare `ready-set` → matrix                      | Implemented (`ready-set/src/lib.rs:48`) |
| Meta built-ins: `help`, `list`, `version`      | Implemented |
| Built-in: `undo`                               | **Not yet implemented** — last milestone before v0.1.0 |
| Capability contract + JSON schemas             | Implemented (`docs/contracts/capabilities.md`) |
| Capability registry + matrix                   | Implemented (`ready-set/src/capabilities.rs`, `ready-set/src/lifecycle.rs`) |
| `.ready-set.toml` schema v2                    | Implemented; v1 rejected |
| SDK capability types + lifecycle helpers       | Implemented (`ready-set-sdk/src/capability.rs`, `lifecycle.rs`) |
| First-party `ready-set-rust` provider          | Implemented — `workspace`, `toolchain`, `formatting`, `linting` |
| Provider lifecycle protocol (`__ready`/`__set`/`__go`) | Implemented + tested |
| `--help` / `--list` text reflecting lifecycle  | Implemented |

If you find code that contradicts what is now in `README.md`, the
implementation has likely landed and a doc has not caught up. Verify
against the actual source under `ready-set/src/`, `ready-set-sdk/src/`,
and `ready-set-rust/src/` before "correcting" the code.

---

## Non-negotiable principles

These come from `README.md` and are binding for any change.

1. **Small core + provider plugins.** New built-ins must clear the
   Built-in vs. plugin bar (see `README.md`): lifecycle grammar
   (`ready`/`set`/`go`), dispatcher meta-command (`help`/`list`/
   `version`/`completions`), bootstrap-of-the-bootstrapper, or
   ecosystem contract across plugins (`undo`). When in doubt, plugin.
2. **Capabilities are first-class.** Domain work belongs in providers,
   not in core. Core owns the grammar, the registry, the matrix, and
   lifecycle dispatch. Adding capability-specific behavior to core is
   wrong — extend or write a provider plugin instead.
3. **Stable contracts.** Any change to `READY_SET_*` env vars,
   manifest schema, `__describe` JSON, capability descriptor /
   report / run-report shapes, the `__ready`/`__set`/`__go`
   protocol, change log JSONL, exit code semantics, or
   `.ready-set.toml` v2 schema is semver-breaking for the core.
   Pre-v0.1.0: lock contracts carefully; harder to change later.
4. **Cross-platform from PR #1.** Linux, macOS, Windows. CI matrix on
   every push and merge request. `std::path::Path`, the `directories`
   crate, the `which` crate. No `sh -c`, no hardcoded path
   separators.
5. **Reversibility.** Anything that mutates the filesystem writes to
   `.ready-set/changes/<provider>-<ts>.jsonl` and stores backups in
   `.ready-set/backups/<sha256>` so `undo` can reverse it. `ready`
   and `go` do not write files; only `set` does.
6. **Composability over completeness.** Each capability useful in
   isolation, scriptable, supports `--json`.
7. **Opinionated defaults, escape hatches.** Bare `ready-set` shows
   the matrix; `set` reconciles required capabilities with defaults.
   Advanced behavior via flags or `.ready-set.toml`.
8. **No telemetry.** Period.

---

## Guidelines for change

### Before adding a built-in

Argue it past the Built-in vs. plugin bar in `README.md`:

- Is it part of the lifecycle grammar? (`ready`, `set`, `go` —
  already shipped.)
- Is it a dispatcher meta-command? (`help`, `list`, `version`,
  future `completions`.)
- Is it bootstrap-of-the-bootstrapper?
- Does it implement an ecosystem contract that must work across
  plugins? (`undo` qualifies.)

If none of these fit, build it as a provider plugin contributing
capabilities.

### Before adding a capability

- Capabilities are owned by provider plugins, not by core. Adding a
  new capability means adding it to an existing provider's manifest
  + handlers, or writing a new `ready-set-<provider>` crate.
- The capability id must match the contract: lowercase kebab-case,
  `^[a-z][a-z0-9-]*$`. Check existing ids in the registry first to
  avoid collisions.
- The descriptor must declare exactly the verbs the provider
  supports. `__ready` is required; `__set` and `__go` are optional
  per capability.
- `__ready` is read-only. `__set` mutations must go through the
  change log. `__go` runs the workflow and must not bootstrap
  missing files.

### Before adding scope to `go`

- `go` is the lifecycle execution verb. It dispatches to provider
  `__go` handlers. It does not bootstrap, it does not configure, and
  it is not a generic task runner.
- Adding a `go` workflow to a capability means implementing it in
  the relevant provider plugin, not in core. Core's `go` only
  selects capabilities and aggregates results.
- If a request would let `go` run arbitrary user commands, it is
  wrong. The line that distinguishes `go` from `make`/`just` is
  that every workflow is tied to a declared capability.

### Before adding or changing a contract

- New `READY_SET_*` env var, manifest field, capability descriptor /
  report / run-report field, change log field, exit code, or
  `.ready-set.toml` field requires a versioned spec under
  `docs/contracts/` with worked examples.
- Schema changes need a `schema_version` bump.
- Pre-v0.1.0: lock contracts carefully now; they are harder to
  change later.

### Before building anything aspirational

The capability lifecycle is implemented. The open questions in
`README.md` are not buildable until specified:

1. Capability dependency model (which capabilities block which).
2. Product-profile detection (library / web service / CLI / …).
3. "Next safe action" ranking when multiple capabilities are
   actionable.
4. Irreversibility model for `go` actions (what is safe to run vs.
   what requires confirmation).
5. Discovery of expected-but-missing capabilities for a profile.

Don't start implementation work that depends on unspecified surface.
Surface the question to the user.

### Code style

- No comments unless they explain a non-obvious WHY (a hidden
  constraint, subtle invariant, workaround for a specific bug). No
  WHAT comments — names already do that.
- Concrete code first. No premature abstractions, no
  designed-for-future-needs.
- No backwards-compatibility shims. Pre-v0.1.0, just change the
  code.
- Use SDK helpers (`Output`, `ExitCode`, `ChangeLog`, `Context`,
  `lifecycle::dispatch_*`, capability types). Don't roll your own.
- Every public SDK item has a doc comment (v0.1.0 acceptance gate).

---

## Known traps

- **Adding capability logic to core.** The dispatcher does not know
  about `formatting` or `tests` directly; it asks providers. If you
  catch yourself writing `if capability == "formatting"` in
  `ready-set/src/`, you are in the wrong crate.
- **Drifting `go` toward a task runner.** `go` runs the workflow
  declared by a capability. If a change to `go` would let it execute
  arbitrary user commands, it's wrong.
- **Treating the open questions as committed.** The lifecycle grammar
  and capability model are in `README.md` and binding. The open
  questions section (profile detection, capability dependencies,
  next-action ranking, …) is still aspirational. Don't build the
  second category as if it were specified.
- **Adding a feature flag instead of making a decision.** Pre-v0.1.0,
  just change the code.
- **Making the dispatcher inspect plugin internals.** The contract
  is the CLI surface (args, exit codes, env, stdout — `__describe`,
  `__ready`, `__set`, `__go`). The dispatcher never links to
  plugins, never loads them dynamically.
- **Reserving crate names on crates.io speculatively.** Publish only
  when the plugin actually exists.
- **Updating `README.md` silently.** It is the spec. Changes are
  worth their own commit and review.

---

## When in doubt

- Read the relevant `README.md` section before deciding.
- Surface trade-offs to the user; don't silently pick a direction.
- If a request would expand the core or the lifecycle surface
  beyond `README.md`, ask whether to update the spec first rather
  than building it ad-hoc.
- Cite source: `README.md:<line>` or `docs/contracts/<file>.md` so
  the user can verify what you're invoking.

---

## What "done" means

For any change, before declaring done:

- Code compiles, `cargo fmt` is clean, `cargo clippy` is clean,
  tests pass on the local platform.
- New public SDK items have doc comments.
- New contracts have a spec under `docs/contracts/` with a schema
  where applicable.
- Filesystem mutations are recorded to the change log.
- If the change touches cross-platform behavior, note which
  platforms were verified and which were not.
