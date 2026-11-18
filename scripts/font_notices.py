"""Legal metadata only. The renderer/font packer and original font bytes are unchanged."""
import json
import struct
from legal_collection import LegalCollectionError, digest

def font_names(content, name_id):
    """Read original SFNT name records (copyright, family, version, etc.)."""
    try:
        count = struct.unpack_from('>H', content, 4)[0]
        table = next(struct.unpack_from('>I', content, 12+16*i+8)[0]
                     for i in range(count) if content[12+16*i:16+16*i] == b'name')
        _, records, strings = struct.unpack_from('>HHH', content, table)
        values = set()
        for i in range(records):
            platform, _, _, key, length, offset = struct.unpack_from('>6H', content, table+6+12*i)
            if key != name_id:
                continue
            start = table+strings+offset
            body = content[start:start+length]
            if len(body) != length:
                raise ValueError('truncated font name')
            values.add(body.decode('utf-16-be' if platform in (0, 3) else 'mac_roman'))
        return sorted(values)
    except (StopIteration, struct.error, UnicodeError, ValueError) as error:
        raise LegalCollectionError('Invalid SFNT font attribution metadata') from error

def make_font_notices(specifications, font_data, font_index, legal_paths):
    entries = {entry['name']:entry for entry in font_index['fonts']}
    if set(entries) != {spec['runtime_name'] for spec in specifications}:
        raise LegalCollectionError('Packed font set differs from reviewed legal/fonts.json')
    result = []
    lines = ['# Embedded font notices', '',
             'Original font bytes are losslessly stored in fonts.dat. These notices cover',
             'the three packed fonts only; no Ubuntu, Hack or emoji-icon-font is packed.', '']
    for specification in specifications:
        spec = dict(specification)
        entry = entries[spec['runtime_name']]
        data = font_data[entry['offset']:entry['offset']+entry['bytes']]
        if digest(data) != spec['sha256'] or digest(data) != entry['sha256']:
            raise LegalCollectionError(f'Font bytes differ from reviewed attribution: {spec["name"]}')
        copyrights = font_names(data, 0)
        if copyrights != [spec['copyright']] or spec['font_version'] not in font_names(data, 5):
            raise LegalCollectionError(f'Font copyright/version differs from reviewed attribution: {spec["name"]}')
        for path in spec['license_paths']:
            if path not in legal_paths:
                raise LegalCollectionError(f'{spec["name"]}: missing required font license {path}')
        spec.update(offset=entry['offset'], bytes=entry['bytes'],
                    copyright_records=copyrights, family_records=font_names(data, 1))
        result.append(spec)
        lines += [f'## {spec["name"]}', '', spec['copyright'], '',
                  f'Font version: {spec["font_version"]}.',
                  f'Component/source: {spec["component"]}/{spec["source_file"]}.',
                  f'Role: {spec["role"]}.',
                  f'License: {spec["license"]}; original text: '+', '.join(spec['license_paths'])+'.',
                  f'fonts.dat byte range: {entry["offset"]} + {entry["bytes"]}.',
                  f'SHA256: {spec["sha256"]}.', '']
    return result, ('\n'.join(lines)+'\n').encode('utf-8')
