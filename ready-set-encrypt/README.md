# ready-set-encrypt

First-party `ready-set` provider plugin for secrets management.

Rotation spawns user-defined secret manager or deployment CLIs such as `fly`,
`neon`, `vercel`, `railway`, `render`, `netlify`, `heroku`, `wrangler`,
`gcloud`, `aws`, `az`, `doctl`, `firebase`, `supabase`, `doppler`,
`infisical`, `op`, `vault`, `gh`, or `kubectl`, and wraps each spawn in the
host's sandbox to bound its filesystem write blast radius: `sandbox-exec` on
macOS, `bubblewrap` (`bwrap`) on Linux, and an `AppContainer` launcher on
Windows. The runtime requirement: `sandbox-exec` ships with macOS; install
`bwrap` on Linux via `apt install bubblewrap` (Debian/Ubuntu) or `dnf install
bubblewrap` (Fedora); install `ready-set-encrypt-launcher.exe` alongside
`ready-set-encrypt.exe` on Windows.

## Rust API Boundary

The Rust library API is intentionally narrow. Provider metadata, provider
dispatch helpers, and the `.rsb` bundle-format primitives are public; scanner,
rotation, sandbox, webhook, scaffold, and workflow internals are not a stable
Rust API. Use the documented CLI/provider protocol and config files for those.

## Capabilities

| Capability | Verbs | Purpose |
|------------|-------|---------|
| `secrets` | `ready`, `set`, `go` | Inventory env vars, scaffold `.env.example` + canonical fake template + `.gitleaks.toml` + rotation manifest, run a leak scan. |
| `rotation` | `ready`, `go` | Track per-secret rotation cadence via a manifest + append-only audit log; rotate `self-issued` / `exec` secrets (sandboxed) and remind on `manual` ones. |
| `secret-bundles` | `ready`, `set`, `go` | Encrypt configured dotenv files into ReadySet `.rsb` bundles and verify they decrypt. |

## `secrets` capability

| Verb | Behavior |
|------|----------|
| `ready` | Inventories environment variables across `.env`, `.env.example`, Rust (`env::var(...)`, `env!(...)`, `option_env!(...)`), and TS/JS (`process.env.X`, `import.meta.env.X`). Compares the sets and classifies the project. |
| `set` | Scaffolds `.env.example`, writes `deploy/secrets/canonical.env.template` with fake values, appends a `# >>> ready-set-encrypt managed >>>` block to `.gitignore`, writes a default `.gitleaks.toml`, and seeds the rotation manifest. Mutations recorded under `.ready-set/changes/` so the planned `ready-set undo` can reverse them. |
| `go` | Runs a leak scan. Uses `gitleaks detect --no-banner --redact` when `gitleaks` is on `PATH`; otherwise falls back to a built-in regex scan. Findings are reported as paths + line numbers — never bytes. |

### `secrets` matrix states

| State | Meaning |
|-------|---------|
| `not-needed` | No env vars detected anywhere — nothing to manage. |
| `missing` | Code references env vars but `.env.example` is absent. |
| `incomplete` | Vars referenced in code are missing from `.env.example`. |
| `stale` | `.env.example` declares vars not referenced in code — pass `--force` to prune. |
| `ready` | `.env.example` matches the detected set. |

## `rotation` capability

| Verb | Behavior |
|------|----------|
| `ready` | Walks the manifest + audit log, classifies each secret against its `cadence_days`. |
| `go` | Default **dry-run**: lists what would rotate. With `ready-set rotate --confirm`: dispatches each overdue secret to its backend and appends an audit entry. Use `--name SECRET` to target a single manifest secret. |

### `rotation` matrix states

| State | Meaning |
|-------|---------|
| `not-needed` | No env vars detected. |
| `blocked` | Env vars exist but rotation manifest is missing — `next_action: ready-set set secrets`. |
| `incomplete` | Manifest declares secrets not in detected inventory (drift). |
| `stale` | ≥1 secret overdue or never rotated — `next_action: ready-set rotate --confirm`. |
| `ready` | All manifest secrets within rotation cadence. |

## `secret-bundles` capability

`secret-bundles` is ReadySet's native encrypted file path. The
`ready-set-encrypt` crate owns the bundle container format and the encryption
implementation. New bundles use AES-256-GCM envelope encryption with a local
wrapping key under ignored `secrets/` by default; legacy XChaCha20-Poly1305
bundles remain decryptable for migration.

Project config lives in `.ready-set/plugins/secrets/config.toml`:

```toml
[bundles]
enabled = true
key_file = "secrets/readyset-bundle.key"

[[bundles.files]]
source = ".env"
encrypted = "deploy/secrets/root.env.rsb"
payload = "dotenv"
environment = "local"
```

| Verb | Behavior |
|------|----------|
| `ready` | Checks the local key, bundle presence, decryptability, and dotenv key parity against the plaintext source when present. |
| `set` | Creates the local key if needed and encrypts every configured source file. |
| `go` | Verifies every configured bundle decrypts and reports non-secret metadata only. |

### Manifest format

`.ready-set/plugins/secrets/manifest.toml`:

```toml
[ready-set-encrypt]
schema_version = 1
default_cadence_days = 90

# Local random bytes + deployment hook:
[secret.SESSION_SECRET]
backend = "self-issued"
cadence_days = 30
target_path = "secrets/session-secret"
deploy_commands = [
  ["secretctl", "set", "SESSION_SECRET", "{{value}}", "--service", "example-api"],
]
notes = "Invalidates active sessions; rotate during low-traffic windows"

# Value from an external command + deployment hook:
[secret.DATABASE_URL]
backend = "exec"
generate_command = ["dbctl", "connection-string", "production"]
deploy_commands = [
  ["secretctl", "set", "DATABASE_URL", "{{value}}", "--service", "example-api"],
]
sandbox_write_paths = ["~/.config/dbctl"]  # dbctl keeps auth state outside the project

# Manual reminder only:
[secret.OPENAI_API_KEY]
backend = "manual"
dashboard_url = "https://platform.openai.com/api-keys"
```

Illustrative command shapes:

| Provider style | Example argv |
|----------------|--------------|
| Fly.io | `["fly", "secrets", "set", "SESSION_SECRET={{value}}", "-a", "example-api"]` |
| Neon | `["neon", "connection-string", "main", "--branch", "production"]` |
| Vercel | `["scripts/rotate-vercel-env", "SESSION_SECRET", "{{value_path}}", "production"]` |
| Railway | `["railway", "variables", "--set", "SESSION_SECRET={{value}}"]` |
| Render | `["render", "services", "env", "set", "srv-example", "SESSION_SECRET={{value}}"]` |
| Netlify | `["netlify", "env:set", "SESSION_SECRET", "{{value}}", "--context", "production"]` |
| Heroku | `["heroku", "config:set", "SESSION_SECRET={{value}}", "--app", "example-api"]` |
| Cloudflare Workers | `["scripts/rotate-wrangler-secret", "SESSION_SECRET", "{{value_path}}"]` |
| Supabase | `["supabase", "secrets", "set", "SESSION_SECRET={{value}}", "--project-ref", "example"]` |
| Firebase | `["scripts/rotate-firebase-secret", "SESSION_SECRET", "{{value_path}}"]` |
| DigitalOcean Apps | `["scripts/rotate-doctl-secret", "SESSION_SECRET", "{{value_path}}"]` |
| AWS Secrets Manager | `["aws", "secretsmanager", "put-secret-value", "--secret-id", "example/session", "--secret-string", "{{value}}"]` |
| GCP Secret Manager | `["gcloud", "secrets", "versions", "add", "SESSION_SECRET", "--data-file", "{{value_path}}"]` |
| Azure Key Vault | `["az", "keyvault", "secret", "set", "--vault-name", "example-vault", "--name", "SESSION_SECRET", "--value", "{{value}}"]` |
| Kubernetes | `["scripts/rotate-kubernetes-secret", "app-secrets", "SESSION_SECRET", "{{value_path}}"]` |
| Doppler | `["doppler", "secrets", "set", "SESSION_SECRET={{value}}", "--project", "example", "--config", "prod"]` |
| Infisical | `["infisical", "secrets", "set", "SESSION_SECRET={{value}}", "--env", "prod", "--path", "/"]` |
| 1Password | `["op", "item", "edit", "Example App", "SESSION_SECRET[password]={{value}}"]` |
| Vault | `["vault", "kv", "put", "secret/example", "SESSION_SECRET={{value}}"]` |
| GitHub Actions | `["gh", "secret", "set", "SESSION_SECRET", "--body", "{{value}}", "--repo", "owner/repo"]` |

These are examples, not bundled adapters. `ready-set-encrypt` only executes the
argv arrays you put in the manifest. When a provider CLI requires stdin,
multiple commands, or interactive setup, put that logic in a project-local
script and call the script from `deploy_commands`.

Per-secret fields:

| Field | Required | Used by | Meaning |
|-------|----------|---------|---------|
| `backend` | yes | all | `"self-issued"`, `"manual"`, or `"exec"` |
| `cadence_days` | no | all | Override `default_cadence_days` for this secret |
| `rotate` | no | all | Defaults to `true`. Set `false` for non-secret config values that should stay in the inventory manifest but not count toward rotation cadence. |
| `target_path` | no | self-issued, exec | Project-relative path the new value is written to (0600 on Unix). Without it, the value goes to stdout (self-issued without deploys) or is pushed to deploy_commands only (no local file) |
| `dashboard_url` | no | manual | URL shown in the reminder |
| `notes` | no | all | Free-form human note (rotation caveats, runbook link) |
| `generate_command` | **yes** for `exec` | exec | argv array; stdout (trimmed) becomes the new value |
| `deploy_commands` | no | self-issued, exec | Sequential argv arrays run after the value is in hand. Fail-fast: first non-zero exit halts the rest. Supports `{{value}}` and `{{value_path}}` substitution inside elements |
| `sandbox_write_paths` | no | all | Extra `(subpath ...)` entries added to the sandbox write allowlist. `~` expanded. Use for tools with state dirs outside the project (e.g. `~/.fly`, `~/.config/neon`, `~/.config/gcloud`, `~/.config/op`, `~/.kube`) |
| `unsandboxed` | no | all | When `true`, skip the platform sandbox wrap. Reserved for genuinely problematic tools; recorded as `sandboxed: false` in the audit log |

Recommended: keep `target_path` values under `secrets/` so the existing managed `.gitignore` block covers them. Substitution happens inside argv *elements only* — never through a shell — so `{{value}}` substitution cannot inject shell metacharacters.

Common state-dir examples for `sandbox_write_paths`:

| CLI family | Example paths |
|------------|---------------|
| Fly.io / Neon / Vercel / Railway | `~/.fly`, `~/.config/neon`, `~/.config/com.vercel.cli`, `~/.railway`, `~/.config/railway` |
| Netlify / Heroku / Render | `~/.config/netlify`, `~/.netrc`, `~/.cache/heroku`, `~/.config/render` |
| AWS / GCP / Azure / DigitalOcean | `~/.aws`, `~/.config/gcloud`, `~/.azure`, `~/.config/doctl` |
| Cloudflare / Firebase / Supabase | `~/.wrangler`, `~/.config/.wrangler`, `~/.config/configstore`, `~/.supabase` |
| Kubernetes / Helm | `~/.kube`, `~/.config/helm` |
| 1Password / Vault / Doppler / Infisical | `~/.config/op`, `~/.vault-token`, `~/.doppler`, `~/.infisical` |

Only add paths your chosen CLI actually needs.

### Audit log format

`.ready-set/plugins/secrets/rotations.jsonl` (append-only, gitignored, **not** managed by the SDK change log):

```json
{"name":"SESSION_SECRET","backend":"self-issued","ts":"2026-05-23T10:15:00Z","outcome":"rotated","value_sha256":"<hex>","target_path":"secrets/session-secret"}
{"name":"OPENAI_API_KEY","backend":"manual","ts":"2026-05-23T10:15:00Z","outcome":"reminded"}
```

`outcome` is one of `rotated | reminded | failed`. The raw secret value never appears in the log; only `value_sha256` is recorded, and only for `rotated` outcomes from `self-issued` / `exec`. Rotation entries also include `deploy_count`, `sandboxed`, and `platform_sandbox` when commands are spawned.

### Backends

| Backend | Behavior |
|---------|----------|
| `self-issued` | Generates 32 random bytes via `getrandom`, hex-encodes (64 chars). If `target_path` set, writes via `atomic_write` + `restrict_to_user(0600)`. If `deploy_commands` set, runs them (sandboxed) after the local write. If neither set, prints the value to stdout once with the SHA. |
| `exec` | Runs `generate_command` (sandboxed); its trimmed stdout becomes the new value. Then writes `target_path` (if set) and runs `deploy_commands` (sandboxed, sequential fail-fast). |
| `manual` | Prints reminder + `dashboard_url` (if present). No filesystem mutation, no commands run. Used for provider keys without a rotation API (OpenAI, Anthropic, Stripe, SendGrid, Resend, etc.). |

### Sandbox model

Every spawned command from `exec` and from `self-issued` `deploy_commands` is
wrapped by the platform backend unless the manifest sets `unsandboxed = true`.
The audit log records whether a command was sandboxed and which backend was
used.

On macOS, commands are wrapped in `sandbox-exec` with a generated profile:

```scheme
(version 1)
(deny default)
(allow process-fork process-exec mach-lookup sysctl-read)
(allow file-read*)
(allow network*)
(allow file-write*
  (subpath "<PROJECT_ROOT>")
  (subpath "<TMPDIR>")
  (subpath "<HOME>/Library/Caches")
  ;; plus sandbox_write_paths from the manifest
  )
(deny file-write*
  (subpath "<HOME>/.ssh")
  (subpath "<HOME>/.gnupg")
  (literal "<HOME>/.zshrc")
  (literal "<HOME>/.bashrc")
  ;; etc.
  )
```

**Threat model:**

| Threat | Mitigated? |
|---|---|
| Malicious provider CLI overwrites `~/.ssh/authorized_keys` | Yes — denylist + write allowlist |
| Buggy CLI accidentally writes to `~/.bashrc` | Yes — same |
| Manifest contains `["rm", "-rf", "<HOME>"]` | Yes — only allowlisted paths are writeable |
| Provider CLI exfiltrates `{{value_path}}` over HTTPS | **No** — network is allowed |
| Provider CLI reads `~/.aws/credentials`, `~/.kube/config`, or unrelated tool credentials and uploads them | **No** — file-read is universally allowed |

Read-side exfiltration is out of scope (would require egress filtering). The mitigations are write-side blast-radius bounds.

**When sandboxing fails:**
- `sandbox-exec` missing from PATH → hard error (`Error::MissingDependency`); set `unsandboxed = true` to bypass.
- `bwrap` missing from PATH on Linux → hard error (`Error::MissingDependency`); install `bubblewrap` or set `unsandboxed = true` to bypass.
- `ready-set-encrypt-launcher.exe` missing from PATH on Windows → hard error (`Error::MissingDependency`); install it alongside the plugin or set `unsandboxed = true` to bypass.
- Profile fails to parse (plugin bug) → hard error before spawning.

On Linux, commands are wrapped in `bwrap` with a read-only host root and
writable overlays for the project root, system temp directory, `~/.cache`, and
declared `sandbox_write_paths`.

On Windows, commands are spawned through `ready-set-encrypt-launcher.exe`,
which creates an AppContainer, grants per-path write ACLs for the project root,
temp/cache paths, and declared `sandbox_write_paths`, then starts the child
process with that AppContainer token.

`sandbox-exec` is technically deprecated by Apple since macOS 10.13 but still ships and works through macOS 15.x. A future release may migrate to direct `sandbox_init_with_parameters` via FFI if Apple removes the tool.

### The `--confirm` convention

`ready-set rotate` defaults to dry-run. Rotation is intentionally irreversible (the upstream secret has been replaced; there is no `undo`), so the plugin requires an explicit opt-in:

```text
ready-set rotate                              # dry-run; prints what would rotate
ready-set rotate --name SESSION_SECRET
ready-set rotate --confirm                   # actually rotates
ready-set rotate --name SESSION_SECRET --confirm
```

This is a **plugin-local convention**, not a core contract. There is no `READY_SET_CONFIRM` env var and no entry in `docs/contracts/`. The core's open question #4 (irreversibility model for `go` actions, `README.md:874`) is still pending; this plugin's `--confirm` flag is the local resolution. The plugin will adopt whichever core convention lands when it ships.

The planned `ready-set undo` will not roll back a rotation. The audit log is the rotation history; the change log only records the manifest mutations from `secrets set`.

## File ownership

This plugin writes (only via `set`):

- `.env.example` — alpha-sorted detected vars in a managed block; user-pinned keys above the block are preserved verbatim.
- `deploy/secrets/canonical.env.template` — alpha-sorted canonical inventory with fake values only. Use it as a checklist for ReadySet bundles, 1Password, Keychain, Vault, Doppler, Infisical, cloud provider dashboards, or provider CLIs; never paste real values into this file.
- `.gitignore` — appends a `# >>> ready-set-encrypt managed >>>` block. Uses **namespaced** markers that do not collide with `ready-set-rust`'s generic `# >>> ready-set managed >>>` block.
- `.gitleaks.toml` — bundled defaults complementing gitleaks' upstream rules. `--force` required to overwrite a divergent file.
- `.ready-set/plugins/secrets/manifest.toml` — rotation manifest. First run creates it with `backend = "manual"` for every detected env var; subsequent runs additively reconcile (append new env vars, never remove user entries; preserve comments via `toml_edit`).
- `.ready-set/changes/encrypt-*.jsonl` and `.ready-set/backups/<sha>` — SDK change log + content-addressed backups.

Plus, only via `ready-set rotate --confirm`:

- `<target_path>` (per manifest entry) — the new self-issued secret value. 0600 on Unix.
- `.ready-set/plugins/secrets/rotations.jsonl` — append-only audit log.

The plugin does **not** write `.ready-set.toml` — owned by `ready-set-rust`.

## Project-local scanner config

Projects can tune inventory behavior with:

```text
.ready-set/plugins/secrets/config.toml
```

Supported keys:

- `include_paths` — project-relative files/directories to scan. Defaults to the whole project.
- `declared_files` — example/template dotenv files whose keys are part of the canonical inventory. Defaults to `.env.example`.
- `local_files` — ignored plaintext dotenv files to read for key names only. Defaults to `.env`.
- `ignore_names` — env names to suppress from source-reference detection.
- `allow_declared_orphans` — treat declared template keys as intentional even when code does not directly reference them.

Optional external leak scan hook:

```toml
[leak_scan.privacy_filter]
enabled = true
command = "scripts/ready-set-privacy-filter"
args = []
mode = "report"
model_dir = "models/privacy-filter"
```

`ready-set-encrypt` does not include a privacy-filter model, hosted service, or
OpenAI adapter. When enabled, it runs the configured command as a project-local
adapter. The adapter receives a JSON request on stdin and writes a JSON response
on stdout:

```json
{
  "schema": "ready_set.privacy_filter_request.v1",
  "mode": "report",
  "model_dir": "models/privacy-filter",
  "blocks": [{ "block_id": "src/main.rs", "text": "..." }]
}
```

```json
{
  "schema": "ready_set.privacy_filter_response.v1",
  "blocks": [
    {
      "block_id": "src/main.rs",
      "spans": [{ "label": "secret", "start_offset": 42 }]
    }
  ]
}
```

Projects that want semantic detection can plug in their own adapter, including
one that calls the OpenAI API with a project-approved model and prompt. Only
enable that path when sending the scanned source blocks to the adapter's
provider is acceptable for the repository.

Generated `ready-set-encrypt` managed blocks are stripped before declared-file
keys are read. That prevents an old false-positive scan from becoming canonical
forever.

## Security model

- The leak-scan output (`go secrets`) reports rule IDs, file paths, and line numbers — never the matched bytes. The bundled gitleaks invocation passes `--redact`. The regex fallback formats summaries from rule IDs + line numbers only.
- `deploy/secrets/canonical.env.template` contains fake values only. It is safe to commit and is not a secret store.
- The audit log records `value_sha256` (hex digest) — never the raw value. The only place a self-issued secret appears is the `target_path` file (when configured) and stdout-once (when not).
- The SDK change log records `before_sha256` / `after_sha256` (hex digests), not contents. Backups are SHA256-content-addressed and live under `.ready-set/backups/`.

## Heuristic limits

The source-tree scan recognizes static literal references:

- Rust: `env::var("KEY")`, `env::var_os("KEY")`, `env!("KEY")`, `option_env!("KEY")`
- Web: `process.env.KEY`, `process.env["KEY"]`, `import.meta.env.KEY`, `import.meta.env['KEY']`

Dynamic lookups like `env::var(format!("X_{i}", ...))`, runtime `getenv` calls in C dependencies, or reflection-style access in dynamic languages are not detected. Pin such names by adding them to the user-curated prelude of `.env.example` (above the managed marker block); they are preserved across `set` runs.

## Excluded directories

The scanner skips `node_modules/`, `target/`, `dist/`, `build/`, `.git/`, `.next/`, `.turbo/`, `.vercel/`, `.cache/`, `.ready-set/`, `coverage/`, `.venv/`, `venv/`, `__pycache__/`. These are excluded from both inventory scans and leak scans.

## Module layout

```
src/
├── main.rs        # argv → dispatch (mirrors ready-set-rust)
├── lib.rs         # MANIFEST + provider id + .gitignore marker constants
├── bundle.rs      # encrypted bundle format + AEAD implementation
├── bundle_cli.rs  # direct `ready-set-encrypt bundle ...` commands
├── options.rs     # SetOptions, RotateOptions
├── inventory.rs   # .env parser + env-ref scanner
├── scaffold.rs    # .env.example + .gitignore block + .gitleaks.toml template
├── manifest.rs    # rotation manifest schema, load, render, additive reconcile
├── rotation.rs    # Backend enum, rotate_secret, audit log read/append, cadence
├── readiness.rs   # __ready impls for both capabilities
├── runner.rs      # __set impl with ChangeLog + atomic_write
├── workflow.rs    # __go impls (leak scan, rotation)
├── sandbox.rs     # macOS/Linux/Windows command sandbox wrapping
├── webhook.rs     # deploy webhook backend
└── templates/
    └── gitleaks.toml
```

## Direct provider protocol

Normally users go through the core:

```text
ready-set ready secrets
ready-set ready rotation
ready-set set secrets
ready-set go secrets
ready-set rotate
ready-set rotate --confirm
ready-set encrypt
ready-set encrypt --dry-run
ready-set encrypt bundle inspect deploy/secrets/root.env.rsb
```

`rotate` and `encrypt` are provider-declared command aliases in this plugin's
metadata; the dispatcher only resolves the alias target.

The provider protocol is also callable directly:

```text
ready-set-encrypt __describe
ready-set-encrypt __ready secrets
ready-set-encrypt __ready rotation
ready-set-encrypt __ready secret-bundles
ready-set-encrypt __set secrets [--dry-run] [--force]
ready-set-encrypt __set secret-bundles [--dry-run]
ready-set-encrypt __go secrets
ready-set-encrypt __go rotation [--name SECRET] [--confirm]
ready-set-encrypt __go secret-bundles
ready-set-encrypt bundle init
ready-set-encrypt bundle encrypt .env --out deploy/secrets/root.env.rsb
ready-set-encrypt bundle exec deploy/secrets/root.env.rsb -- npm run monitor:health
```

## Platform support

| Platform | Status |
|----------|--------|
| macOS    | **Supported.** Sandbox: `sandbox-exec` profile wrapping. Verified by `cargo test` on macOS 15.x. |
| Linux    | **Supported.** Sandbox: `bubblewrap` (`bwrap`) wrapping with `--ro-bind /` base + writable overlays for project_root, tmpdir, `~/.cache`, and `sandbox_write_paths`. Requires `bwrap` on `PATH` at runtime. |
| Windows  | **Supported.** Sandbox: `AppContainer` + per-path ACL grants via `ready-set-encrypt-launcher.exe`. |

## Test hygiene

End-to-end tests create project fixtures with `tempfile::TempDir`, so normal
test files are removed when the test exits. macOS sandbox tests that need an
outside-project write probe also use a unique `TempDir` with the
`ready-set-encrypt-e2e-` prefix, created outside the sandbox's allowed temp
subtree. Those probes are owned by the test and are panic-cleaned by `TempDir`
drop; tests must not write fixed files under `$HOME`.

## Out of scope (deferred)

- **Egress filtering / network policy.** Profile leaves `(allow network*)`. Read-side exfiltration via HTTPS is not mitigated.
- **`sandbox-exec` → libSandbox FFI migration.** Defensive; only if Apple removes the tool from a future macOS release.
- **First-party native provider backends** (Neon REST, Fly Machines API, Cloudflare API, Vercel API, Railway API, Render API, Netlify API, Heroku Platform API, AWS Secrets Manager, GCP Secret Manager, Azure Key Vault, Vault, Doppler, Infisical, 1Password Connect, GitHub Actions secrets). Use `exec` for these — that's the whole point.
- **Rollback-on-deploy-failure.** Sequential fail-fast; partial deploys are surfaced via the audit log.

## License

Licensed under either of MIT or Apache-2.0, at your option.
