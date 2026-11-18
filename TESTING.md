# Testing

Commands in this document are instructions, not evidence that they have been run.
Test/build/GUI/service execution requires explicit authorization in the working session.
After structural changes, behavior is unconfirmed until the checks below are repeated.

## Static review

Inspect `.gitignore`, `release-inputs.json`, embedded document/file allowlists and
third-party license attribution. Review public files for personal paths, accounts,
credentials and copied local reports. Preserve upstream author and license information.
The generated payload must come from the intended pinned source commits; never silently
substitute a different local working tree. Review the separate manual workflow YAML
without dispatching it. No commit, tag, publication or CI run is part of static review.

## Offline automated checks

From a prepared Windows x64 checkout, with payload staged as described in README:

```powershell
. ./scripts/env.ps1
python -m unittest discover -s ./tests -p 'test_*.py'
cargo fmt --all -- --check
cargo test --locked --offline
cargo build --release --bin hh-clicker-launcher --locked --offline
python ./scripts/package-single.py
python ./scripts/verify-single-exe.py ./dist/HH-Clicker-Rust-portable-3.1.0-win-x64.exe
```

Use the current Cargo product version in the verification path. The full packaging
command is `./scripts/build.ps1 -Test -Package -SourcesDir <source-directory>`.
Python tests check release input validation and public resource policy. Rust tests use
synthetic local fixtures and fake command execution where appropriate. Some tests execute
PowerShell or temporary helper processes, inspect Windows TCP tables and bind temporary
loopback listeners; they are not a blanket permission to execute the suite.
They do not install software, authorize an account or start real service containers.
The PE verifier checks embedded payload/file hashes, selector resources, notices,
architecture and exclusion of runtime dependencies/private files.

## Offline GUI review

Copy the EXE into a fresh QA folder first. Never run development QA against working data.
For an isolated data directory, use an absolute path computed from that QA folder:

```powershell
$QaRoot = (Resolve-Path ./qa/gui-check).ProviderPath
$QaData = Join-Path $QaRoot 'data'
& "$QaRoot/HH-Clicker-Rust-portable-3.1.0-win-x64.exe" --offline --data-dir $QaData --quit-after 30
```

The QA directory and EXE must already exist. `--offline` disables real probes and service
actions. `--settings` opens settings. Optional `--scale 1`, `1.25` or `1.5` changes the
window size for GUI checking; warn the person using the desktop before opening a scaled
window. `--scale` and `--quit-after` require offline mode.

`HH_RUST_UI_PREVIEW=ready` plus `--offline` and explicit `--data-dir` activates synthetic
presentation states. F8 / Shift+F8 cycle them. Clear the variable afterwards. Preview
frames are not service, installer or authentication tests. Check main/settings, scrolling,
long paths/messages, progress, disabled/hover/active controls, update warnings and Codex
states. Confirm original Inter metrics/SVG edges at 100/125/150%. F12 has no diagnostics
panel. No screenshot or real device code belongs in public source or build artifacts.

## Explicit manual integration checks

Use a separate writable portable copy and only its own UUID/projects/data. Obtain
separate authorization before installing components, using real accounts or controlling
working services. A useful sequence is:

1. Missing prerequisites: virtualization/WSL reboot, Docker install and engine start,
   portable Node with npm, Chrome first-run Continue and download cleanup/retry.
2. Preflight: compatible versions, optional Codex login, unavailable engine, occupied
   IPv4/IPv6 port attribution and automatic port selection without firewall prompts.
3. First service image build and launch, Browser Agent selector loading/heartbeat,
   opening the local frontend and user-completed Codex device authorization.
4. Stop/restart, partial failure/recovery, closing during start, cancelling preparation,
   fresh-copy close without engine and stable stopping states.
5. Foreign-resource rejection and preservation of this copy's database/profile state.
6. Replace only the stopped EXE while preserving app/data, verify resource extraction
   and existing instance identity. File rollback is not database rollback.
7. Logs/snapshots: no database password, device code, credentials or applicant text;
   retain recognizable operational causes and error codes.

The optional `scenario` binary is a local QA driver and is not packaged. Its mutating
verbs require the same explicit authorization as GUI actions. Existing local reports
and memory dumps remain ignored. Do not infer a leak or exact allocation ownership
from one process working-set snapshot.


## Legal collection audit

`python -m unittest discover -s ./tests -p 'test_*.py'` includes synthetic offline
fixtures for nested notices/OFL, Cargo license-file, missing required files, original
copyright bytes, path safety, deduplication with all original attributions, exclusion
of source bodies/unrelated docs, readable font notices and MPL source modification
checks. These tests do not inspect a newly built EXE by themselves.

For an artifact audit, initialize the build environment and build from the intended
payload, then use `scripts/verify-single-exe.py <new-exe>`. The verifier requires the
new legal inventory/resources, checks original byte ranges/hashes, mandatory paths,
component attributions, font bindings/copyright and MPL source-offer evidence. An old
EXE without those resources cannot be certified using only its historical checksums.
Keep old artifacts rather than overwriting them; inspect the newly built release binary
in target/release or copy it to a fresh QA output path before verification.

Resource packaging requires the exact cached MPL .crate archives for comparison with
Cargo.lock. Missing or locally modified MPL inputs block the upstream-only source offer.
The policy reports lockfile packages with no selected legal material and does not claim
all those packages are linked on Windows. See THIRD_PARTY_NOTICES for remaining limits.
