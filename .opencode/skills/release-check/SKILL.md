---
name: release-check
description: Audit and prepare hangar release candidates, including version consistency, Rust quality gates, platform bundles, checksums, and recorded smoke-test evidence. Use for release preparation or release-pipeline diagnosis; do not publish, tag, or push without explicit user authorization.
---

# Hangar Release Check

Prepare a release candidate without treating a successful build as proof of platform behavior.

## Required checks

1. Read `AGENTS.md` and `docs/design/v0.4.0-closure-plan.md`; preserve S11 ordering and the real-HOME testing restriction.
2. Confirm the intended version agrees across the CLI crate, GUI crate, Tauri config, tag, and user-facing documentation.
3. Run write-path verification with isolated `HOME` and `CODEX_HOME`. Preserve the real `RUSTUP_HOME` and `CARGO_HOME` when required for the installed toolchain/cache.
4. Run the project gate in order: `cargo fmt`, `cargo clippy -- -D warnings`, then `cargo test`.
5. Build release CLI and GUI artifacts. For macOS, verify the app bundle signature with `codesign --verify --deep --strict`; distinguish ad-hoc signing from Developer ID signing and notarization.
6. Check that every staged installer and standalone binary is included in `SHA256SUMS.txt` and the GitHub Release upload patterns, including RPM.
7. Record platform smoke evidence separately for Linux, macOS, and Windows. Never infer a true result for an unavailable platform from compilation alone.

## Safety and stopping conditions

- Do not read or mutate real credentials merely to prove a build; real-account validation requires an explicit user request.
- Do not manufacture tokens, expiry values, or network success as release evidence.
- Do not create or push a tag, publish a GitHub Release, replace an installed binary, or delete backups unless the user explicitly authorizes that action.
- If any required platform evidence is missing, leave the corresponding closure-plan item open and report the exact gap.

## Handoff

Report the candidate version/commit, commands and results, artifact paths and hashes, signing/notarization state, verified platforms, unverified platforms, and rollback location for any local replacement.
