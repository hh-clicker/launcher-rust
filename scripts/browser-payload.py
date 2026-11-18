"""Compile and stage Browser Agent; production dependencies are installed at runtime.
No services/browsers are started. All npm build state stays under .tools.
"""
import hashlib,json,os,pathlib,shutil,subprocess,uuid
root=pathlib.Path(__file__).resolve().parents[1]
payload=root/'payload'
source=payload/'browser-agent'
node=shutil.which('node.exe')
if not node: raise RuntimeError('Node.js build dependency not found')
node=pathlib.Path(node)
npm=node.parent/'node_modules/npm/bin/npm-cli.js'
if not npm.is_file():raise RuntimeError('npm next to Node.js not found')
work=root/'.tools/browser-build'/str(uuid.uuid4())
build=work/'build'
build.mkdir(parents=True)
for name in ['package.json','package-lock.json','tsconfig.json','tsconfig.build.json']:
    shutil.copyfile(source/name,build/name)
shutil.copytree(source/'src',build/'src')
empty=work/'empty.npmrc';empty.write_text('', encoding='utf-8');global_config=work/'global.npmrc';global_config.write_text('', encoding='utf-8')
env={k:v for k,v in os.environ.items() if not k.lower().startswith('npm_config_')}
env.update(npm_config_cache=str(root/'.tools/npm-cache'),npm_config_userconfig=str(empty),npm_config_globalconfig=str(global_config),PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD='1')
def run(args,cwd):subprocess.run([str(node),*map(str,args)],cwd=cwd,env=env,check=True)
run([npm,'ci','--ignore-scripts','--no-audit','--no-fund'],build)
run([build/'node_modules/typescript/bin/tsc','-p','tsconfig.build.json'],build)
for name,origin in [('dist',build/'dist')]:
    destination=source/name
    # Only replace generated distribution directories inside the exact payload root.
    assert destination.resolve().is_relative_to(source.resolve()) and destination.name in ['dist','node_modules']
    if destination.exists():shutil.rmtree(destination)
    shutil.copytree(origin,destination)
manifest_path=payload/'release-manifest.json'
manifest=json.loads(manifest_path.read_text(encoding='utf-8'))
manifest['files']={name:checksum for name,checksum in manifest['files'].items() if not name.startswith(('browser-agent/dist/','browser-agent/node_modules/'))}
for directory in [source/'dist']:
    for path in directory.rglob('*'):
        if path.is_file():manifest['files'][path.relative_to(payload).as_posix()]=hashlib.sha256(path.read_bytes()).hexdigest()
manifest_path.write_text(json.dumps(manifest,ensure_ascii=False,sort_keys=True,indent=2)+'\n',encoding='utf-8')
print('Browser Agent compiled; JavaScript and hashes staged; no npm dependencies bundled')
