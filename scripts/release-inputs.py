"""Validate pinned build inputs, optionally exposing safe SHA outputs to Actions."""
import argparse, json, pathlib, re
from build_inputs import ROOT, product_version

def validate(record, version):
    expected = {'backend', 'browser-agent', 'frontend', 'generation-agent', 'infra'}
    if record.get('schemaVersion') != 1 or record.get('version') != version:
        raise ValueError('Release inputs must match the Cargo product version')
    if set(record.get('repositories', {})) != expected:
        raise ValueError('Expected exactly the five component repositories')
    for name, item in record['repositories'].items():
        if not re.fullmatch(r'[0-9a-f]{40}', item.get('commit', '')):
            raise ValueError(f'{name}: expected a full commit SHA')
    return record

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--github-output', type=pathlib.Path)
    parser.add_argument('--launcher-properties', type=pathlib.Path)
    args = parser.parse_args()
    record = validate(json.loads((ROOT/'release-inputs.json').read_text(encoding='utf-8')), product_version())
    if args.launcher_properties:
        props = args.launcher_properties.read_text(encoding='utf-8')
        version = re.search(r'^hhClickerVersion=(.+)$', props, re.MULTILINE)
        if not version or version[1].strip() != record['version']:
            raise ValueError('Rust and Kotlin product versions disagree')
    if args.github_output:
        with args.github_output.open('a', encoding='utf-8') as output:
            output.write(f"version={record['version']}\n")
            for name, item in sorted(record['repositories'].items()):
                output.write(f"{name.replace('-', '_')}={item['commit']}\n")
    else:
        print(json.dumps(record, indent=2))

if __name__ == '__main__':
    main()
