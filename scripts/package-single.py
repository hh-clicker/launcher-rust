"""Publish one self-contained EXE into a fresh path. Never overwrite portable data."""
import argparse, hashlib, json, pathlib, re, shutil
from build_inputs import product_version
root = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--name', default=f'HH-Clicker-Rust-portable-{product_version()}-win-x64')
args = parser.parse_args()
if not re.fullmatch(r'HH-Clicker-Rust-portable-[0-9]+\.[0-9]+\.[0-9]+-win-x64', args.name):
    raise ValueError('Invalid artifact name')
destination = root/'dist'/(args.name+'.exe')
destination.parent.mkdir(parents=True, exist_ok=True)
with destination.open('xb') as target, (root/'target/release/hh-clicker-launcher.exe').open('rb') as source:
    shutil.copyfileobj(source, target)
print(json.dumps(dict(exe=str(destination), exe_bytes=destination.stat().st_size, sha256=hashlib.sha256(destination.read_bytes()).hexdigest()), indent=2))
