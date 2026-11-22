#!/usr/bin/env python3
"""Adds one Vixen Studio release to the Sparkle appcast (B49), keeping every earlier item.

Studio's release script runs it after each release is published. appcast.xml lives alone on the repo's `appcast` branch.

    scripts/studio_appcast.py appcast.xml --title T --version BUILD --short-version S --url ZIP_URL --length BYTES \\
        --signature EDDSA --notes-url RELEASE_URL --notes-file release-notes.md --min-system 26.0
"""
import argparse
import email.utils
import html
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

SPARKLE = "http://www.andymatuschak.org/xml-namespaces/sparkle"
DC = "http://purl.org/dc/elements/1.1/"
ET.register_namespace("sparkle", SPARKLE)
ET.register_namespace("dc", DC)


def s(tag):
    return f"{{{SPARKLE}}}{tag}"


def changes_html(notes: str) -> str:
    """The release notes' Changes list as HTML, for Sparkle's update window."""
    items, inside = [], False
    for line in notes.splitlines():
        if line.startswith("### "):
            inside = line.strip() == "### Changes"
        elif inside and line.startswith("- "):
            items.append(f"<li>{html.escape(line[2:].strip())}</li>")
    body = "<ul>" + "".join(items) + "</ul>" if items else "<p>Fixes and improvements.</p>"
    return "<h3>Changes</h3>" + body


def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument("appcast", type=Path)
    for name in ("title", "version", "short-version", "url", "length", "signature", "notes-url", "notes-file", "min-system"):
        p.add_argument("--" + name, required=True)
    a = p.parse_args(argv)
    if not a.version.isdigit() or not a.length.isdigit():
        sys.exit("version and length must be whole numbers")
    if not re.fullmatch(r"[A-Za-z0-9+/]+={0,2}", a.signature):
        sys.exit("signature must be base64")
    if not a.url.startswith("https://") or not a.notes_url.startswith("https://"):
        sys.exit("URLs must be https")

    if a.appcast.exists():
        tree = ET.parse(a.appcast)
        channel = tree.getroot().find("channel")
    else:
        root = ET.Element("rss", {"version": "2.0"})
        channel = ET.SubElement(root, "channel")
        ET.SubElement(channel, "title").text = "Vixen Studio"
        ET.SubElement(channel, "link").text = "https://github.com/bguliano/vixen-macos-studio/releases"
        ET.SubElement(channel, "description").text = "Vixen Studio updates (unofficial alphas)"
        ET.SubElement(channel, "language").text = "en"
        tree = ET.ElementTree(root)
    existing = [int(v.text) for v in channel.iter(s("version")) if (v.text or "").isdigit()]
    if existing and int(a.version) <= max(existing):
        sys.exit(f"build {a.version} isn't newer than the appcast's newest, {max(existing)}")

    item = ET.Element("item")
    ET.SubElement(item, "title").text = a.title
    ET.SubElement(item, "pubDate").text = email.utils.formatdate(usegmt=True)
    ET.SubElement(item, s("version")).text = a.version
    ET.SubElement(item, s("shortVersionString")).text = a.short_version
    ET.SubElement(item, s("minimumSystemVersion")).text = a.min_system
    ET.SubElement(item, s("fullReleaseNotesLink")).text = a.notes_url
    ET.SubElement(item, "description").text = changes_html(Path(a.notes_file).read_text())
    ET.SubElement(item, "enclosure", {
        "url": a.url,
        s("edSignature"): a.signature,
        "length": a.length,
        "type": "application/octet-stream",
    })
    items = channel.findall("item")
    index = list(channel).index(items[0]) if items else len(channel)
    channel.insert(index, item)
    ET.indent(tree, "  ")
    tree.write(a.appcast, encoding="utf-8", xml_declaration=True)


if __name__ == "__main__":
    main()
