#!/usr/bin/env python3
"""Restore Studio comments after Cargo rewrites its generated lockfile; never change its graph."""
from pathlib import Path
import os
import stat
import tempfile
import tomllib

path = Path(__file__).resolve().parents[1] / "Cargo.lock"
if path.is_symlink():
    raise SystemExit("Refusing a symlinked Cargo.lock")
original = path.read_text()
before = tomllib.loads(original)
text = original
if ("# STUDIO" + "-BEGIN mcp-lock-dependency") not in text:
    anchor = '[[package]]' + chr(10) + 'name = "studio-mcp"'
    start = text.index(anchor)
    end = text.index(chr(10) + '[[package]]', start + len(anchor))
    text = (text[:start] + ('# STUDIO' + '-BEGIN mcp-lock-dependency: generated local MCP crate dependency entry') + chr(10)
            + text[start:end].rstrip() + chr(10) + ('# STUDIO' + '-END mcp-lock-dependency') + chr(10) + text[end:])
def mark(text, package, dependencies):
    anchor = '[[package]]' + chr(10) + 'name = "' + package + '"'
    start = text.index(anchor)
    end = text.index(chr(10) + '[[package]]', start + len(anchor))
    section = text[start:end]
    for dependency, reason in dependencies:
        section = section.replace(' "' + dependency + '",' + chr(10), ' "' + dependency + ('", # STUDIO' + ': ') + reason + chr(10))
    return text[:start] + section + text[end:]
text = mark(text, 'vixen-ffi', [('serde_json', 'Studio command JSON boundary'), ('studio-mcp', 'in-app local MCP transport')])
video = 'AVFoundation writer for preview_video'
text = mark(text, 'vixen-session', [(name, video) for name in ['block2', 'objc2', 'objc2-av-foundation', 'objc2-core-foundation',
                                                                  'objc2-core-media', 'objc2-core-video', 'objc2-foundation']])
text = mark(text, 'objc2-av-foundation', [('block2', video), ('objc2-core-video', video)])
if tomllib.loads(text) != before:
    raise SystemExit("Marker restoration would change dependency data")
if text != original:
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, prefix=".studio-lock-", delete=False) as output:
            temporary = Path(output.name)
            output.write(text)
            output.flush()
            os.fsync(output.fileno())
        temporary.chmod(stat.S_IMODE(path.stat().st_mode))
        os.replace(temporary, path)
    finally:
        if temporary is not None and temporary.exists():
            temporary.unlink()
print("Studio lockfile markers preserved; dependency graph unchanged (" + str(len(before['package'])) + " packages)")
