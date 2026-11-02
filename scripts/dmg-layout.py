#!/usr/bin/env python3
"""Writes the Finder layout (.DS_Store) of a mounted, writable DMG volume, without Finder.

It sets the window size, hides the toolbar and sidebar, sets icon view at a fixed icon size,
places each named item, and points the background at an image inside the volume (an alias
record, as Finder writes it). Standard library only.

Usage: dmg-layout.py <mount point> --volume-name NAME --background REL_PATH
                     --window WxH (content size) --icon-size N --item NAME=X,Y [--item ...]

REL_PATH is relative to the volume root (for example .background/background.tiff). X,Y are icon
centers in the window's content area, in points. The alias records the final mount point as
/Volumes/<NAME>; Finder also resolves it by volume name and file id when it mounts elsewhere.
"""

import argparse
import datetime
import os
import plistlib
import struct
import sys

HFS_EPOCH = datetime.datetime(1904, 1, 1, tzinfo=datetime.timezone.utc)


def hfs_time(ts):
    t = datetime.datetime.fromtimestamp(ts, tz=datetime.timezone.utc)
    return int((t - HFS_EPOCH).total_seconds()) & 0xFFFFFFFF


def pascal(s, size):
    b = s.encode("mac_roman", "replace")[: size - 1]
    return bytes([len(b)]) + b + b"\0" * (size - 1 - len(b))


def utf16_counted(s):
    b = s.encode("utf-16-be")
    return struct.pack(">H", len(b) // 2) + b


def alias_record(mount, volume_name, rel_path):
    """An Alias Manager version 2 record for <mount>/<rel_path>."""
    path = os.path.join(mount, rel_path)
    parent = os.path.dirname(path)
    name = os.path.basename(path)
    folder_name = os.path.basename(parent)
    st, pst, vst = os.stat(path), os.stat(parent), os.stat(mount)
    vol_created = hfs_time(getattr(vst, "st_birthtime", vst.st_mtime))
    file_created = hfs_time(getattr(st, "st_birthtime", st.st_mtime))

    body = struct.pack(
        ">h28sI2shI64sII4s4s",
        0,  # kind: file
        pascal(volume_name, 28),
        vol_created,
        b"H+",
        5,  # disk type: ejectable
        pst.st_ino,  # parent folder CNID (st_ino is the CNID on HFS+)
        pascal(name, 64),
        st.st_ino,
        file_created,
        b"\0\0\0\0",
        b"\0\0\0\0",
    )
    body += struct.pack(">hhI2s10s", -1, -1, 0, b"\0\0", b"\0" * 10)

    rel = "/" + rel_path.strip("/")
    tags = [
        (0, folder_name.encode("utf-8")),
        (1, struct.pack(">I", pst.st_ino)),
        (2, (volume_name + ":" + rel.strip("/").replace("/", ":")).encode("utf-8")),
        (14, utf16_counted(name)),
        (15, utf16_counted(volume_name)),
        (18, rel.encode("utf-8")),
        (19, ("/Volumes/" + volume_name).encode("utf-8")),
    ]
    for tag, data in tags:
        body += struct.pack(">hh", tag, len(data)) + data
        if len(data) % 2:
            body += b"\0"
    body += struct.pack(">hh", -1, 0)
    size = 8 + len(body)
    return struct.pack(">4sHH", b"\0\0\0\0", size, 2) + body


def record(name, code, typ, value):
    out = struct.pack(">I", len(name)) + name.encode("utf-16-be") + code + typ
    if typ == b"blob":
        return out + struct.pack(">I", len(value)) + value
    if typ == b"long":
        return out + struct.pack(">I", value)
    if typ == b"type":
        return out + value
    raise ValueError(typ)


def ds_store(records):
    """A .DS_Store with one leaf node. records: list of (name, code, type, value)."""
    records = sorted(records, key=lambda r: (r[0].lower(), r[1]))
    leaf = struct.pack(">II", 0, len(records)) + b"".join(record(*r) for r in records)
    if len(leaf) > 4096:
        raise ValueError("too many records for one node")
    # Buddy allocator layout (offsets after the 4-byte file prefix):
    #   0..32 header, 32..64 DSDB, 2048..4096 root block, 4096..8192 leaf node.
    dsdb_addr, root_addr, leaf_addr = 32 | 5, 2048 | 11, 4096 | 12
    dsdb = struct.pack(">IIIII", 2, 0, len(records), 1, 0x1000)
    addrs = [root_addr, dsdb_addr, leaf_addr]
    root = struct.pack(">II", len(addrs), 0)
    root += b"".join(struct.pack(">I", a) for a in addrs) + b"\0" * 4 * (256 - len(addrs))
    root += struct.pack(">I", 1) + bytes([4]) + b"DSDB" + struct.pack(">I", 1)
    free = {6: [64], 7: [128], 8: [256], 9: [512], 10: [1024]}
    for i in range(32):
        offs = free.get(i, [])
        root += struct.pack(">I", len(offs)) + b"".join(struct.pack(">I", o) for o in offs)
    assert len(root) <= 2048

    buf = bytearray(8192)
    buf[0:32] = struct.pack(">4sIII16s", b"Bud1", 2048, 2048, 2048,
                            bytes.fromhex("0000100c000000870000200b00000000"))
    buf[32:32 + len(dsdb)] = dsdb
    buf[2048:2048 + len(root)] = root
    buf[4096:4096 + len(leaf)] = leaf
    return struct.pack(">I", 1) + bytes(buf)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("mount")
    p.add_argument("--volume-name", required=True)
    p.add_argument("--background", required=True)
    p.add_argument("--window", required=True)
    p.add_argument("--icon-size", type=int, default=128)
    p.add_argument("--text-size", type=int, default=13)
    p.add_argument("--title-bar", type=int, default=32)
    p.add_argument("--item", action="append", default=[])
    a = p.parse_args()

    w, h = (int(v) for v in a.window.lower().split("x"))
    bwsp = {
        "ContainerShowSidebar": False,
        "ShowPathbar": False,
        "ShowSidebar": False,
        "ShowStatusBar": False,
        "ShowTabView": False,
        "ShowToolbar": False,
        "SidebarWidth": 0,
        # WindowBounds is the whole frame; the title bar (32 pt on macOS 26) comes out of it.
        "WindowBounds": "{{200, 160}, {%d, %d}}" % (w, h + a.title_bar),
    }
    icvp = {
        "arrangeBy": "none",
        "backgroundColorBlue": 1.0,
        "backgroundColorGreen": 1.0,
        "backgroundColorRed": 1.0,
        "backgroundImageAlias": alias_record(a.mount, a.volume_name, a.background),
        "backgroundType": 2,
        "gridOffsetX": 0.0,
        "gridOffsetY": 0.0,
        "gridSpacing": 100.0,
        "iconSize": float(a.icon_size),
        "labelOnBottom": True,
        "showIconPreview": True,
        "showItemInfo": False,
        "textSize": float(a.text_size),
        "viewOptionsVersion": 1,
    }
    records = [
        (".", b"bwsp", b"blob", plistlib.dumps(bwsp, fmt=plistlib.FMT_BINARY)),
        (".", b"icvp", b"blob", plistlib.dumps(icvp, fmt=plistlib.FMT_BINARY)),
        (".", b"vSrn", b"long", 1),
        (".", b"vstl", b"type", b"icnv"),
    ]
    for item in a.item:
        name, _, pos = item.rpartition("=")
        x, y = (int(v) for v in pos.split(","))
        if not os.path.lexists(os.path.join(a.mount, name)):
            sys.exit(f"dmg-layout: no item {name!r} in {a.mount}")
        records.append((name, b"Iloc", b"blob", struct.pack(">II", x, y) + b"\xff" * 6 + b"\0\0"))

    with open(os.path.join(a.mount, ".DS_Store"), "wb") as f:
        f.write(ds_store(records))


if __name__ == "__main__":
    main()
