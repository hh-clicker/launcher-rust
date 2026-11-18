// Read-only integration check against the compiled Browser Agent in a real bundle.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {isAbsolute, join} from 'node:path';
import {pathToFileURL} from 'node:url';
const [root, profiles] = process.argv.slice(2);
assert(root && profiles && isAbsolute(root) && isAbsolute(profiles));
process.env.HH_CLICKER_BROWSER_PROFILE_DIR=profiles;
const {loadHhSelectors}=await import(pathToFileURL(join(root,'dist/hh/selector-registry.js')).href);
const registry=loadHhSelectors();
for(const name of ['profile','common','search','vacancy','response-form','letter-form','resume-text']) {
    const group=JSON.parse(await readFile(join(root,'resources/hh-selectors',`${name}.json`),'utf8'));
    assert.match(registry.revision(group.id),/^[a-f0-9]{64}$/);
    for(const key of Object.keys(group.selectors)) assert(registry.locator(group.id,key));
}
console.log('Compiled Browser Agent loaded all seven selector groups; no browser/network/account actions');
