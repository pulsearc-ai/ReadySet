# Security Policy

Thanks for helping keep ReadySet and its users safe.

## Supported versions

ReadySet is pre-`0.1.0`. Only the latest pre-release on crates.io
receives security fixes:

| Version            | Supported          |
|--------------------|--------------------|
| `0.1.0-alpha.*`    | :white_check_mark: |
| Older / unreleased | :x:                |

Once `0.1.0` ships, this table will list the supported stable line.

## Reporting a vulnerability

**Please do not open a public GitHub issue for security-sensitive
reports.**

Email **security@pulsearc.ai** with:

- A description of the issue and where it lives in the code.
- Reproduction steps or a proof-of-concept if available.
- Your assessment of impact (e.g. arbitrary write outside the project
  root, change-log tampering, plugin escape).
- Your name / handle and how you'd like to be credited (if at all) in
  the fix announcement.

You can expect:

- An acknowledgement within **3 business days**.
- A first triage and severity assessment within **7 business days**.
- A fix targeted for the next pre-release if the issue is confirmed.
- Coordinated disclosure once a fix has been published to crates.io.

## What's in scope

ReadySet is a tool that writes to project filesystems on behalf of the
user. Particularly interested in:

- Path traversal in `set`, `go`, or the change log paths.
- Anything that lets a plugin escape the documented dispatcher↔plugin
  contract (env contract leakage, arbitrary core mutation, etc.).
- Tampering with `.ready-set/changes/` or `.ready-set/backups/` that
  would break `undo` reversibility.
- Cache poisoning of `~/.cache/ready-set/plugins.json`.
- Anything that turns a malicious plugin manifest into code execution
  outside the plugin's own process.

## What's out of scope

- Bugs in third-party plugins not maintained by PulseArc (report them
  upstream).
- Vulnerabilities in dependencies already known to `cargo audit` —
  Dependabot tracks these and we update on their cadence.
- Issues that require local code execution by the attacker (e.g.
  "anyone with write access to your home directory can mess things
  up").

## Public advisories

Confirmed vulnerabilities are disclosed via GitHub Security Advisories
on this repository. Crate releases that contain security fixes will
mark them in the changelog and on crates.io.
