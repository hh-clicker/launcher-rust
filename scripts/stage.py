"""Create an allowlisted application payload from tracked service sources only.

Does not run services, installers, migrations, npm, Gradle, or modify source repos.
"""
import argparse, hashlib, json, pathlib, subprocess
from build_inputs import product_version, is_legal_source
root = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--sources-dir', type=pathlib.Path, required=True,
                    help='Directory containing the five service checkouts')
parser.add_argument('--allow-dirty', action='store_true',
                    help='Explicit local development snapshot; not a reproducible release')
args = parser.parse_args()
workspace = args.sources_dir.resolve()
release = json.loads((root/'release-inputs.json').read_text(encoding='utf-8'))
if release['version'] != product_version():
    raise RuntimeError('Cargo version and release-inputs.json disagree')
dest = root / 'payload'
dest.mkdir(exist_ok=True)
inputs = {
    'backend': ('src/main/', 'gradle/', 'gradlew', 'gradlew.bat', 'settings.gradle.kts', 'build.gradle.kts', 'Dockerfile', '.dockerignore'),
    'frontend': ('src/', 'public/', 'package.json', 'package-lock.json', 'index.html', 'vite.config.ts', 'tsconfig', 'env.d.ts', 'nginx.conf', 'Dockerfile', '.dockerignore'),
    'browser-agent': ('src/', 'resources/hh-selectors/', 'package.json', 'package-lock.json', 'tsconfig.json', 'tsconfig.build.json'),
    'generation-agent': ('src/', 'package.json', 'Dockerfile', '.dockerignore', 'compose.yaml'),
    'infra': ('compose.yaml',),
}
manifest = {'schemaVersion': 1, 'version': product_version(), 'kind': 'working-sources' if args.allow_dirty else 'pinned-sources', 'repositories': {}, 'files': {}}
for repo, allow in inputs.items():
    source = workspace / repo
    git = ['git', '-c', 'safe.directory=' + source.as_posix(), '-C', str(source)]
    head = subprocess.check_output(git + ['rev-parse', 'HEAD'], text=True).strip()
    dirty = bool(subprocess.check_output(git + ['status', '--porcelain', '--untracked-files=normal']))
    expected = release['repositories'][repo]['commit']
    if not args.allow_dirty and (head != expected or dirty):
        raise RuntimeError(f'{repo}: expected clean checkout at {expected}; no source files were changed')
    names = subprocess.check_output(git + ['ls-files', '-z']).decode().split('\0')
    manifest['repositories'][repo] = {'commit':head, 'dirty':dirty}
    selected = []
    for name in filter(None, names):
        if not is_legal_source(name) and not any(name.startswith(p) if p.endswith('/') or p == 'tsconfig' else name == p for p in allow):
            continue
        parts = pathlib.PurePosixPath(name).parts
        if any(p in ('AGENTS.md', '.dev', 'PROJECT_HANDOFF.md', '.env', '.git', 'node_modules', 'profiles', 'snapshots', 'backups') for p in parts):
            raise RuntimeError(f'Private path selected: {repo}/{name}')
        src = source / name
        if src.is_symlink() or not src.is_file() or not src.resolve().is_relative_to(source.resolve()):
            raise RuntimeError(f'Unexpected source type: {repo}/{name}')
        out = dest / repo / name
        out.parent.mkdir(parents=True, exist_ok=True)
        # Clean pinned builds use repository bytes, independent of core.autocrlf
        # and checkout filters on the build machine. Explicit dirty snapshots use
        # working-tree bytes and record that fact in the manifest.
        content = src.read_bytes() if args.allow_dirty else subprocess.check_output(git + ['show', f'HEAD:{name}'])
        out.write_bytes(content)
        selected.append(name)
        manifest['files'][f'{repo}/{name}'] = hashlib.sha256(out.read_bytes()).hexdigest()
    # Never silently retain files from an older payload after sources were removed.
    for out in (dest / repo).rglob('*'):
        relative=out.relative_to(dest / repo).as_posix()
        if repo=='browser-agent' and (relative.startswith('dist/') or relative.startswith('node_modules/')):
            continue
        if out.is_file() and relative not in selected:
            raise RuntimeError(f'Stale payload file; use a fresh staging directory: {out}')
(dest / 'release-manifest.json').write_text(json.dumps(manifest, ensure_ascii=False, sort_keys=True, indent=2) + '\n', encoding='utf-8')
print(f'Staged {len(manifest["files"])} application files; no user data or private instruction files')
