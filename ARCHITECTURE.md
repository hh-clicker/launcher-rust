# Architecture

The launcher is a Windows x64 desktop application. egui/eframe renders through OpenGL;
there is no WebView or bundled JVM for the launcher itself. The implementation uses
Rust modules and concrete functions; it does not mirror Kotlin classes or Java packages.

## Module boundaries

| Module | Responsibility |
|---|---|
| `main` | CLI validation, resource and instance locks, font initialization, window creation |
| `ui` | UI state, navigation, user events and rendering; `ui/view.rs` holds presentation |
| `ui/worker` | Serialized long-running actions and publication of state snapshots |
| `preflight` | Read-only executable discovery, versions and prerequisite checks |
| `setup` | Explicit preparation, downloads, signature checks, installation and cleanup |
| `runtime` | Component state, Compose ownership, start/stop, observation and auth orchestration |
| `process` | Argument-safe child execution, bounded output, cancellation and timeouts |
| `platform/windows` | Windows API, registry, shell integration, TCP tables, mutexes and jobs |
| `config` | Portable layout, validated configuration, atomic writes and build identity |
| `bundle` | Content-addressed resource extraction and integrity checking |
| `fonts`, `ui/icons` | Original fonts and physical-pixel SVG rendering |
| `auth`, `updates` | Bounded HTTP operations and response interpretation |
| `diagnostics` | Filtering at log and snapshot persistence boundaries |

## Actions and observation

The UI sends typed actions to one worker. Network/process/install operations execute
outside the rendering thread. A separate observer supplies versioned observations while
action state progresses. Late observations cannot undo a stop or overwrite a newer
operation. A preflight snapshot caches Docker discovery and container inspection only
within one check; it is discarded before subsequent actions.

Preflight never installs dependencies, changes system configuration or deletes state.
Preparation is an explicit action; normal checks remain usable after cancellation.
The WSL terminal is visible. System installers use Windows elevation. Docker Desktop
starts outside the launcher's kill-on-close job so a detached engine can survive its CLI.
Browser Agent and ordinary owned commands use jobs; no recovery kills arbitrary PIDs.

Command arguments and environment are constructed separately from shell script text.
Command failures carry `CommandError` causes (cancelled, timeout, exit or output limit)
with user-facing Display messages. OS errors retain their original causes via anyhow.
Higher-level ownership, payload and configuration failures also retain descriptive context.
External output is never interpreted as instructions or blindly persisted as safe data.

## Persistent boundaries

`data/` is mutable and independent of `app/`. Existing UUIDs, paths, config schema,
Compose labels and project names are compatibility contracts. Resource hashes select a
new immutable application directory; they do not create a new user identity.
The runtime validates ownership before controlling containers and rejects remote engines.
Only this copy's resources are stopped; no volume deletion or global Docker cleanup occurs.

The embedded bundle contains only allowlisted, hashed inputs. Extraction uses a temporary
directory, validates paths and content, then atomically publishes the resource directory.
Warm starts stream file hashes without decompressing the entire bundle again.
The original two Inter fonts and Noto Emoji are losslessly packed and mapped read-only.
The epaint patch preserves fractional font metrics and coverage gamma; SVGs use 4×4
supersampling and premultiplied alpha. Refactoring must preserve this rendering pipeline.

## Diagnostics

State snapshots omit configuration credentials and authentication codes. Known database
passwords/device codes and common credential fields are redacted at persistence boundaries.
Command logs are filtered line by line, including across pipe chunk boundaries, with a
bounded line size. Browser Agent output can contain applicant text in arbitrary exceptions;
its persistent log records lifecycle events and recognized failure codes instead of raw text.
Raw structured command stdout remains available to internal parsers and is not rewritten.
Application requests, user data and normal UI paths are not anonymized.
The external Chrome MSI owns its installer log; it is local and must not be published.
No telemetry or full environment dump is introduced. Logs and memory dumps are local data.

## Build inputs

`release-inputs.json` fixes the five service commits for the product version in Cargo.toml.
`stage.py --sources-dir ...` validates clean source checkouts and reads Git object bytes, avoiding host checkout filters;
it never edits the source repositories. Browser Agent
is compiled during packaging. Production npm dependencies are installed later into the
portable data directory, using the embedded lock-file.
`build.rs` creates the resource container and Windows version metadata. The legal collector selects recursive legal files, declared license-file paths, initial
source legal headers and pinned supplements. Selected bytes are deduplicated while
retaining component/version/path attribution and byte ranges. This is a Cargo.lock
inventory, not proof of linked code or exhaustive upstream notice coverage. Reviewed
mandatory files, font attributions and MPL source offers are checked before packing.
See THIRD_PARTY_NOTICES.md for the scope and remaining discovery limits.
Only README, project LICENSE and third-party notices are embedded as documentation.
Local QA reports, screenshots, toolchains, payload workdirs and artifacts are ignored.

A clean source checkout must include `vendor/epaint`. Toolchain/crate input versions are
pinned; host SDK and compiler selection are documented, not claimed byte-identical.
Linux/macOS launchers, automatic EXE replacement and independent payload downloads are
outside this implementation's current scope.
