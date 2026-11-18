# HH Clicker Launcher

Windows x64 portable launcher implemented in Rust with egui/eframe and OpenGL.
Repository: [hh-clicker/launcher-rust](https://github.com/hh-clicker/launcher-rust).
The title bar retains **Rust portable** to distinguish it from the Kotlin launcher.
The product version matches the main HH Clicker version; Cargo.toml is the version
source for this implementation. This project does not replace the Kotlin release pipeline.

[Architecture](ARCHITECTURE.md) · [Testing](TESTING.md) ·
[Agent instructions](AGENTS.md) · [Third-party notices](THIRD_PARTY_NOTICES.md)

## Run portable

Put the single EXE in a writable folder. It extracts verified application resources
and licenses to `app/<resource-hash>/`; mutable state is kept separately in `data/`.
No Rust, JVM, Python, Git or Visual Studio installation is needed on the user's host.
Backend Java and build tools run inside Docker images.

Prerequisites are WSL 2 (package 2.1.5 or newer), hardware virtualization, Docker Desktop
with its WSL 2 Linux backend, Google Chrome, and Node.js satisfying
`^22.18.0 || >=24.12.0`. The settings page offers explicit preparation actions for
missing components, including a portable Node.js runtime with npm. Installers require
acceptance of the corresponding terms and normal Windows elevation prompts.

Preparation refreshes checks automatically. WSL uses a visible terminal and may require
a manual reboot. Docker Desktop is started through its CLI with an executable fallback.
After Chrome installation, open Chrome manually to finish its first-run setup, then
press **Продолжить**. Successfully installed component downloads are removed; failed or
cancelled downloads remain available for retry.

The EXE includes service source inputs, compiled Browser Agent JavaScript and selector
resources. Browser Agent production dependencies are installed during explicit preparation
using `npm ci --omit=dev --ignore-scripts`. npm configuration and cache are isolated;
Node.js distributions keep their upstream licenses. First preparation and Docker image
builds require Internet access. Prepared resources and existing images are reused locally.

**Запустить** starts PostgreSQL, Backend, Frontend, Generation Agent and Browser Agent.
**Открыть HH Clicker** opens this copy's frontend. Codex authorization is optional for
reaching the main screen; AI operations require authorization in Generation Agent.
The user completes device authentication on the OpenAI page. The launcher does not
supply an account or treat the existence of credentials as proof of authorization.

## Data and isolation

```text
HH-Clicker-Rust-portable-<version>-win-x64.exe
app/<hash>/payload/      replaceable service sources and Browser Agent
app/<hash>/fonts.dat     the three original GUI fonts
app/<hash>/licenses/     collected legal texts and attribution index
data/config.json        instance UUID, ports, generated database password, settings
data/postgres/          this copy's database
data/generation/        Generation Agent state
data/codex/             this copy's Codex authorization
data/browser-profiles/  this copy's Chrome profiles
data/browser-agent/     prepared JavaScript runtime dependencies
data/runtimes/          optional portable Node.js
data/logs/              operational diagnostics
data/launcher-state.json filtered launcher state snapshot
```

Resources use `hh-rust-<uuid>` and `hh-rust-<uuid>-generation` Compose projects.
These identifiers are part of compatibility with existing portable copies.
Host service ports bind to loopback; PostgreSQL is not published to the host.
Default port ranges are 28080–28179, 23000–23099 and 27374–27473.
Port checks read Windows IPv4/IPv6 TCP tables without opening listening sockets.
Foreign resources and remote Docker endpoints are refused.

Closing stops resources owned by this copy and preserves data. A fresh copy that has
not started or observed its own containers does not require an engine to close.
An abnormal launcher exit terminates its Browser Agent through a Windows Job Object;
containers can remain running until the next explicit management operation.

Stop services and close the launcher before replacing the EXE or moving the folder.
Keep `app/` and `data/`; a different resource set is extracted to a separate directory.
Do not duplicate `data/` for simultaneously running copies: it contains the existing
owner identity. Replacing an EXE does not roll back database changes.
Update checks link to official HH Clicker releases; there is no automatic replacement
of this portable EXE with the Kotlin installer.

## Build from source

Build prerequisites: Windows x64, Python 3.11+, Node.js with npm, MSVC x64 Build Tools,
Windows SDK and Git for preparing source checkouts. Git is not a runtime dependency.
`toolchain.json` pins verified Rust archives; `Cargo.lock` pins Rust dependencies.
The required epaint patch is included under `vendor/epaint` with upstream licenses.
Keep `legal/` as source input: it contains reviewed required paths, font attributions
and pinned upstream license supplements. The resource builder does not fetch legal
texts from the network. See [Third-party notices](THIRD_PARTY_NOTICES.md) for coverage;
a valid archive checksum alone does not establish legal completeness.
Git must preserve the original bytes under `legal/upstream/`; `.gitattributes` disables
line-ending conversion there because packaging verifies pinned upstream SHA-256 hashes.

Prepare separate clean checkouts of the five repositories below at the exact commits
listed in `release-inputs.json`. Pass their parent directory explicitly; scripts never
clone, reset, clean or switch the service repositories themselves.

```text
sources/
  backend/
  browser-agent/
  frontend/
  generation-agent/
  infra/
launcher-rust/
```

From the Rust launcher checkout:

```powershell
python ./scripts/bootstrap.py
. ./scripts/env.ps1
cargo fetch --locked
./scripts/build.ps1 -Test -Package -SourcesDir ../sources
```

These commands download tools/dependencies, run tests and build the EXE when executed.
`-Package` first stages payload, compiles Browser Agent, then optionally tests and builds.
The output is `dist/HH-Clicker-Rust-portable-<version>-win-x64.exe`.
Packaging refuses to overwrite an existing artifact. Use a fresh output directory;
never remove a working portable installation to make a build succeed.

The default packaging path rejects dirty or incorrectly pinned service checkouts.
For an explicit local working-tree snapshot only, add `-AllowDirty`; the generated
manifest records commit and dirty status. This mode is not a reproducible release.
It includes tracked service files only. Never edit source checkouts during staging.
Existing ignored payload is not evidence of a reproducible build: regenerate it from
pinned inputs before distribution. Clean pinned staging reads Git object bytes, avoiding checkout line-ending/filter changes.
Source inputs and manifest are allowlisted and hashed.

`env.ps1` uses directory-local Cargo/Rust tools, a static CRT and neutral remapped source
paths so compiler diagnostics do not embed the local build directory. The resource builder
honors `CARGO_HOME`; standard-library notices come from the active Rust sysroot
(or an explicit `HH_RUST_DOCS_DIR`). No personal paths are required.
Reproducible inputs do not imply byte-identical EXEs across different SDK/MSVC versions.

## Manual build workflow

Starting with 3.1.0, the main release workflow also builds this launcher after the
Windows installer, using the same release tag for both launchers. It requires matching
product versions and service SHAs in both manifests. `rust-release.yml` uploads
`HH-Clicker-Rust-portable-<version>-win-x64.exe` and its SHA-256 to the existing draft;
it refuses to modify published releases. Linux and macOS builds follow the Rust job.

The separate `rust-windows-portable.yml` workflow belongs in
[hh-clicker/.github](https://github.com/hh-clicker/.github), under `.github/workflows/`.
It runs only via `workflow_dispatch`, checks out `hh-clicker/launcher-rust`, verifies
the product version against the selected Kotlin launcher, gets the five pinned service
commits, tests/builds Windows x64 and uploads only the EXE as an Actions artifact.
It does not create tags, releases or commits. Existing release workflows are independent.
Private cross-repository checkouts use the existing `RELEASE_REPOSITORIES_TOKEN` secret;
public checkouts can use the workflow token. The workflow must be on the default branch
before it can be dispatched. See [Testing](TESTING.md) before running checks.
