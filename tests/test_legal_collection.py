"""Synthetic offline fixtures; never invokes Cargo, Git, a compiler or the network."""
import hashlib
import io
import json
import pathlib
import struct
import sys
import tarfile
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'scripts'))
from legal_collection import (LegalCollectionError, collect_crate, deduplicate,
                              digest, legal_filename, verify_collection, verify_registry_source)
from font_notices import make_font_notices

COPYRIGHT = b'Copyright 2020 Synthetic Authors\r\nPermission is granted.\r\n'

def synthetic_font():
    strings = bytearray()
    names = []
    for key, value in [(0, 'Copyright Synthetic Font Authors'), (1, 'Synthetic Font'), (5, 'Version 1.0')]:
        encoded = value.encode('utf-16-be')
        names.append(struct.pack('>6H', 3, 1, 0x409, key, len(encoded), len(strings)))
        strings.extend(encoded)
    table = struct.pack('>3H', 0, len(names), 6+12*len(names))+b''.join(names)+strings
    return struct.pack('>I4H', 0x10000, 1, 0, 0, 0)+struct.pack('>4s3I', b'name', 0, 28, len(table))+table

class LegalCollectionTests(unittest.TestCase):
    def test_checked_in_supplements_retain_pinned_bytes(self):
        supplements = json.loads((ROOT/'legal/supplements.json').read_text(encoding='utf-8'))
        for supplement in supplements:
            with self.subTest(path=supplement['file']):
                self.assertEqual(
                    digest((ROOT/supplement['file']).read_bytes()),
                    supplement['sha256'],
                    'Checkout must preserve the original upstream license bytes',
                )

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='hh-legal-test-')
        self.root = pathlib.Path(self.temporary.name)
        self.package = dict(name='sample', version='1.2.3')
        self.write('Cargo.toml', b'[package]\nname="sample"\nversion="1.2.3"\nlicense="MIT"\n')

    def tearDown(self):
        self.temporary.cleanup()

    def write(self, path, content):
        target = self.root/path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content)
        return target

    def collect(self, required=()):
        return collect_crate(self.root, self.package, required)

    def test_nested_notices_ofl_and_original_copyright_bytes(self):
        for path in ['LICENSE', 'fonts/OFL.txt', 'third_party/fiat/LICENSE',
                     'src/polyfill/once_cell/LICENSE-MIT', 'legal/third-party.txt']:
            self.write(path, COPYRIGHT)
        selected, origins, _ = self.collect()
        for path in ['fonts/OFL.txt', 'third_party/fiat/LICENSE', 'src/polyfill/once_cell/LICENSE-MIT']:
            self.assertEqual(selected[path], COPYRIGHT)
            self.assertEqual(origins[path]['path'], path)

    def test_no_source_bodies_readmes_images_or_unrelated_docs(self):
        for path in ['README.md', 'docs/guide.md', 'src/copying.rs', 'licenses/image.png', 'legal/code.rs']:
            self.write(path, b'ordinary source or docs; do not ship as a legal file')
        self.write('src/example.rs', b'// Copyright Synthetic Authors\n// SPDX-License-Identifier: MIT\nfn private_body() {}\n')
        selected, origins, _ = self.collect()
        self.assertEqual(len(selected), 1)
        key = next(iter(selected))
        self.assertTrue(key.startswith('@headers/src/example.rs/'))
        self.assertNotIn(b'fn private_body', selected[key])
        self.assertIn(b'Copyright Synthetic Authors', selected[key])
        self.assertEqual(origins[key]['offset'], 0)

    def test_license_file_declaration_accepts_custom_filename(self):
        self.write('Cargo.toml', b'[package]\nname="sample"\nversion="1.2.3"\nlicense-file="attribution/custom.data"\n')
        self.write('attribution/custom.data', COPYRIGHT)
        selected, origins, _ = self.collect()
        self.assertEqual(selected['attribution/custom.data'], COPYRIGHT)
        self.assertEqual(origins['attribution/custom.data']['kind'], 'cargo-license-file')

    def test_missing_or_empty_declared_license_is_actionable(self):
        self.write('Cargo.toml', b'[package]\nname="sample"\nversion="1.2.3"\nlicense-file="custom.txt"\n')
        with self.assertRaisesRegex(LegalCollectionError, 'sample-1.2.3: missing required legal file custom.txt'):
            self.collect()
        self.write('custom.txt', b'')
        with self.assertRaisesRegex(LegalCollectionError, 'empty required legal file'):
            self.collect()

    def test_required_nested_notice_is_not_optional(self):
        with self.assertRaisesRegex(LegalCollectionError, 'third_party/fiat/LICENSE'):
            self.collect(['third_party/fiat/LICENSE'])

    def test_license_file_cannot_escape_component(self):
        self.write('Cargo.toml', b'[package]\nname="sample"\nversion="1.2.3"\nlicense-file="../private.txt"\n')
        with self.assertRaisesRegex(LegalCollectionError, 'unsafe legal path'):
            self.collect()

    def test_deduplication_keeps_all_component_versions_and_paths(self):
        paths = ['licenses/first-1.0/LICENSE', 'licenses/second-2.0/nested/NOTICE']
        text, index = deduplicate({path:COPYRIGHT for path in paths})
        self.assertEqual(set(index), set(paths))
        self.assertEqual(index[paths[0]], index[paths[1]])
        for path in paths:
            item = index[path]
            self.assertEqual(text[item['offset']:item['offset']+item['bytes']], COPYRIGHT)
            self.assertIn(path.encode(), text)

    def test_reviewed_policy_requires_known_missing_notices(self):
        policy = json.loads((ROOT/'legal/required-files.json').read_text(encoding='utf-8'))
        self.assertIn('fonts/OFL.txt', policy['epaint_default_fonts-0.31.1'])
        for path in ['third_party/fiat/LICENSE', 'src/polyfill/once_cell/LICENSE-APACHE',
                     'src/polyfill/once_cell/LICENSE-MIT']:
            self.assertIn(path, policy['ring-0.17.14'])

    def test_mpl_source_archive_integrity_and_local_modification_detection(self):
        self.write('src/lib.rs', b'// original source\npub fn sample() {}\n')
        self.write('empty.txt', b'')
        source = self.root/'crate'
        source.mkdir()
        members = ['Cargo.toml', 'src/lib.rs', 'empty.txt']
        for name in members:
            path = source/name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes((self.root/name).read_bytes())
        archive = self.root/'source.crate'
        with tarfile.open(archive, 'w:gz') as tar:
            for name in members:
                body = (self.root/name).read_bytes()
                info = tarfile.TarInfo('sample-1.2.3/'+name)
                info.size = len(body)
                tar.addfile(info, io.BytesIO(body))
        package = dict(self.package, checksum=digest(archive.read_bytes()))
        offer = verify_registry_source(source, package, archive)
        self.assertEqual(offer['status'], 'unmodified-registry-archive-verified')
        self.assertIn('/sample-1.2.3.crate', offer['archive_url'])
        (source/'src/lib.rs').write_bytes(b'// local modification\npub fn sample() {}\n')
        with self.assertRaisesRegex(LegalCollectionError, 'locally changed MPL source'):
            verify_registry_source(source, package, archive)

    def test_source_file_named_copying_is_not_a_license(self):
        self.assertFalse(legal_filename('src/copying.rs'))
        self.assertTrue(legal_filename('fonts/OFL.txt'))
        self.assertTrue(legal_filename('license-apache-2.0'))
        self.assertTrue(legal_filename('LICENSE-Apache-2.0_WITH_LLVM-exception'))

class FontNoticeTests(unittest.TestCase):
    def fixture(self):
        data = synthetic_font()
        spec = dict(runtime_name='Inter', name='Synthetic Font', role='fixture',
                    component='sample-1.0', source_file='font.ttf', font_version='Version 1.0',
                    sha256=digest(data), copyright='Copyright Synthetic Font Authors',
                    license='OFL-1.1', license_paths=['licenses/sample-1.0/OFL.txt'])
        index = dict(fonts=[dict(name='Inter', offset=0, bytes=len(data), sha256=digest(data))])
        return data, spec, index

    def test_readable_copyright_and_exact_font_binding(self):
        data, spec, index = self.fixture()
        records, readable = make_font_notices([spec], data, index, set(spec['license_paths']))
        self.assertIn(spec['copyright'].encode(), readable)
        self.assertIn(b'sample-1.0/font.ttf', readable)
        self.assertEqual(records[0]['sha256'], digest(data))

    def test_missing_font_license_or_changed_copyright_fails(self):
        data, spec, index = self.fixture()
        with self.assertRaisesRegex(LegalCollectionError, 'missing required font license'):
            make_font_notices([spec], data, index, set())
        spec['copyright'] = 'Incorrect attribution'
        with self.assertRaisesRegex(LegalCollectionError, 'copyright/version differs'):
            make_font_notices([spec], data, index, set(spec['license_paths']))

    def test_real_specifications_name_exactly_the_three_packed_fonts(self):
        specs = json.loads((ROOT/'legal/fonts.json').read_text(encoding='utf-8'))
        self.assertEqual({s['runtime_name'] for s in specs}, {'Inter', 'Inter Bold', 'NotoEmoji-Regular'})
        noto = next(s for s in specs if s['runtime_name'] == 'NotoEmoji-Regular')
        self.assertEqual(noto['copyright'], 'Copyright 2013 Google Inc. All Rights Reserved.')

    def collection_fixture(self):
        data, spec, index = self.fixture()
        legal_path = spec['license_paths'][0]
        text, legal_index = deduplicate({legal_path:COPYRIGHT})
        records, readable = make_font_notices([spec], data, index, {legal_path})
        fonts = [dict(records[0], runtime_name=name) for name in ['Inter', 'Inter Bold', 'NotoEmoji-Regular']]
        collection = dict(scope='Cargo.lock inventory, not a linked-code inventory',
                          collected_files={legal_path:digest(COPYRIGHT)}, required_paths=[legal_path])
        return {'licenses/THIRD_PARTY_LICENSES.txt':text,
                'licenses/index.json':json.dumps(legal_index).encode(),
                'legal-collection.json':json.dumps(collection).encode(),
                'dependency-notices.json':b'[]', 'font-notices.json':json.dumps(fonts).encode(),
                'FONT_NOTICES.md':readable, 'MPL_SOURCE_OFFERS.md':b'# No MPL fixture', 'fonts.dat':data}

    def test_integrity_does_not_substitute_for_required_path_coverage(self):
        files = self.collection_fixture()
        verify_collection(files)
        collection = json.loads(files['legal-collection.json'])
        collection['required_paths'].append('licenses/ring-0.17.14/third_party/fiat/LICENSE')
        files['legal-collection.json'] = json.dumps(collection).encode()
        with self.assertRaisesRegex(LegalCollectionError, 'Missing required legal file'):
            verify_collection(files)

    def test_verifier_rejects_missing_readable_font_copyright(self):
        files = self.collection_fixture()
        files['FONT_NOTICES.md'] = b'# Fonts without attribution'
        with self.assertRaisesRegex(LegalCollectionError, 'Missing readable font copyright'):
            verify_collection(files)

if __name__ == '__main__':
    unittest.main()
