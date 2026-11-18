"""Shared, explicit locations and metadata for build/packaging scripts."""
import os
import pathlib
import re
import subprocess
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
PUBLIC_DOCUMENTS = ('README.md', 'LICENSE', 'THIRD_PARTY_NOTICES.md')

def is_legal_source(name):
    """Retain tracked upstream notices alongside allowlisted service sources."""
    parts = pathlib.PurePosixPath(name).parts
    file_pattern = r'^(?:unlicense|licen[cs]es?|copying|copyright|notices?|patents?|authors|third[-_ ]?party(?:[-_ ]?(?:licen[cs]es?|notices?))?)(?:$|[._ -])'
    return bool(parts) and (bool(re.match(file_pattern, parts[-1], re.I)) or
        any(re.fullmatch(r'licen[cs]es?|legal|notices?', part, re.I) for part in parts[:-1]))

def product_version():
    return tomllib.loads((ROOT/'Cargo.toml').read_text(encoding='utf-8'))['package']['version']

def crate_source(name, version):
    cargo_home = pathlib.Path(os.environ.get('CARGO_HOME', pathlib.Path.home()/'.cargo'))
    sources = list((cargo_home/'registry/src').glob(f'*/{name}-{version}'))
    if len(sources) != 1:
        raise RuntimeError(f'Expected one fetched source for {name}-{version}; run cargo fetch --locked')
    return sources[0]

def rust_docs():
    explicit = os.environ.get('HH_RUST_DOCS_DIR')
    if explicit:
        return pathlib.Path(explicit)
    sysroot = subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip()
    return pathlib.Path(sysroot)/'share/doc/rust'
