"""Read resources from both historical ZIP and compact sequential bundles."""
import io, json, struct, zipfile

def embedded_resources(exe):
    matches = []
    position = 0
    while True:
        position = exe.find(b'PK\x05\x06', position)
        if position < 0: break
        try:
            _, disk, cd_disk, count_disk, count, size, start, comment = struct.unpack_from('<4s4H2IH', exe, position)
            beginning = position-size-start
            if disk == cd_disk == 0 and count_disk == count and beginning >= 0:
                candidate = exe[beginning:position+22+comment]
                with zipfile.ZipFile(io.BytesIO(candidate)) as archive:
                    names = archive.namelist()
                    if names == ['resources.bin']:
                        assert archive.testzip() is None
                        payload = archive.read('resources.bin')
                        assert payload[:8] == b'HHRES001'
                        length = int.from_bytes(payload[8:12], 'little')
                        index = json.loads(payload[12:12+length])
                        offset = 12+length
                        files = {}
                        for name in sorted(index):
                            end = offset+index[name]['bytes']
                            files[name] = payload[offset:end]
                            assert len(files[name]) == index[name]['bytes']
                            offset = end
                        assert offset == len(payload)
                        matches.append((candidate, files))
                    elif 'checksums.json' in names and 'payload/release-manifest.json' in names:
                        assert archive.testzip() is None
                        matches.append((candidate, {n:archive.read(n) for n in names}))
        except (ValueError, struct.error, zipfile.BadZipFile):
            pass
        position += 4
    assert len(matches) == 1, f'Expected one resource bundle, found {len(matches)}'
    return matches[0]
