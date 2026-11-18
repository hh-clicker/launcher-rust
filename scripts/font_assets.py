"""The three user-approved GUI fonts with their original bytes/tweaks."""
import hashlib
from build_inputs import crate_source
FONT_PACKAGE = 'epaint_default_fonts-0.31.1'

def font_assets(root):
    builtins = crate_source('epaint_default_fonts', '0.31.1')/'fonts'
    inputs = [
        ('NotoEmoji-Regular', builtins/'NotoEmoji-Regular.ttf', 0.81),
        ('Inter', root/'assets/inter_regular.ttf', 1.0),
        ('Inter Bold', root/'assets/inter_semibold.ttf', 1.0),
    ]
    data = bytearray()
    entries = []
    for name, path, scale in inputs:
        content = path.read_bytes()
        entries.append(dict(name=name, offset=len(data), bytes=len(content), sha256=hashlib.sha256(content).hexdigest(), scale=scale))
        data.extend(content)
    return bytes(data), dict(bytes=len(data), sha256=hashlib.sha256(data).hexdigest(), fonts=entries)
