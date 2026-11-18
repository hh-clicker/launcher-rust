"""Offline legal collection from exact crate sources; no linkage/compliance inference."""
import hashlib
import json
import os
import pathlib
import re
import tarfile
import tomllib

TEXT_SUFFIXES = {'', '.txt', '.md', '.rst', '.html', '.htm', '.license', '.licence'}
LEGAL_NAME = re.compile(
    r'^(?:licen[cs]es?|copying|copyright|notices?|patents?|authors|ofl|unlicense|'
    r'third[-_ ]?party(?:[-_ ]?(?:licen[cs]es?|notices?))?)(?:$|[._ -])', re.I)
LEGAL_DIR = re.compile(r'^(?:licen[cs]es?|legal|notices?)$', re.I)
SOURCE_SUFFIXES = {'.rs', '.c', '.h', '.cc', '.cpp', '.s', '.asm', '.inc', '.py', '.pl', '.sh'}
NON_TEXT_SUFFIXES = SOURCE_SUFFIXES | {'.png', '.jpg', '.jpeg', '.gif', '.svg', '.ttf', '.otf',
                                    '.woff', '.woff2', '.exe', '.dll', '.zip', '.gz', '.pyc', '.wasm'}

class LegalCollectionError(RuntimeError):
    pass

def digest(content):
    return hashlib.sha256(content).hexdigest()

def legal_filename(relative):
    path = pathlib.PurePosixPath(relative)
    # SPDX-bearing names such as license-apache-2.0 and
    # LICENSE-Apache-2.0_WITH_LLVM-exception have non-document suffixes.
    return ((bool(LEGAL_NAME.match(path.name)) and path.suffix.lower() not in NON_TEXT_SUFFIXES) or
            (path.suffix.lower() in TEXT_SUFFIXES and
             any(LEGAL_DIR.fullmatch(part) for part in path.parts[:-1])))

def read_required(source, relative, component, allow_empty=False):
    path = pathlib.PurePosixPath(relative)
    if not path.parts or path.is_absolute() or any(p in {'.', '..'} for p in path.parts) or '\\' in relative or ':' in relative:
        raise LegalCollectionError(f'{component}: unsafe legal path {relative!r}')
    target = source.joinpath(*path.parts)
    if not target.resolve().is_relative_to(source.resolve()):
        raise LegalCollectionError(f'{component}: legal path escapes crate: {relative}')
    for parent in [target, *target.parents]:
        if parent == source.parent:
            break
        if parent.is_symlink():
            raise LegalCollectionError(f'{component}: symlink in legal path: {relative}')
    try:
        stat = target.stat()
        if not target.is_file() or getattr(stat, 'st_file_attributes', 0) & 0x400:
            raise OSError('not a regular file')
        content = target.read_bytes()
    except OSError as error:
        raise LegalCollectionError(f'{component}: missing required legal file {relative}') from error
    if not allow_empty and not content.strip():
        raise LegalCollectionError(f'{component}: empty required legal file {relative}')
    return content

def legal_headers(content):
    """Yield only initial comment blocks bearing legal markers, with original bytes."""
    offset = 3 if content.startswith(b'\xef\xbb\xbf') else 0
    while offset < len(content):
        while offset < len(content) and content[offset:offset+1].isspace():
            offset += 1
        start = offset
        if content[offset:offset+2] == b'/*':
            end = content.find(b'*/', offset+2)
            if end < 0:
                break
            offset = end+2
        elif content[offset:offset+2] == b'//' or content[offset:offset+2] in {b'# ', b'#\t'} or content[offset:offset+3] == b'#!/':
            prefix = b'//' if content[offset:offset+2] == b'//' else b'#'
            def line_comment(at):
                if prefix == b'//':
                    return content[at:at+2] == b'//'
                return content[at:at+2] in {b'# ', b'#\t', b'#\n', b'#\r'} or content[at:at+3] == b'#!/'
            while offset < len(content) and line_comment(offset):
                end = content.find(b'\n', offset)
                offset = len(content) if end < 0 else end+1
                while offset < len(content) and content[offset:offset+1] in {b' ', b'\t'}:
                    offset += 1
        else:
            break
        block = content[start:offset]
        if b'copyright' in block.lower() or b'spdx-license-identifier:' in block.lower():
            if len(block) > 65536:
                raise LegalCollectionError('Oversized legal header; add an explicitly reviewed legal file')
            yield start, block

def collect_crate(source, package, required=()):
    """Return source-relative legal bytes and original-path/byte-range evidence."""
    source = pathlib.Path(source)
    component = f"{package['name']}-{package['version']}"
    metadata = tomllib.loads((source/'Cargo.toml').read_text(encoding='utf-8'))['package']
    if (metadata.get('name'), metadata.get('version')) != (package['name'], package['version']):
        raise LegalCollectionError(f'{component}: crate metadata does not match Cargo.lock')
    selected = {}
    origins = {}
    declared = metadata.get('license-file') or metadata.get('license_file')
    def add(path, reason):
        selected[path] = read_required(source, path, component)
        origins[path] = dict(path=path, kind=reason)
    if declared:
        add(declared, 'cargo-license-file')
    for path in required:
        add(path, 'reviewed-required-file')
    for directory, dirs, names in os.walk(source, followlinks=False):
        dirs[:] = sorted(d for d in dirs if d not in {'.git', 'target', 'node_modules'}
                         and not pathlib.Path(directory, d).is_symlink())
        for name in sorted(names):
            path = pathlib.Path(directory, name)
            relative = path.relative_to(source).as_posix()
            if relative in selected:
                continue
            if legal_filename(relative):
                add(relative, 'recursive-legal-file')
            elif path.suffix.lower() in SOURCE_SUFFIXES:
                # Do not copy source bodies or whole READMEs to stand in for notices.
                if path.is_symlink() or not path.resolve().is_relative_to(source.resolve()):
                    continue
                for offset, header in legal_headers(path.read_bytes()):
                    key = f'@headers/{relative}/{offset}.txt'
                    selected[key] = header
                    origins[key] = dict(path=relative, kind='source-legal-header',
                                        offset=offset, bytes=len(header))
    return selected, origins, metadata

def verify_registry_source(source, package, archive):
    """For MPL, require the exact Cargo.lock archive and unmodified extracted sources."""
    component = f"{package['name']}-{package['version']}"
    archive = pathlib.Path(archive)
    if not archive.is_file():
        raise LegalCollectionError(f'{component}: source archive required to verify MPL source offer; restore {archive.name}')
    if digest(archive.read_bytes()) != package.get('checksum'):
        raise LegalCollectionError(f'{component}: source archive checksum differs from Cargo.lock')
    expected = set()
    with tarfile.open(archive, 'r:gz') as tar:
        for member in tar:
            if member.isdir():
                continue
            path = pathlib.PurePosixPath(member.name)
            if not member.isfile() or not path.parts or path.parts[0] != component or '..' in path.parts:
                raise LegalCollectionError(f'{component}: unsupported path in source archive')
            relative = pathlib.PurePosixPath(*path.parts[1:]).as_posix()
            expected.add(relative)
            if read_required(pathlib.Path(source), relative, component, allow_empty=True) != tar.extractfile(member).read():
                raise LegalCollectionError(f'{component}: locally changed MPL source {relative}; provide modified sources and update the source offer before packaging')
    actual = {p.relative_to(source).as_posix() for p in pathlib.Path(source).rglob('*') if p.is_file()}
    if actual - expected - {'.cargo-ok'}:
        raise LegalCollectionError(f'{component}: extra MPL source files require a reviewed modified-source offer')
    return dict(status='unmodified-registry-archive-verified',
                archive_url=f'https://static.crates.io/crates/{package["name"]}/{component}.crate',
                archive_sha256=package['checksum'], license='MPL-2.0')

def deduplicate(files):
    """Every original path survives, even when its exact bytes are stored once."""
    groups = {}
    for path, content in sorted(files.items()):
        group = groups.setdefault(digest(content), dict(content=content, paths=[]))
        if group['content'] != content:
            raise LegalCollectionError('SHA256 collision in legal collection')
        group['paths'].append(path)
    text = bytearray(b'HH Clicker Launcher - collected license and notice texts\n')
    index = {}
    for sha, group in sorted(groups.items()):
        text.extend(('\n\n'+'='*72+'\n'+'\n'.join(group['paths'])+'\nSHA256 '+sha+'\n'+'='*72+'\n\n').encode('utf-8'))
        offset = len(text)
        text.extend(group['content'])
        for path in group['paths']:
            index[path] = dict(offset=offset, bytes=len(group['content']), sha256=sha)
    return bytes(text), index

def collect_dependencies(root, packages, resolve_source, watch_inputs=None):
    """Conservative lockfile inventory, explicitly not a list of linked DLLs/crates."""
    required = json.loads((root/'legal/required-files.json').read_text(encoding='utf-8'))
    supplements = json.loads((root/'legal/supplements.json').read_text(encoding='utf-8'))
    legal = {}
    records = []
    for package in packages:
        vendor = root/'vendor'/package['name']
        if not package.get('source', '').startswith('registry+') and not vendor.is_dir():
            continue
        component = f"{package['name']}-{package['version']}"
        source = vendor if vendor.is_dir() else resolve_source(package['name'], package['version'])
        selected, origins, meta = collect_crate(source, package, required.get(component, []))
        if watch_inputs is not None:
            watch_inputs.add(source/'Cargo.toml')
            watch_inputs.update(source/origin['path'] for origin in origins.values())
        for supplement in supplements:
            if component not in supplement['components']:
                continue
            vcs_file = source/'.cargo_vcs_info.json'
            if not vcs_file.is_file() or json.loads(vcs_file.read_text(encoding='utf-8')).get('git', {}).get('sha1') != supplement['upstream_commit']:
                raise LegalCollectionError(f'{component}: supplement commit does not match published crate provenance')
            body = read_required(root, supplement['file'], component)
            if digest(body) != supplement['sha256']:
                raise LegalCollectionError(f'{component}: supplement changed: {supplement["file"]}')
            key = 'upstream-workspace/'+pathlib.PurePosixPath(supplement['file']).name
            selected[key] = body
            origins[key] = dict(kind='pinned-upstream-supplement', **supplement)
        if vendor.is_dir():
            selected['PATCHES.md'] = read_required(source, 'PATCHES.md', component)
            origins['PATCHES.md'] = dict(kind='local-patch-description', path='PATCHES.md')
        source_offer = None
        if 'MPL-2.0' in meta.get('license', ''):
            if vendor.is_dir():
                raise LegalCollectionError(f'{component}: vendored MPL sources need a reviewed modified-source offer')
            archive = source.parents[2]/'cache'/source.parent.name/(component+'.crate')
            source_offer = verify_registry_source(source, package, archive)
            if watch_inputs is not None:
                watch_inputs.add(archive)
                watch_inputs.update(path for path in source.rglob('*') if path.is_file())
        for path, body in selected.items():
            legal[f'licenses/{component}/{path}'] = body
        records.append(dict(crate=package['name'], version=package['version'],
            license=meta.get('license'), license_file=meta.get('license-file'),
            authors=meta.get('authors', []), repository=meta.get('repository'),
            inventory_scope='Cargo.lock; linkage not asserted',
            locally_modified=True if vendor.is_dir() else (False if source_offer else None),
            legal_paths={f'licenses/{component}/{p}':origin for p, origin in sorted(origins.items())},
            source_offer=source_offer,
            review_status='collected-by-policy' if selected else 'no-standalone-legal-file-or-header-found'))
    return legal, records

def verify_collection(files):
    """Content checks plus required paths/evidence; hashes alone do not prove completeness."""
    for path in ['licenses/THIRD_PARTY_LICENSES.txt', 'licenses/index.json',
                 'legal-collection.json', 'dependency-notices.json', 'font-notices.json',
                 'FONT_NOTICES.md', 'MPL_SOURCE_OFFERS.md', 'fonts.dat']:
        if path not in files:
            raise LegalCollectionError(f'Missing legal collection resource: {path}; rebuild with the audited collector')
    text = files['licenses/THIRD_PARTY_LICENSES.txt']
    index = json.loads(files['licenses/index.json'])
    inventory = json.loads(files['legal-collection.json'])
    if inventory['scope'] != 'Cargo.lock inventory, not a linked-code inventory':
        raise LegalCollectionError('Missing honest legal inventory scope')
    for path, item in index.items():
        if not path.startswith('licenses/') or '..' in pathlib.PurePosixPath(path).parts:
            raise LegalCollectionError('Unsafe legal index path')
        body = text[item['offset']:item['offset']+item['bytes']]
        if len(body) != item['bytes'] or digest(body) != item['sha256']:
            raise LegalCollectionError(f'Corrupt legal text: {path}')
    for path, expected in inventory['collected_files'].items():
        if path not in index or index[path]['sha256'] != expected:
            raise LegalCollectionError(f'Missing collected legal file: {path}')
    for path in inventory['required_paths']:
        if path not in index:
            raise LegalCollectionError(f'Missing required legal file: {path}')
    for record in json.loads(files['dependency-notices.json']):
        if not set(record['legal_paths']).issubset(index):
            raise LegalCollectionError(f'Missing component attribution: {record["crate"]}')
        if record['license'] and 'MPL-2.0' in record['license']:
            offer = record.get('source_offer')
            if not offer or offer['status'] != 'unmodified-registry-archive-verified':
                raise LegalCollectionError('Missing verified MPL source offer')
    fonts = json.loads(files['font-notices.json'])
    if len(fonts) != 3 or {f['runtime_name'] for f in fonts} != {'Inter', 'Inter Bold', 'NotoEmoji-Regular'}:
        raise LegalCollectionError('Missing attribution for the reviewed three-font set')
    data = files['fonts.dat']
    readable = files['FONT_NOTICES.md'].decode('utf-8')
    for font in fonts:
        body = data[font['offset']:font['offset']+font['bytes']]
        if digest(body) != font['sha256'] or not font['copyright']:
            raise LegalCollectionError(f'Font attribution mismatch: {font["name"]}')
        if font['copyright'] not in readable:
            raise LegalCollectionError(f'Missing readable font copyright: {font["name"]}')
        for path in font['license_paths']:
            if path not in index and path not in files:
                raise LegalCollectionError(f'Missing font license: {path}')
