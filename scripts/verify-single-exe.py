"""Read-only verification of resources actually embedded in the delivered PE executable."""
import hashlib, io, json, pathlib, struct, sys, zipfile
from build_inputs import ROOT, PUBLIC_DOCUMENTS, product_version
path = pathlib.Path(sys.argv[1])
exe = path.read_bytes()
assert exe[:2] == b'MZ'
for local_root in {str(ROOT.resolve()), ROOT.resolve().as_posix()}:
    for encoding in ['utf-8', 'utf-16le']:
        assert local_root.encode(encoding).lower() not in exe.lower(), 'Local build path embedded in EXE; use scripts/env.ps1'
offset = struct.unpack_from('<I', exe, 0x3c)[0]
assert exe[offset:offset+4] == b'PE\0\0'
assert struct.unpack_from('<H', exe, offset+4)[0] == 0x8664
from resource_archive import embedded_resources
from legal_collection import verify_collection
blob, resources = embedded_resources(exe)
verify_collection(resources)
class ResourceView:
    def __enter__(self): return self
    def __exit__(self, *args): pass
    def read(self, name): return resources[name]
    def namelist(self): return list(resources)
with ResourceView() as archive:
    names = archive.namelist()
    assert all(name in names for name in PUBLIC_DOCUMENTS)
    assert not any(pathlib.PurePosixPath(name).name in ['VALIDATION.md', 'GUI_REVIEW.md', 'FEATURES.md'] for name in names)
    assert len(names) == len(set(names))
    for name in names:
        parts = pathlib.PurePosixPath(name).parts
        assert not any(p.lower() in ['..', 'node_modules', '.dev', '.git', 'agents.md', 'project_handoff.md', '.env', 'profiles', 'data', '.tools', 'target', 'auth.json', 'credentials.json'] for p in parts), name
    checksums = json.loads(archive.read('checksums.json'))
    assert set(names) == set(checksums)|{'checksums.json'}
    for name, expected in checksums.items():
        assert hashlib.sha256(archive.read(name)).hexdigest() == expected, name
    manifest = json.loads(archive.read('payload/release-manifest.json'))
    assert manifest['version'] == product_version(), 'Payload and launcher versions disagree'
    for name, expected in manifest['files'].items():
        assert hashlib.sha256(archive.read('payload/'+name)).hexdigest() == expected, name
    for name in ['profile', 'common', 'search', 'vacancy', 'response-form', 'letter-form', 'resume-text']:
        group = json.loads(archive.read(f'payload/browser-agent/resources/hh-selectors/{name}.json'))
        assert group.get('schemaVersion') == 1 and group.get('id') and group.get('selectors')
    assert 'payload/browser-agent/dist/main.js' in names
    assert 'payload/browser-agent/package-lock.json' in names
    assert 'THIRD_PARTY_NOTICES.md' in names
    assert any(n.startswith('licenses/') for n in names)
    if 'licenses/index.json' in names:
        texts = archive.read('licenses/THIRD_PARTY_LICENSES.txt')
        index = json.loads(archive.read('licenses/index.json'))
        assert index
        for original, item in index.items():
            assert original.startswith('licenses/') and '..' not in pathlib.PurePosixPath(original).parts
            body = texts[item['offset']:item['offset']+item['bytes']]
            assert len(body) == item['bytes'] and hashlib.sha256(body).hexdigest() == item['sha256'], original
print(json.dumps(dict(exe=str(path), exe_bytes=len(exe), sha256=hashlib.sha256(exe).hexdigest(), embedded_bytes=len(blob), embedded_files=len(names), payload_files=len(manifest['files']), machine='AMD64', node_modules=False), indent=2))
