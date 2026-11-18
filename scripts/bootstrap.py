"""Download a verified, directory-local Rust toolchain (no installer/registry/PATH writes)."""
import concurrent.futures, hashlib, json, pathlib, shutil, tarfile, urllib.request
root = pathlib.Path(__file__).resolve().parents[1] / '.tools'
root.mkdir(exist_ok=True)
manifest = json.loads((pathlib.Path(__file__).parent / 'toolchain.json').read_text(encoding='utf-8'))
def install(package):
    record = manifest[package]
    archive = root / (package + '.tar.xz')
    if not archive.exists():
        urllib.request.urlretrieve(record['xz_url'], archive)
    assert hashlib.file_digest(archive.open('rb'), 'sha256').hexdigest() == record['xz_hash'], package
    with tarfile.open(archive) as tar:
        tar.extractall(root / 'unpacked', filter='data')
        top = tar.getnames()[0].split('/')[0]
    source = root / 'unpacked' / top / (package if package != 'rust-std' else 'rust-std-x86_64-pc-windows-msvc')
    shutil.copytree(source, root / 'rust', dirs_exist_ok=True)
    print(package + ' verified and extracted', flush=True)
with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
    list(pool.map(install, ['rustc', 'cargo', 'rust-std', 'rustfmt-preview']))
