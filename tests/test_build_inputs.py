"""Offline policy tests. No compiler, Git, process launch or network required."""
import copy
import importlib.util
import json
import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'scripts'))
from build_inputs import PUBLIC_DOCUMENTS, product_version, is_legal_source

spec = importlib.util.spec_from_file_location('release_inputs', ROOT/'scripts/release-inputs.py')
release_inputs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release_inputs)

class BuildInputsTests(unittest.TestCase):
    def test_service_notices_survive_the_payload_allowlist(self):
        for name in ['LICENSE', 'LICENSE.md', 'LICENCE-MIT.txt', 'NOTICE.md', 'COPYING.md',
                     'THIRD_PARTY_NOTICES.md', 'licenses/Apache-2.0.md', 'legal/attribution.txt']:
            self.assertTrue(is_legal_source(name), name)
        for name in ['README.md', 'AGENTS.md', '.env', 'PROJECT_HANDOFF.md', 'src/main.rs']:
            self.assertFalse(is_legal_source(name), name)

    def setUp(self):
        self.inputs = json.loads((ROOT/'release-inputs.json').read_text(encoding='utf-8'))

    def test_versions_match_and_all_components_are_pinned(self):
        release_inputs.validate(self.inputs, product_version())

    def test_release_input_rejects_branch_or_shell_text_as_commit(self):
        for invalid in ['master', 'v3.0.0', 'abc\ninfra=master', 'a'*39, 'a'*41]:
            record = copy.deepcopy(self.inputs)
            record['repositories']['infra']['commit'] = invalid
            with self.assertRaises(ValueError):
                release_inputs.validate(record, product_version())

    def test_wrong_version_or_extra_repository_is_rejected(self):
        with self.assertRaises(ValueError):
            release_inputs.validate(self.inputs, '0.0.0')
        self.inputs['repositories']['unexpected'] = {'commit':'a'*40}
        with self.assertRaises(ValueError):
            release_inputs.validate(self.inputs, product_version())

    def test_local_reports_are_not_embedded_documents(self):
        self.assertEqual(set(PUBLIC_DOCUMENTS), {'README.md', 'LICENSE', 'THIRD_PARTY_NOTICES.md'})
        for name in PUBLIC_DOCUMENTS:
            self.assertTrue((ROOT/name).is_file(), name)

    def test_first_party_text_has_no_double_encoded_cyrillic(self):
        paths = [*ROOT.joinpath('src').rglob('*.rs')]
        paths.extend(ROOT/name for name in ['README.md', 'ARCHITECTURE.md', 'TESTING.md', 'AGENTS.md'])
        markers = ['\u0420\u00b5', '\u0421\u201a', '\u0432\u0402\u201d']
        for path in paths:
            text = path.read_text(encoding='utf-8')
            for marker in markers:
                self.assertNotIn(marker, text, str(path.relative_to(ROOT)))

if __name__ == '__main__':
    unittest.main()
