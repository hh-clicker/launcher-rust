# Reviewed legal inputs

This directory is public build input and must be kept in the source repository.
It is not a new dependency or an update to component versions.

- `required-files.json` identifies mandatory source-relative legal files for the exact
  reviewed crate versions. Missing/empty files fail resource packaging. Other discovered
  legal files are still collected; this list is a floor, not an exhaustive license list.
- `fonts.json` identifies the actual three packed fonts by runtime key, original bytes,
  SFNT copyright/version, license path and role. Update it only after reviewing any
  deliberate font replacement. It does not configure rendering or change the fonts.
- `supplements.json` maps immutable upstream commits and file hashes to the exact crates
  whose published package omitted workspace license texts. `upstream/` contains those
  original bytes. The collector validates both hashes and published crate VCS provenance.

The egui 0.31.1 workspace commit is taken from the published crates' `.cargo_vcs_info.json`.
The same approach pins clipboard-win 5.4.3 and profiling 1.0.18. Legal text retrieval was
performed during review; builds use these checked-in files offline, not a moving branch.
The broader lockfile inventory can include other-platform/build-only packages and is
not evidence that each component was linked into the Windows executable.

MPL SVG crates are not modified here. Their cached source archives must match Cargo.lock,
and extracted source must match those archives; modified MPL sources require a revised
source-availability offer and must not be described as the unchanged upstream release.
`vendor/epaint` is a separate MIT/Apache patch with its own licenses and PATCHES.md.

File/header selection is bounded and not a proof that arbitrary embedded notices have
all been discovered. For example, ring's published fiat LICENSE refers to AUTHORS that
is absent from the crate archive. The original reference is preserved, not synthesized.
See `THIRD_PARTY_NOTICES.md` for collection scope and artifact verification instructions.
