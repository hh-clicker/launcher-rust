# Third-party notices

The portable EXE contains a legal collection extracted under `app/<resource-hash>/`.
Paths below are relative to that directory. This describes the collection policy;
archive integrity alone is not proof that every upstream notice has been identified.

## Scope and provenance

`dependency-notices.json` is a conservative inventory of registry packages in Cargo.lock
plus the vendored epaint patch. It includes build dependencies, optional dependencies
and packages for other operating systems. It is **not a list of crates linked into the
Windows EXE**, nor an attribution of every package's whole source tree to this binary.
For example, epaint_default_fonts declares licenses for fonts that are not packed here.
The three actually packed fonts are identified separately below.

The offline collector reads the exact fetched crate versions and `vendor/epaint`.
It selects recursively named license/copyright/notice files, text in legal directories,
Cargo `license-file` declarations, and initial source comment blocks containing
copyright or SPDX markers. Source bodies and general READMEs are not copied as notices.
Selected files and header byte ranges retain their original bytes. Their component,
version and original path remain in the inventory. Identical bytes are stored once in
`licenses/THIRD_PARTY_LICENSES.txt`; `licenses/index.json` preserves every original path,
byte range and SHA256, including all attributions sharing a deduplicated text.

`legal-collection.json` records the policy, collected hashes and mandatory paths.
`legal/required-files.json` in the source repository requires reviewed paths, including
epaint_default_fonts 0.31.1 `fonts/OFL.txt`, ring 0.17.14 nested fiat/once_cell notices,
ureq's chunked notices and tracing-core's spin notice. Missing mandatory/declared files,
changed reviewed font bytes, or changed pinned supplements fail packaging explicitly.
Cargo watches selected legal inputs so their removal invalidates the resource build.

Some published crates omit workspace-level licenses. Reviewed supplements are stored
in `legal/upstream/`, with exact upstream commits, URLs, hashes and component mappings
in `legal/supplements.json`. They cover the egui 0.31.1 workspace, clipboard-win 5.4.3
and profiling 1.0.18; packaging checks their hashes and crate VCS provenance. No license
is fetched from a moving branch or downloaded during resource packaging.

This policy is bounded: notices buried in arbitrary source/doc bodies are not proven
complete by filename discovery. Inventory entries with no selected legal material are
reported explicitly. In particular, ring's fiat notice refers to an AUTHORS file not
present in the published crate; its original attribution is retained without inventing
that file. The collection is not a general certification of every lockfile package.
Rust standard-library notices supplied by the selected toolchain are collected separately.
The compiler, LLVM tools and other build executables are not shipped to users.

## Embedded fonts

`FONT_NOTICES.md` provides readable copyright/role/source information.
`font-notices.json` ties each notice to a SHA256 and byte range in `fonts.dat`.
The collector checks SFNT copyright/version records against reviewed `legal/fonts.json`.
It does not alter the fonts, their tweaks or the renderer.

| Actual packed font | Copyright (original font metadata) | License and original text |
|---|---|---|
| Inter Regular, version 4.001 / git-9221beed3 | Copyright 2016 The Inter Project Authors | OFL-1.1; `licenses-and-assets/inter_ofl.txt` |
| Inter SemiBold, version 4.001 / git-9221beed3 | Copyright 2016 The Inter Project Authors | OFL-1.1; `licenses-and-assets/inter_ofl.txt` |
| Noto Emoji Regular, version 1.05 uh, from epaint_default_fonts 0.31.1 | Copyright 2013 Google Inc. All Rights Reserved. | OFL-1.1; `licenses/epaint_default_fonts-0.31.1/fonts/OFL.txt` via the index |

Inter's original license file also retains its own copyright wording and upstream URL.
The internal font key `Inter Bold` refers to SemiBold, not an additional font. Noto Emoji
is a monochrome emoji/symbol fallback. Ubuntu Light, Hack and emoji-icon-font bytes are
not included in fonts.dat. The OFL text for Noto is from the pinned
[egui 0.31.1 font source](https://raw.githubusercontent.com/emilk/egui/0.31.1/crates/epaint_default_fonts/fonts/OFL.txt).

## MPL-2.0 SVG components

The Windows normal-dependency graph includes resvg, usvg, usvg-parser and usvg-tree
0.37.0. This graph observation does not assert that every function is retained by linking.
Their MPL-2.0 Source Code Form is available in these exact published source archives:

- [resvg 0.37.0](https://static.crates.io/crates/resvg/resvg-0.37.0.crate)
- [usvg 0.37.0](https://static.crates.io/crates/usvg/usvg-0.37.0.crate)
- [usvg-parser 0.37.0](https://static.crates.io/crates/usvg-parser/usvg-parser-0.37.0.crate)
- [usvg-tree 0.37.0](https://static.crates.io/crates/usvg-tree/usvg-tree-0.37.0.crate)

The source is offered under [MPL-2.0](https://www.mozilla.org/en-US/MPL/2.0/).
The launcher license does not restrict recipients' rights to that source. The collection
contains each crate's original LICENSE.txt. Packaging verifies the local archive against
Cargo.lock and compares the extracted source files against it before describing a crate
as unmodified. `MPL_SOURCE_OFFERS.md` and each inventory entry record the archive URL,
checksum and verification status. Cached `.crate` archives are therefore required for
this check; restore the pinned archive if it is missing.

No local modifications to these four MPL components are intended. Changed or additional
MPL source files, or a vendored MPL replacement without a reviewed modified-source offer,
block packaging. If such modifications are introduced, publish precisely those modified
MPL sources under MPL-2.0 and update their availability notice before distributing the EXE;
an upstream archive alone would not describe modified code. See Mozilla's
[MPL FAQ, distribution questions](https://www.mozilla.org/en-US/MPL/2.0/FAQ/).

## Local patch and icons

The epaint 0.31.1 patch is MIT OR Apache-2.0. Its original license texts and PATCHES.md
are retained under the `licenses/epaint-0.31.1/` index paths. The modified source is
`vendor/epaint` in the launcher source repository. It preserves coverage gamma and
fractional font sizes; it is independent of the unmodified MPL SVG crates.

Tabler SVG icons retain the MIT notice and Copyright (c) 2020–2026 Paweł Kuna in
`licenses-and-assets/ORIGINAL_NOTICES.md`. The HH Clicker application icon is supplied
by the HH Clicker project. Third-party legal author names are not anonymized.

## Application payload and downloaded components

Service source inputs and tracked legal files follow the pinned `release-inputs.json`
and generated `payload/release-manifest.json`. A notice added only in a newer service
revision does not alter an older pinned source input. Cargo collection coverage does
not certify the legal completeness of the separately staged service payload.

The compiled Browser Agent and its lock-file are embedded; production npm dependencies
are downloaded during explicit preparation, not bundled in this EXE. Their upstream
notices remain with those packages under `data/browser-agent/<payload-id>/node_modules`.
The optional unmodified official Node.js distribution is downloaded separately with
its LICENSE and verified against its release's SHASUMS256.txt.

Docker Desktop and Google Chrome are not redistributed in the initial EXE. Explicit
installation actions obtain official packages and check the expected Authenticode
publisher. WSL is a Windows component. Local data, credentials and private reports
are excluded from distributable resources.
