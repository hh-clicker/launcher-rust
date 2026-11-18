"""Create deterministic resources for the single executable; never stage upstream code."""
import hashlib, json, pathlib, sys, tomllib, zipfile
from font_assets import font_assets
from font_notices import make_font_notices
from legal_collection import collect_dependencies, deduplicate, digest, verify_collection
from build_inputs import PUBLIC_DOCUMENTS, crate_source, product_version, rust_docs

root = pathlib.Path(__file__).resolve().parents[1]
files = {}
manifest = json.loads((root/'payload/release-manifest.json').read_text(encoding='utf-8'))
if manifest['version'] != product_version():
    raise RuntimeError('Payload and launcher versions disagree; regenerate the pinned payload')
payload_files = {}
for name, expected in manifest['files'].items():
    if name.startswith('browser-agent/node_modules/'):
        continue
    path = root/'payload'/name
    content = path.read_bytes()
    if hashlib.sha256(content).hexdigest() != expected:
        raise RuntimeError(f'Payload changed: {name}')
    files['payload/'+name] = content
    payload_files[name] = expected
assert 'browser-agent/dist/main.js' in payload_files
manifest['files'] = payload_files
files['payload/release-manifest.json'] = (json.dumps(manifest, ensure_ascii=False, sort_keys=True, indent=2)+'\n').encode()
for name in PUBLIC_DOCUMENTS:
    files[name] = (root/name).read_bytes()
# GUI assets are already embedded by Rust. Only their notices must be extracted.
for name in ['inter_ofl.txt', 'ORIGINAL_NOTICES.md']:
    files['licenses-and-assets/'+name] = (root/'assets'/name).read_bytes()
rust = rust_docs()
if not (rust/'licenses').is_dir() or not (rust/'COPYRIGHT-library.html').is_file():
    raise RuntimeError('Rust standard-library notices are required; use scripts/bootstrap.py or HH_RUST_DOCS_DIR')
for path in (rust/'licenses').rglob('*'):
    if path.is_file():
        files['licenses/rust-standard-library/'+path.relative_to(rust/'licenses').as_posix()] = path.read_bytes()
files['licenses/rust-standard-library/COPYRIGHT-library.html'] = (rust/'COPYRIGHT-library.html').read_bytes()
packages = tomllib.loads((root/'Cargo.lock').read_text(encoding='utf-8'))['package']
legal_inputs = set()
crate_legal, notices = collect_dependencies(root, packages, crate_source, legal_inputs)
files.update(crate_legal)
files['dependency-notices.json'] = (json.dumps(notices, indent=2)+'\n').encode('utf-8')
# This is an inventory of selected legal material from the entire lockfile,
# including build-only/other-target crates, not evidence of linked Windows code.
legal = {name:files.pop(name) for name in sorted(files) if name.startswith('licenses/')}
license_text, license_index = deduplicate(legal)
files['licenses/THIRD_PARTY_LICENSES.txt'] = license_text
files['licenses/index.json'] = (json.dumps(license_index, sort_keys=True, indent=2)+'\n').encode('utf-8')
policy = json.loads((root/'legal/required-files.json').read_text(encoding='utf-8'))
components = {f"{item['crate']}-{item['version']}" for item in notices}
collection = dict(schema_version=1, scope='Cargo.lock inventory, not a linked-code inventory',
    rules=['recursive named legal files/text in legal directories', 'Cargo license-file',
           'initial source comment blocks with copyright/SPDX markers', 'reviewed pinned supplements'],
    limits=['No inference that all lockfile entries are linked into Windows EXE',
            'No proof of completeness for notices embedded in arbitrary source/doc bodies',
            'No legal-file discovery outside the selected crate archives and reviewed supplements'],
    required_paths=sorted(f'licenses/{component}/{path}' for component,paths in policy.items()
                          if component in components for path in paths),
    collected_files={path:digest(body) for path,body in sorted(legal.items())},
    packages_without_selected_legal_material=[f"{n['crate']}-{n['version']}" for n in notices
                                            if n['review_status'] != 'collected-by-policy'])
files['legal-collection.json'] = (json.dumps(collection, indent=2)+'\n').encode('utf-8')
font_bytes, font_index = font_assets(root)
files['fonts.dat'] = font_bytes
specifications = json.loads((root/'legal/fonts.json').read_text(encoding='utf-8'))
font_notices, readable_fonts = make_font_notices(specifications, font_bytes, font_index,
                                               set(license_index)|set(files))
files['font-notices.json'] = (json.dumps(font_notices, indent=2)+'\n').encode('utf-8')
files['FONT_NOTICES.md'] = readable_fonts
mpl_lines = ['# MPL-2.0 source availability', '',
    'The following MPL components are unmodified relative to their checksum-verified',
    'Cargo.lock source archives. Their Source Code Form is available under MPL-2.0.',
    'The launcher license does not restrict your rights in that source.',
    'License and conditions: https://www.mozilla.org/en-US/MPL/2.0/', '']
for notice in notices:
    offer = notice['source_offer']
    if offer:
        component = f"{notice['crate']}-{notice['version']}"
        mpl_lines += [f'## {component}', '', f'Source archive: {offer["archive_url"]}',
                      f'SHA256: {offer["archive_sha256"]}',
                      f'License text: licenses/{component}/LICENSE.txt (via licenses/index.json).', '']
files['MPL_SOURCE_OFFERS.md'] = ('\n'.join(mpl_lines)+'\n').encode('utf-8')
# Check evidence/mandatory material before emitting any distributable archive.
verify_collection(files)
pathlib.Path(sys.argv[1]).with_name('legal-inputs.txt').write_text(
    '\n'.join(str(path.resolve()) for path in sorted(legal_inputs))+'\n', encoding='utf-8')
pathlib.Path(sys.argv[1]).with_name('font-index.json').write_text(json.dumps(font_index, indent=2)+'\n', encoding='utf-8')
files['checksums.json'] = (json.dumps({n: hashlib.sha256(b).hexdigest() for n,b in sorted(files.items())}, indent=2)+'\n').encode()
destination = pathlib.Path(sys.argv[1])
destination.parent.mkdir(parents=True, exist_ok=True)
index = {}
for name, content in sorted(files.items()):
    parts = pathlib.PurePosixPath(name).parts
    assert not any(p.lower() in ['..', '.dev', '.git', 'agents.md', 'project_handoff.md', '.env', 'data', 'profiles', 'node_modules'] for p in parts), name
    index[name] = dict(bytes=len(content), sha256=hashlib.sha256(content).hexdigest())
index_bytes = json.dumps(index, sort_keys=True, separators=(',', ':')).encode()
def entry(name):
    result = zipfile.ZipInfo(name, date_time=(2026,1,1,0,0,0))
    result.compress_type = zipfile.ZIP_DEFLATED
    result.external_attr = 0o100644 << 16
    return result
with zipfile.ZipFile(destination, 'w', zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
    with archive.open(entry('resources.bin'), 'w') as stream:
        stream.write(b'HHRES001'+len(index_bytes).to_bytes(4,'little')+index_bytes)
        for name in index:
            stream.write(files[name])
# Warm launches only decompress this small trusted index, never the resource data.
with zipfile.ZipFile(destination.with_name('resource-index.zip'), 'w', zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
    archive.writestr(entry('index.json'), index_bytes, compresslevel=9)
destination.with_name('resource-id.txt').write_text(hashlib.sha256(destination.read_bytes()).hexdigest(), encoding='ascii')
print(f'Embedded {len(files)} files; no node_modules; {destination.stat().st_size} compressed bytes')
