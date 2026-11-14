#!/usr/bin/env python3
"""Build the data for the "Binary layout" docs page.

Each example is encoded by the real compiler (`gravenc from-json --raw`) from
site/layouts/layout.graven, then annotated by walking the bytes with the layout recorded
in the lockfile: every byte range gets a label, a colour group and an explanation.

Usage: site/layouts.py <gravenc> <out.json>
"""
import json
import os
import struct
import subprocess
import sys
import tempfile
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
SCHEMA = os.path.join(HERE, "layouts", "layout.graven")

# id → (root table, JSON document)
EXAMPLES = {
    "scalars": ("Reading", {"device": 7, "temp": 21.5, "unit": "Celsius", "online": True}),
    "strings": ("Note", {"title": "hi", "tag": "beef"}),
    "lists": ("Path", {"ids": [3, 5, 8], "points": [{"x": 1, "y": -2}, {"x": 40, "y": 7}]}),
    "tables": ("Catalog", {"products": [{"name": "pen", "price": 150}, {"name": "ink", "price": 900}]}),
    "strlist": ("Tags", {"tags": ["red", "blue"]}),
    "nested": ("Customer", {"name": "Ana", "home": {"city": "Rio", "zip": 20000}}),
    "union": ("Payment", {"amount": 500, "method": {"Pix": {"key": "a@b"}}}),
}

PRIM = {
    "u8": "<B", "u16": "<H", "u32": "<I", "u64": "<Q", "i8": "<b", "i16": "<h", "i32": "<i", "i64": "<q",
    "f32": "<f", "f64": "<d",
}


class Walker:
    def __init__(self, lock, data):
        self.t = {t["name"]: t for t in lock.get("tables", [])}
        self.s = {s["name"]: s for s in lock.get("structs", [])}
        self.e = {e["name"]: e for e in lock.get("enums", [])}
        self.u = {u["name"]: u for u in lock.get("unions", [])}
        self.b = data
        self.r = []

    def add(self, first, last, group, label, desc):
        self.r.append([first, last, group, label, desc])

    def u16(self, p):
        return struct.unpack_from("<H", self.b, p)[0]

    def u32(self, p):
        return struct.unpack_from("<I", self.b, p)[0]

    def i32(self, p):
        return struct.unpack_from("<i", self.b, p)[0]

    def size_of(self, ty):
        if ty in PRIM:
            return struct.calcsize(PRIM[ty])
        if ty == "bool":
            return 1
        if ty in self.e:
            return self.size_of(self.e[ty]["base"])
        return self.s[ty]["size"]

    def value(self, ty, p):
        """Decode an inline value (scalar, bool, enum, struct) as display text."""
        if ty in PRIM:
            v = struct.unpack_from(PRIM[ty], self.b, p)[0]
            return f"{v:g}" if ty[0] == "f" else str(v)
        if ty == "bool":
            return "true" if self.b[p] else "false"
        if ty in self.e:
            raw = int(self.value(self.e[ty]["base"], p))
            names = {v["value"]: v["name"] for v in self.e[ty]["values"]}
            return names.get(raw, f"unknown ({raw})")
        parts, q = [], p
        for f in self.s[ty]["fields"]:
            parts.append(f"{f['name']} = {self.value(f['ty'], q)}")
            q += self.size_of(f["ty"])
        return ", ".join(parts)

    def describe_scalar(self, ty):
        if ty in self.e:
            return f"enum {ty}, stored as its {self.e[ty]['base']}"
        if ty in self.s:
            return f"struct {ty}, {self.s[ty]['size']} bytes inline"
        return f"little-endian {ty}"

    def table(self, name, pos, label, inline_len=None):
        t = self.t[name]
        head = label or name
        if inline_len is None:
            flen = self.u16(pos)
            self.add(pos, pos + 1, "tbl", f"{head} length",
                     f"The fixed part of this {name} is {flen} bytes. A reader with a longer layout sees the fields past it as absent.")
            fs = pos + 2
        else:
            flen, fs = inline_len, pos
        fields = t["fields"]
        bit_bytes = sorted({f["bit"][0] for f in fields if "bit" in f and f["bit"][0] < flen})
        for bb in bit_bytes:
            on, off = [], []
            for f in fields:
                if "bit" in f and f["bit"][0] == bb:
                    byte, shift = f["bit"]
                    present = self.b[fs + byte] >> shift & 1
                    if f["ty"] == "bool" and present:
                        val = "true" if self.b[fs + byte] >> (shift + 1) & 1 else "false"
                        on.append(f"{f['name']} = {val}")
                    else:
                        (on if present else off).append(f["name"])
            desc = f"One bit per scalar field says whether it is present. Present: {', '.join(on) or 'none'}."
            if off:
                desc += f" Absent: {', '.join(off)}."
            if any(f["ty"] == "bool" for f in fields if f.get("bit", [None])[0] == bb):
                desc += " A bool uses two bits, presence and value, and has no slot of its own."
            self.add(fs + bb, fs + bb, "tbl", f"{head} presence bits", desc)
        for f in fields:
            if f.get("slot") is None or f["presence"] == "reserved":
                continue
            slot, size, ty = fs + f["slot"], f["size"], f["ty"]
            if f["slot"] + size > flen:
                continue
            path = f"{label}.{f['name']}" if label else f["name"]
            if "bit" in f:  # inline value
                byte, shift = f["bit"]
                if self.b[fs + byte] >> shift & 1:
                    self.add(slot, slot + size - 1, "val", path,
                             f"{self.value(ty, slot)}: {self.describe_scalar(ty)}, read in place. No alignment, no padding.")
                else:
                    self.add(slot, slot + size - 1, "env", f"{path} (absent)",
                             "The presence bit is clear, so these bytes are ignored. The writer leaves them zero.")
                continue
            if self.u.get(ty):
                tag = self.u16(slot)
                variants = {v["tag"]: v for v in self.u[ty]["variants"]}
                v = variants.get(tag)
                self.add(slot, slot + 1, "tbl", f"{path} tag",
                         f"Union tag {tag}: the variant is {v['name']}." if v else "Tag 0: no variant.")
                rel = self.i32(slot + 2)
                if rel == 0:
                    self.add(slot + 2, slot + 5, "env", f"{path} (absent)", "A zero pointer: nothing is stored.")
                    continue
                target = slot + 2 + rel
                self.add(slot + 2, slot + 5, "ptr", f"{path} pointer",
                         f"Relative pointer {rel:+d}, counted from this slot: the {v['table']} table starts at byte {target}.")
                self.table(v["table"], target, f"{path}.{v['name']}")
                continue
            rel = self.i32(slot)
            if rel == 0:
                self.add(slot, slot + 3, "env", f"{path} (absent)", "A zero pointer: the field is absent.")
                continue
            target = slot + rel
            self.add(slot, slot + 3, "ptr", f"{path} pointer",
                     f"Relative pointer {rel:+d}, counted from this slot: the data starts at byte {target}.")
            self.data(ty, target, path)

    def data(self, ty, p, path):
        if ty in ("string", "bytes"):
            n = self.u32(p)
            raw = self.b[p + 4:p + 4 + n]
            self.add(p, p + 3, "tbl", f"{path} length", f"{n} bytes follow.")
            if n:
                shown = f'"{raw.decode()}", UTF-8' if ty == "string" else raw.hex(" ")
                self.add(p + 4, p + 3 + n, "sum", f"{path} data",
                         f"{shown}. A reader borrows these bytes straight from the buffer, without copying.")
            return
        if ty.startswith("["):
            el = ty[1:-1]
            n = self.u32(p)
            if el in self.t:
                efl = self.u16(p + 4)
                self.add(p, p + 5, "tbl", f"{path} header",
                         f"A u32 count ({n}) and a u16 element length ({efl}). The {n} {el} elements follow back to back, "
                         f"each a fixed part of {efl} bytes without a header of its own, so element i is at a known offset.")
                for i in range(n):
                    self.table(el, p + 6 + i * efl, f"{path}[{i}]", inline_len=efl)
                return
            self.add(p, p + 3, "tbl", f"{path} count", f"{n} elements follow.")
            q = p + 4
            for i in range(n):
                if el in ("string", "bytes"):
                    rel = self.i32(q)
                    if rel == 0:
                        self.add(q, q + 3, "env", f"{path}[{i}] (empty)", "A zero pointer reads as an empty value.")
                    else:
                        self.add(q, q + 3, "ptr", f"{path}[{i}] pointer",
                                 f"Relative pointer {rel:+d}: element {i} starts at byte {q + rel}.")
                        self.data(el, q + rel, f"{path}[{i}]")
                    q += 4
                    continue
                size = self.size_of(el)
                self.add(q, q + size - 1, "val", f"{path}[{i}]",
                         f"{self.value(el, q)}: elements are stored inline, {size} bytes each.")
                q += size
            return
        if ty in self.t:
            self.table(ty, p, path)
            return
        raise ValueError(f"unhandled type {ty}")


def main(gravenc, out):
    with tempfile.TemporaryDirectory() as tmp:
        schema = os.path.join(tmp, "layout.graven")
        open(schema, "w", encoding="utf8").write(open(SCHEMA, encoding="utf8").read())
        subprocess.run([gravenc, "update", schema, "--init"], check=True, capture_output=True)
        lock = tomllib.load(open(schema + ".lock", "rb"))
        result = {}
        for name, (root, doc) in EXAMPLES.items():
            data = subprocess.run(
                [gravenc, "from-json", schema, "--root", root, "-", "-o", "-", "--raw"],
                input=json.dumps(doc).encode(), check=True, capture_output=True,
            ).stdout
            w = Walker(lock, data)
            w.table(root, 0, "")
            ranges = sorted(w.r)
            covered = set()
            for first, last, *_ in ranges:
                span = set(range(first, last + 1))
                if span & covered:
                    raise SystemExit(f"{name}: overlapping ranges at {sorted(span & covered)}")
                covered |= span
            missing = sorted(set(range(len(data))) - covered)
            if missing:
                raise SystemExit(f"{name}: bytes not explained: {missing}")
            for r in ranges:  # root-level labels have no leading dot
                r[3] = r[3].lstrip(".") or root
            result[name] = {"root": root, "json": doc, "hex": data.hex(" "), "ranges": ranges}
    os.makedirs(os.path.dirname(out), exist_ok=True)
    json.dump(result, open(out, "w", encoding="utf8"), indent=1)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
