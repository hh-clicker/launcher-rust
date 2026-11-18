# Repository instructions

## Scope

Windows x64 Rust portable implementation of HH Clicker Launcher, published under
`hh-clicker/launcher-rust`. Keep changes within the authorized repositories. Do not
initialize Git history, commit, tag, push, publish or dispatch workflows without an
explicit instruction. Git operations that create signable objects use configured signing.

## Code map

- `src/ui.rs`, `src/ui/`: presentation, navigation, background action worker and icons.
- `src/preflight.rs`: read-only prerequisite checks; `src/setup.rs`: explicit preparation.
- `src/runtime.rs`: component lifecycle and ownership; `src/process.rs`: child execution.
- `src/platform/windows.rs`: Windows handles, shell, registry and TCP tables.
- `src/config.rs`, `src/bundle.rs`: separate mutable data and replaceable resources.
- `scripts/`, `release-inputs.json`: pinned staging, license packaging and single EXE.
- `vendor/epaint`: required font rendering patch, upstream licenses and PATCHES.md.
- `legal/`: required public packaging inputs, font attribution and versioned upstream supplements.

## Invariants

Preserve tested appearance, original font/icon rendering and the Rust portable title
marker. Do not add abstractions solely to mirror Kotlin classes. Use idiomatic Rust
module/type/function names and keep unsafe contracts explicit.
First-party text is UTF-8. Specify encoding explicitly in Python text I/O instead
of inheriting the Windows locale encoding.

Preserve UUIDs, config schema, Compose labels/project names and data paths. Manage only
proven owned resources on a local engine. Never prune Docker globally or delete volumes.
Keep data across replacement/moves of a stopped portable copy. Existing credentials or
containers alone do not prove authorization/health.

Checks do not install or modify system configuration. Preparation is explicit. Blocking
operations stay off the UI thread. Preserve visible WSL, detached Desktop startup,
Chrome Continue, cancellation and stop-state ordering. UI does not parse raw CLI output.

Never log secrets, device codes, credentials, applicant text or whole environments.
Filter external output at persistence boundaries; retain useful causes and error codes.
Do not anonymize operational user data or alter AI requests. Keep local data, dumps,
reports, screenshots, backups, private instructions and tools out of public source/assets.
Preserve third-party legal names and notices.

## Build and checks

See README, ARCHITECTURE and TESTING. Keep Cargo.lock and the necessary epaint sources.
The product version matches the Kotlin launcher. Pinned packaging rejects dirty or wrong
component revisions; local working-tree staging must be explicit. Embed only allowlisted
application files and public notices. Do not include npm dependencies in the EXE.

Prepare tests for changed contracts, but do not run tests, builds, GUI, installers,
migrations or services without explicit user authorization. Do not modify working
installations, databases, profiles or containers without a separate instruction.
Report checks actually performed and distinguish static review from execution evidence.
