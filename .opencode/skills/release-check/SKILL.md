---
name: release-check
description: Audit and prepare hangar CLI or GUI release candidates, including channel-specific version consistency, Rust quality gates, artifacts, checksums, and recorded smoke-test evidence. Use for release preparation or release-pipeline diagnosis; do not publish, tag, or push without explicit user authorization.
---

# Hangar Release Check

Prepare a release candidate without treating a successful build as proof of platform behavior.

## Required checks

1. Read `AGENTS.md` and `docs/design/v0.4.0-closure-plan.md`; preserve S11 ordering and the real-HOME testing restriction.
2. Identify the release channel before changing versions:
   - CLI tag `vX.Y.Z`: CLI crate, tag, release notes, and CLI-facing documentation must agree. Do not bump GUI/Tauri merely to match a CLI release.
   - GUI tag `gui-vX.Y.Z`: GUI crate, Tauri config, tag, release notes, and GUI-facing documentation must agree. Do not bump CLI merely to match a GUI release.
3. Run write-path verification with isolated `HOME` and `CODEX_HOME`. Preserve the real `RUSTUP_HOME` and `CARGO_HOME` when required for the installed toolchain/cache.
4. Run the project gate in order: `cargo fmt`, `cargo clippy -- -D warnings`, then `cargo test`.
5. Build artifacts for the selected channel. CLI candidates require the local release CLI; GUI candidates require the platform bundle and, on macOS, `codesign --verify --deep --strict` with ad-hoc/Developer ID/notarization state recorded separately.
6. Check that every asset produced by the selected workflow is included in its `SHA256SUMS.txt` and upload patterns. RPM applies only to a GUI workflow that produces one.
7. Record platform smoke evidence separately for Linux, macOS, and Windows. Never infer a true result for an unavailable platform from compilation alone.

## Safety and stopping conditions

- Do not read or mutate real credentials merely to prove a build; real-account validation requires an explicit user request.
- Do not manufacture tokens, expiry values, or network success as release evidence.
- Do not create or push a tag, publish a GitHub Release, replace an installed binary, or delete backups unless the user explicitly authorizes that action.
- If any required platform evidence is missing, leave the corresponding closure-plan item open and report the exact gap.

## Handoff

Report the channel, candidate version/commit, commands and results, artifact paths and hashes, verified platforms, unverified platforms, and rollback location for any local replacement. Signing/notarization state is required only for GUI bundles.
