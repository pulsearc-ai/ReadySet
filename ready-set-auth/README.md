# ready-set-auth

`ready-set-auth` is a local ReadySet provider:

- the `ready-set-auth` provider binary discovered by `ready-set auth`
- audit helpers used by that provider

It contributes one capability:

| Capability | Verbs | Purpose |
| --- | --- | --- |
| `auth` | `ready`, `set`, `go` | Audit whether a web app has OAuth/OIDC login, provider identity storage, and a bridge into its existing account/session model. |

## Commands

```sh
ready-set-auth __describe
ready-set-auth __ready auth
ready-set-auth __set auth [--dry-run] [--force]
ready-set-auth __go auth
```

When installed on `PATH`, the normal dispatcher commands work:

```sh
ready-set auth
ready-set ready auth
ready-set set auth
ready-set go auth
```

`set` writes `.ready-set/plugins/auth/implementation-plan.md`. The plan is
provider- and framework-neutral; any detected paths are hints, not required
architecture. The mutation is recorded under `.ready-set/changes/` so
the planned `ready-set undo` can reverse it.

## Configuring the Audit

By default the provider looks for the first-party Rust/React layout this repo
uses. Other architectures can point the same checks at their own files with:

```toml
# .ready-set/plugins/auth/config.toml
recognize_paths = ["service/app.py"]
server_sources = ["service/app.py"]
route_markers = ["oauth_start", "oauth_callback"]
session_markers = ["issue_session"]
identity_sources = ["service/models.py"]
identity_markers = ["oauth_accounts"]
env_examples = ["config/example.env"]
env_vars = ["AUTH0_CLIENT_ID", "AUTH0_CLIENT_SECRET"]
client_sources = ["web/login.html"]
login_markers = ["/oauth/start"]
account_policy_markers = ["invite required"]
```

Markers are literal substrings. Required checks pass when every marker for that
check appears somewhere in the configured files/directories.

Deployed applications should not depend on this crate. `ready-set-auth` is for
local readiness checks and implementation planning; production auth code should
live in the application or in an explicitly chosen application dependency.

## Rust API Boundary

The Rust library API is intentionally narrow: provider metadata and provider
dispatch helpers only. This crate does not ship OAuth/JWT implementation code
as a supported application-auth SDK.
