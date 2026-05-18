# ready-set: Vision

> **Status:** Draft vision document. Not a contract. Not a roadmap. The
> source of truth for *what we are building toward and why.* Iterate on this
> document the same way we iterate on a feature: through review and edit,
> not through silent drift.

## The wedge

Every project has a set of foundations — testing, linting, CI, security,
release, deployment, docs — that should be configured, checked, and
exercised. Today that knowledge lives in checklists, golden-path templates,
half-rotten READMEs, post-merge bots, ad-hoc scripts, and ChatGPT prompts.

The same question — *"what does this project need next?"* — is asked by
humans, by autonomous agents, by CI, by onboarding scripts, and by platform
teams. Each computes the answer differently, with different fidelity, at
different times.

**ready-set is the canonical answer to that question.** A typed capability
matrix that's always-fresh, queryable, and consumable by anything that
speaks JSON. A long-running evaluator keeps it current. Capability providers
— some hand-written, some LLM-backed — produce the rows. Humans glance at
it. Agents subscribe to it. CI gates on it. Org dashboards aggregate it.

That's the product.

## What you do with it

You install ready-set in a project. The first thing you see is a matrix:

```
workspace      Ready
toolchain      Ready
formatting     Stale       → ready-set set formatting
linting        Ready
tests          Missing     → ready-set set tests
ci             Missing     → ready-set ai set ci
docs           Optional
release        Not Needed
security       Stale       → ready-set set security
```

You start `ready-set serve`. A daemon watches the repo. The matrix updates
the moment you change a file. Your shell prompt shows `[5/8 ready]`. Your
editor's status bar mirrors it. When your AI assistant is told *"make this
project release-ready,"* it doesn't guess — it queries the matrix and works
the rows.

Some rows resolve via static templates: fast, deterministic, free. Some opt
into AI mode: the provider reads your repo and proposes setup tailored to
*this* codebase, not a generic boilerplate. The output shape is the same
either way; the consumer doesn't care which path produced it.

When you decide to act on a row, you run `ready-set set <capability>` or
`ready-set go <capability>`. Anything that mutates the repo records a
change-log entry. `ready-set undo` always reverses the last action.
**Trust is built on reversibility.**

## Audiences

Three concentric audiences, each more important than they look at first:

| Tier | Audience | What they consume |
|---|---|---|
| 1 | The working developer | The matrix; the lifecycle commands; an editor status bar |
| 2 | The AI coding agent | The matrix as a structured task feed over MCP / JSON |
| 3 | The platform engineer | The matrix as an internal-standards contract; org-level rollups |

The product wins when all three audiences consume the same matrix.

### Tier 1: the working developer

Wants the boring foundations to be right without maintaining a personal
checklist. The headline experience is the matrix. The win condition is *"my
project is configured the way a project of this shape should be."*

### Tier 2: the AI coding agent

Doesn't have eyes; needs a structured task feed. The matrix is purpose-built
for this — typed capabilities, typed states, typed next actions, machine-
stable JSON. Cursor, Claude Code, Codex, and the next ten agents all want
the same data shape and shouldn't each build it from scratch by scraping
the filesystem.

When an agent is told *"set up CI for this repo,"* it should reach for
ready-set the same way it reaches for `git` or `grep` today.

### Tier 3: the platform engineer

Defines what "ready" means for their org. Custom providers encode internal
standards. The matrix becomes the contract between platform teams and the
services they support. Onboarding a new service to org standards becomes
`ready-set set` instead of a 47-step Confluence page.

## The mechanism

Three foundations, each replacing something today's tools fake:

### A typed capability protocol

Every capability — Rust toolchain, GitHub Actions workflow, Kubernetes
deployment, secret scanning — has the same shape on the wire:

```json
{
  "id": "linting",
  "provider": "rust",
  "state": "stale",
  "relevance": "required",
  "summary": "clippy.toml differs from template v0.3",
  "next_action": {
    "command": "ready-set set linting",
    "description": "Reconcile clippy.toml against the template"
  }
}
```

Across every language, every domain, every provider. **This is the part
that has to be right; everything else hangs off it.**

### A long-running evaluator

The daemon means the matrix is free to query. The cost of *"is my project
ready?"* drops from "run a linter, wait" to "ask the daemon, get an answer
in milliseconds."

That cost reduction unlocks ambient UX (status bars, IDE panels) and
high-frequency agent queries that wouldn't be viable on a CLI roundtrip
model. The daemon is what makes ready-set the answer for agents instead of
a tool agents occasionally invoke.

### Pluggable intelligence

A capability provider can be a static template merge or an LLM call that
read the repo. The protocol doesn't know or care. Authors choose per-
capability.

| Mode | Speed | Cost | Determinism | Use for |
|---|---|---|---|---|
| Static | ms | $0 | exact | toolchain, formatting, file presence |
| AI | seconds | API call | bounded variance | security advice, deployment shape, custom lint rules |

Static is the default. AI mode is opt-in per capability per repo, with
explicit data-flow consent. The same `CapabilityReport` comes back either
way; the wire shape never changes.

## The knowledge graph

The matrix is the surface; underneath, ready-set models the project as a
typed graph.

### Why a graph

The flat matrix breaks down at the edges. *`release` requires `ci` requires
`tests`* — a dependency relationship that's tribal knowledge today. Two
providers want to contribute to the same `security` capability without
fighting over ownership. *"Why is `linting` stale?"* should be a structured
explanation, not a free-text summary. AI agents reason better over typed
graphs than over flat lists.

A graph dissolves all of that. The matrix becomes one *projection* of the
graph onto capability nodes; everything else (provenance, dependencies,
composition, profiles) is another projection.

### Nodes

| Node type | Examples |
|---|---|
| `Capability` | `workspace`, `linting`, `tests`, `release`, `security` |
| `Provider` | `rust`, `ci`, `secrets`, `deploy` |
| `Artifact` | `rustfmt.toml`, `.github/workflows/ci.yml`, `Cargo.toml#workspace.lints` |
| `Template` | the canonical content + version a capability expects |
| `Command` | `cargo fmt --check`, `cargo audit` |
| `Profile` | `rust-cli`, `rust-library`, `web-service` |
| `Run` | one execution of `set` or `go` (= a change-log entry) |
| `Evidence` | a fact: *"file X has sha256 Y at time T"* |

### Edges

| Edge | Reads as |
|---|---|
| `provides` | Provider → Capability |
| `requires` | Capability → Capability (`release` requires `ci`) |
| `consumes` | Capability → Artifact (linting consumes `clippy.toml`) |
| `produces` | Run → Artifact (a `set` run produced this file) |
| `templates` | Capability → Template |
| `evidences` | Evidence → Capability state claim |
| `supersedes` | Run → Run (time order) |
| `blocks` | Capability → Capability (transitively `Blocked` when prereq is `Stale` or `Missing`) |
| `instances_of` | Profile → Capability (a profile is a curated capability set) |

### What this unlocks

- **Dependency reasoning is free.** `release` is `Blocked` because `ci` is
  `Missing` because `tests` is `Missing`. The matrix orders work correctly
  without anyone hand-coding precedence. Agents asking *"shortest path to
  all-Ready?"* get a graph traversal answer.

- **Cross-capability invariants are checkable.** `formatting` and `linting`
  both `consume` the workspace lints table. When `linting`'s `set` produces
  a new version of that artifact, `formatting`'s state is invalidated
  automatically. Today this would be a quiet bug; with edges, it's a graph
  walk.

- **Provenance is structural.** Why is `linting` stale? Because there's a
  path: `linting` → `consumes` → `clippy.toml` → `evidences` → *(sha
  mismatch with template version 0.3)*. The "why" stops being a free-text
  field and becomes a traversable explanation.

- **Plugin composition stops being a fight.** `security` can be `provided`
  by three providers in parallel: `ready-set-rust` (cargo audit),
  `ready-set-secrets` (gitleaks), `ready-set-supply-chain` (sigstore). Each
  contributes nodes and edges. There's no "primary provider" arbitration in
  config; the graph is the merge.

- **Templates as subgraph patches.** `set linting` is no longer "merge a
  TOML file." It's "apply this subgraph patch: add these `produces` edges,
  attach this `Run` node, mark these prior `Evidence` nodes superseded."
  Reversibility (`undo`) becomes graph rollback — formal, not hand-coded
  per provider.

- **AI-mode capabilities have real context.** An LLM provider for
  `security` doesn't get *"the project root path."* It gets a graph subset:
  the focal capability, its `requires` chain, the artifacts it consumes,
  recent runs, the evidence nodes attached. That's the difference between
  guessing and reasoning from facts.

## Anti-goals

Sharpness comes from saying no:

- **Not a task runner.** `cargo`, `just`, `make`, `npm scripts` exist.
- **Not a CI system.** GitHub Actions and GitLab CI exist.
- **Not a meta-linter.** precommit, biome, lefthook exist.
- **Not a project generator.** `cargo new`, `create-react-app`, `cookiecutter` exist.
- **Not an AI coding assistant.** Cursor, Claude Code, Codex exist — ready-set *serves* them.
- **Not a code review tool.** ready-set is about foundation state, not changeset opinion.
- **Not opinionated about how you write code.** Only about whether your foundations are configured the way a project of your shape should be.

ready-set is one thing: **the structured, always-fresh answer to "what
does this project need next?"** Every feature must ladder up to making that
answer more useful or more universal.

## Why this moment

- **AI agents are reshaping how code gets written.** They need a structured
  task surface. Today they synthesize one from filesystem inspection (slow,
  brittle) or human prompts (vague). A typed, always-current capability
  matrix is the missing protocol layer between agent and project.

- **LLMs make context-aware capability providers viable.** The historical
  objection to bootstrap tools — *"their opinions don't match mine"* —
  dissolves when the provider can read the repo and adapt.

- **Long-running per-project daemons are normalized.** LSP, MCP, watchman,
  Bun's runtime, Vite. The "always-on background process per repo" pattern
  is mainstream now in a way it wasn't five years ago.

- **Platform engineering is a real discipline with budget.** Orgs want
  internal standards encoded in tooling, not in Confluence pages that
  drift. ready-set gives them a primitive to encode against.

## Principles

These are bets we make once and don't revisit lightly:

1. **One canonical answer.** Humans, agents, CI, IDE — all consume the same
   matrix. We never build separate surfaces with diverging data.

2. **The protocol is the product.** Capability descriptor, report, run-
   report, graph-contribution shapes are a long-term API. Every feature
   ladders up to making the protocol more useful, not bypassing it.

3. **Intelligence is opt-in.** AI-mode capabilities cost money and send
   code over the wire. They're explicitly enabled per capability per repo.
   Default mode is fast, free, deterministic, offline.

4. **Mutations are reversible.** Anything that writes to the repo records
   to the change log. `undo` always works. Trust is built on reversibility.

5. **The core stays generic.** Domain knowledge — Rust, Node, Kubernetes,
   GitHub Actions, security scanners — lives in providers. Core knows about
   capabilities, states, and the graph. Nothing else.

## Steady state

Five years out, if this works:

- `ready-set serve` is a normal background process in any serious project,
  the way LSP servers are normal today.

- `ready-set` (no args) is the first thing developers run when they open an
  unfamiliar repo.

- AI agents reach for ready-set over filesystem inspection. They list it
  among their tools, like they list git or grep today.

- A registry of provider plugins covers most languages and most cross-
  cutting concerns. Some are foundation-blessed; some are community.

- AI-mode capabilities are normal for the long-tail (security, deployment,
  observability) where one-size templates always failed. Static mode
  dominates the basics (toolchain, formatting) where determinism matters.

- Platform teams ship internal providers that encode their organization's
  standards. Onboarding a new service to org standards is `ready-set set`,
  not Confluence.

- The matrix is a unit of measurement. *"X% of our services are Ready"*
  appears in engineering org dashboards alongside DORA metrics.

That's what we're building toward. Everything between here and there is
implementation detail.

## What this document is not

This document does not commit to a sequence of implementation steps. It
does not promise a release date. It does not freeze the protocol or the
graph vocabulary. Those are work products of separate efforts:

- `README.md` carries the current architecture and roadmap.
- `docs/contracts/` carries the binding protocol surface.
- `AGENTS.md` and `CLAUDE.md` carry the guidance for changes.

When this vision drifts away from those documents, the documents are
right and the vision is what needs editing. Vision serves the work, not
the other way around.
