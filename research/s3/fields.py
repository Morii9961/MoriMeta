"""S3: field mapping, IPTC charset/length, time fields, sidecar preservation, MakerNotes serials,
JUMBF detection, and what the Windows Property System (Explorer/Photos) displays.

Run research/s3/corpus.py first.
Usage:  python research/s3/fields.py
Writes: research/results/s3/fields.json (+ console summary)
"""

from __future__ import annotations

import json
import re
import shutil
import struct
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import etlib as E  # noqa: E402

CORPUS = E.WORK / "corpus" / "jpeg"
NEFS = E.WORK / "corpus" / "nikon"
OUT = E.RESEARCH / "results" / "s3"
VOLATILE_GROUPS = ("System:", "File:", "ExifTool:ExifToolVersion", "SourceFile")

CREATOR = "森 Morii"
COPYRIGHT = "© Morii 2026"


def xml_value(v: str) -> str:
    """Same rules as spikes/exiftool-session/src/encode.rs::xml_value (for -ex commands)."""
    out = []
    for i, c in enumerate(v):
        if c == "\0" or (ord(c) < 0x20 and c not in "\t\n\r") or c in "￾￿":
            raise ValueError(f"unrepresentable {c!r}")
        out.append({"&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
                    "\t": "&#9;", "\n": "&#10;", "\r": "&#13;"}.get(c, "&#32;" if (c == " " and i == 0) else c))
    return "".join(out)


def assign(tag: str, value: str) -> str:
    return f"-{tag}={xml_value(value)}"


def read_all(path: Path, numeric: bool = True) -> dict:
    lines = ["-json", "-a", "-G1", "-u", "-struct", "-api", "StructFormat=JSONQ"] + (["-n"] if numeric else []) + [E.p(path)]
    r = E.run(lines)
    d = r.json()[0]
    return {k: v for k, v in d.items() if not k.startswith(VOLATILE_GROUPS)}


def diff(a: dict, b: dict) -> dict:
    keys = set(a) | set(b)
    return {
        "added": sorted(k for k in keys if k not in a),
        "removed": sorted(k for k in keys if k not in b),
        "changed": sorted(k for k in keys if k in a and k in b and a[k] != b[k]),
    }


def shell_props(paths: list[Path]) -> dict:
    lst = OUT / "_shell_list.txt"
    res = OUT / "_shell_out.json"
    lst.write_text("\n".join(str(Path(p).resolve()) for p in paths), encoding="utf-8")
    ps = subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
                         str(Path(__file__).with_name("shellprops.ps1")), str(lst), str(res)], capture_output=True)
    lst.unlink()
    try:
        rows = json.loads(res.read_text(encoding="utf-8-sig"))
        res.unlink()
    except Exception:
        return {"error": ps.stderr.decode(errors="replace")}
    return {Path(r["path"]).name: {k: v for k, v in r.items() if k != "path"} for r in rows}


def cp1252_ok(s: str) -> bool:
    try:
        s.encode("cp1252")
        return True
    except UnicodeEncodeError:
        return False


# ---------------------------------------------------------------- A: explicit vs MWG
def exp_mapping(lab: Path) -> dict:
    res = {}
    outs = []
    for base in ["writer.jpg", "xmp_only.jpg", "iptc_latin.jpg", "iptc_utf8.jpg", "z8.jpg", "nikon_d70.jpg"]:
        src = CORPUS / base
        before = read_all(src, numeric=False)
        has_iptc = any(k.startswith("IPTC") for k in before)
        cs_utf8 = str(before.get("IPTC:CodedCharacterSet", "")).upper() == "UTF8"
        has_digest = "Photoshop:IPTCDigest" in before
        lines = ["-ex", assign("IFD0:Artist", CREATOR), assign("IFD0:Copyright", COPYRIGHT),
                 assign("XMP-dc:Creator", CREATOR), assign("XMP-dc:Rights-x-default", COPYRIGHT)]
        iptc_action = "none (no IPTC in file)"
        if has_iptc:
            if cs_utf8 or (cp1252_ok(CREATOR) and cp1252_ok(COPYRIGHT)):
                iptc_action = "update in place"
            else:
                # FAQ #10: rewrite all IPTC as UTF-8, then assign
                # (fields being assigned are excluded from the copy, otherwise list tags accumulate, see C)
                lines += ["-tagsFromFile", "@", "-IPTC:all", "--IPTC:By-line", "--IPTC:CopyrightNotice",
                          "-IPTC:CodedCharacterSet=UTF8"]
                iptc_action = "convert IPTC to UTF-8, then update"
            lines += [assign("IPTC:By-line", CREATOR), assign("IPTC:CopyrightNotice", COPYRIGHT)]
        if has_digest:
            lines.append("-Photoshop:IPTCDigest=new")
        ob = lab / f"A-explicit-{base}"
        rb = E.run(lines + ["-o", E.p(ob), E.p(src)])
        om = lab / f"A-mwg-{base}"
        rm = E.run(["-ex", assign("MWG:Creator", CREATOR), assign("MWG:Copyright", COPYRIGHT), "-o", E.p(om), E.p(src)])
        row = {"iptc_action_explicit": iptc_action, "had_digest": has_digest}
        for name, o, r in [("explicit", ob, rb), ("mwg", om, rm)]:
            if not o.exists():
                row[name] = {"written": False, "stderr": r.err.strip()}
                continue
            after = read_all(o, numeric=False)
            d = diff(before, after)
            row[name] = {
                "written": True, "stderr": r.err.strip(), "read_warning": after.get("ExifTool:Warning"),
                "changed_or_added": {k: after.get(k) for k in d["added"] + d["changed"] if not k.startswith("Composite")},
                "removed": d["removed"],
            }
            outs.append(o)
        if ob.exists() and om.exists():
            a, m = read_all(ob), read_all(om)
            dd = diff(a, m)
            row["explicit_vs_mwg"] = {k: {"explicit": a.get(k), "mwg": m.get(k)} for k in dd["added"] + dd["removed"] + dd["changed"]
                                      if not k.startswith("Composite")}
        res[base] = row
    res["_windows_shell"] = shell_props(outs)
    return res


# ---------------------------------------------------------------- B: IPTC byte limits
def exp_iptc_limits(lab: Path) -> dict:
    res = {"exiftool_formats": {}}
    iptc_pm = (E.PKG / "exiftool_files" / "lib" / "Image" / "ExifTool" / "IPTC.pm").read_text(encoding="latin-1")
    for name in ["By-line", "CopyrightNotice", "ObjectName", "Caption-Abstract", "City", "Keywords"]:
        m = re.search(r"Name => '" + re.escape(name) + r"',.*?Format => '([^']+)'", iptc_pm, re.S)
        res["exiftool_formats"][name] = m.group(1) if m else None
    cases = [("iptc_utf8.jpg", "By-line", "森" * 10), ("iptc_utf8.jpg", "By-line", "森" * 11),
             ("iptc_latin.jpg", "By-line", "A" * 32), ("iptc_latin.jpg", "By-line", "A" * 33),
             ("iptc_latin.jpg", "By-line", "é" * 32), ("iptc_utf8.jpg", "By-line", "é" * 17),
             ("iptc_utf8.jpg", "CopyrightNotice", "©" * 64), ("iptc_utf8.jpg", "CopyrightNotice", "©" * 65)]
    rows = []
    for i, (base, tag, val) in enumerate(cases):
        o = lab / f"B-{i}-{base}"
        r = E.run(["-ex", assign(f"IPTC:{tag}", val), "-o", E.p(o), E.p(CORPUS / base)])
        stored = E.read_tags(o, [f"IPTC:{tag}"]).get(f"IPTC:{tag}") if o.exists() else None
        enc = "utf-8" if "utf8" in base else "cp1252"
        rows.append({"file": base, "tag": tag, "chars": len(val), "encoded_bytes": len(val.encode(enc)),
                     "stderr": r.err.strip(), "stored_chars": len(stored) if isinstance(stored, str) else None,
                     "stored_exact": stored == val, "stored_is_valid_prefix": isinstance(stored, str) and val.startswith(stored)})
    res["cases"] = rows
    return res


# ---------------------------------------------------------------- C: Latin IPTC -> UTF-8 conversion
def exp_iptc_convert(lab: Path) -> dict:
    src = CORPUS / "iptc_latin.jpg"
    o = lab / "C-converted.jpg"
    r = E.run(["-tagsFromFile", "@", "-IPTC:all", "-IPTC:CodedCharacterSet=UTF8", "-Photoshop:IPTCDigest=new", "-o", E.p(o), E.p(src)])
    b, a = read_all(src, numeric=False), read_all(o, numeric=False)
    iptc_b = {k: v for k, v in b.items() if k.startswith("IPTC:")}
    iptc_a = {k: v for k, v in a.items() if k.startswith("IPTC:") and k != "IPTC:CodedCharacterSet"}
    # naive "convert + assign in one command": does the list tag get replaced or appended?
    n = lab / "C-naive.jpg"
    E.run(["-ex", "-tagsFromFile", "@", "-IPTC:all", "-IPTC:CodedCharacterSet=UTF8", assign("IPTC:By-line", CREATOR),
           "-o", E.p(n), E.p(src)])
    x = lab / "C-excluded.jpg"
    E.run(["-ex", "-tagsFromFile", "@", "-IPTC:all", "--IPTC:By-line", "-IPTC:CodedCharacterSet=UTF8",
           assign("IPTC:By-line", CREATOR), "-o", E.p(x), E.p(src)])
    return {"stderr": r.err.strip(), "coded_character_set_after": a.get("IPTC:CodedCharacterSet"),
            "iptc_values_identical_except_added_envelope": iptc_b == {k: v for k, v in iptc_a.items() if k != "IPTC:EnvelopeRecordVersion"},
            "before": iptc_b, "after": iptc_a, "digest_warning_after": a.get("ExifTool:Warning"),
            "naive_convert_and_assign_byline": E.read_tags(n, ["IPTC:By-line"]).get("IPTC:By-line"),
            "excluded_convert_and_assign_byline": E.read_tags(x, ["IPTC:By-line"]).get("IPTC:By-line")}


# ---------------------------------------------------------------- E: time fields
TIME_RE = re.compile(r"(Date|Time|Offset|SubSec)", re.I)


def exp_time(lab: Path) -> dict:
    inv = {}
    for f in list(CORPUS.glob("*.jpg")) + list(NEFS.glob("*.NEF")):
        t = read_all(f, numeric=False)
        inv[f.name] = sorted(k for k in t if TIME_RE.search(k.split(":", 1)[1]) and not k.startswith(("Composite", "System")))
    # Absolute on z8.jpg: all existing capture/digitized locations updated, subsec removed, offset kept
    src = CORPUS / "z8.jpg"
    before = read_all(src, numeric=False)
    lines = ["-ExifIFD:DateTimeOriginal=2026:09:04 12:27:00", "-ExifIFD:SubSecTimeOriginal=",
             "-ExifIFD:CreateDate=2026:09:04 12:27:00", "-ExifIFD:SubSecTimeDigitized=",
             "-XMP-xmp:CreateDate=2026:09:04 12:27:00+02:00"]
    if "IFD0:DateTimeOriginal" in before:
        lines.append("-IFD0:DateTimeOriginal=2026:09:04 12:27:00")
    o = lab / "E-absolute-z8.jpg"
    r = E.run(lines + ["-o", E.p(o), E.p(src)])
    after = read_all(o, numeric=False)
    d = diff(before, after)
    tim = lambda t: {k: v for k, v in t.items() if TIME_RE.search(k.split(":", 1)[1])}
    return {
        "inventory": inv,
        "absolute_z8": {"stderr": r.err.strip(), "diff": d,
                        "time_tags_after": tim(after), "time_tags_before": tim(before)},
        "_windows_shell": shell_props([src, o]),
    }


# ---------------------------------------------------------------- F: sidecars
LR_LIKE = """<?xpacket begin="﻿" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core 7.0-c000 (synthetic test)">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:tiff="http://ns.adobe.com/tiff/1.0/"
    xmlns:exif="http://ns.adobe.com/exif/1.0/"
    xmlns:aux="http://ns.adobe.com/exif/1.0/aux/"
    xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/"
    xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/"
    xmlns:stEvt="http://ns.adobe.com/xap/1.0/sType/ResourceEvent#"
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    xmlns:lr="http://ns.adobe.com/lightroom/1.0/"
    xmlns:mmtest="http://example.com/morimeta-test/1.0/"
   xmp:Rating="3"
   xmp:CreateDate="2023-06-02T18:53:25.67+02:00"
   xmp:MetadataDate="2026-01-01T10:00:00+08:00"
   tiff:Make="NIKON CORPORATION"
   exif:DateTimeOriginal="2023-06-02T18:53:25.67+02:00"
   aux:SerialNumber="6004967"
   aux:LensSerialNumber="20002661"
   photoshop:SidecarForExtension="NEF"
   photoshop:DateCreated="2023-06-02T18:53:25.67+02:00"
   xmpMM:DocumentID="xmp.did:0123456789abcdef"
   xmpMM:OriginalDocumentID="0123456789ABCDEF0123456789ABCDEF"
   xmpMM:InstanceID="xmp.iid:fedcba9876543210"
   crs:Version="17.0"
   crs:ProcessVersion="15.4"
   crs:WhiteBalance="As Shot"
   crs:Exposure2012="+0.35"
   crs:HasSettings="True"
   mmtest:Custom="unknown-namespace value"
   mmtest:Number="42">
   <dc:creator><rdf:Seq><rdf:li>Old Creator</rdf:li></rdf:Seq></dc:creator>
   <dc:title><rdf:Alt><rdf:li xml:lang="x-default">Title</rdf:li><rdf:li xml:lang="de">Titel</rdf:li></rdf:Alt></dc:title>
   <dc:subject><rdf:Bag><rdf:li>travel</rdf:li><rdf:li>Hokkaido</rdf:li></rdf:Bag></dc:subject>
   <lr:hierarchicalSubject><rdf:Bag><rdf:li>Places|Japan|Hokkaido</rdf:li></rdf:Bag></lr:hierarchicalSubject>
   <crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012>
   <crs:Look><rdf:Description crs:Name="Adobe Color" crs:Amount="1"><crs:Parameters><rdf:Description crs:Version="15.0" crs:ProcessVersion="11.0" crs:ToneCurvePV2012="0, 0"/></crs:Parameters></rdf:Description></crs:Look>
   <xmpMM:History><rdf:Seq><rdf:li stEvt:action="saved" stEvt:instanceID="xmp.iid:fedcba9876543210" stEvt:when="2026-01-01T10:00:00+08:00" stEvt:softwareAgent="Adobe Photoshop Lightroom Classic 15.0 (Windows)" stEvt:changed="/metadata"/></rdf:Seq></xmpMM:History>
   <mmtest:Struct><rdf:Description mmtest:A="1" mmtest:B="two"/></mmtest:Struct>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>
"""


def exp_sidecar(lab: Path) -> dict:
    res = {}
    # F1 minimal sidecar from scratch
    f1 = lab / "DSC_0001.xmp"
    r = E.run(["-ex", assign("XMP-dc:Creator", CREATOR), assign("XMP-dc:Rights-x-default", COPYRIGHT),
               "-XMP-exif:DateTimeOriginal=2026:09:04 12:27:00+02:00", "-o", E.p(f1)])
    res["F1_minimal_create"] = {"stderr": r.err.strip(), "xml": f1.read_text(encoding="utf-8") if f1.exists() else None,
                                "tags": read_all(f1) if f1.exists() else None}

    def update(name: str, xml: str) -> dict:
        src = lab / f"{name}.xmp"
        src.write_text(xml, encoding="utf-8")
        out = lab / f"{name}.out.xmp"
        r = E.run(["-ex", assign("XMP-dc:Creator", CREATOR), "-o", E.p(out), E.p(src)])
        row = {"exit": r.code, "stderr": r.err.strip(), "written": out.exists()}
        if out.exists():
            b, a = read_all(src), read_all(out)
            d = diff(b, a)
            row["diff_excluding_creator"] = {k: v for k, v in
                                             {kk: [x for x in vv if not x.startswith("XMP-dc:Creator") and x != "XMP-x:XMPToolkit"] for kk, vv in d.items()}.items()}
            row["creator_after"] = a.get("XMP-dc:Creator")
            txt = out.read_text(encoding="utf-8")
            row["comment_kept"] = "<!--" in txt
            row["out_xml_bytes"] = len(txt.encode())
        return row

    res["F2_update_lr_like"] = update("F2", LR_LIKE)
    res["F3a_comment"] = update("F3a", LR_LIKE.replace("<dc:creator>", "<!-- note by another tool -->\n   <dc:creator>"))
    res["F3b_second_description_other_about"] = update(
        "F3b", LR_LIKE.replace("  </rdf:Description>\n </rdf:RDF>",
                               "  </rdf:Description>\n  <rdf:Description rdf:about=\"uuid:other\" xmlns:mmtest=\"http://example.com/morimeta-test/1.0/\" mmtest:Other=\"x\"/>\n </rdf:RDF>"))
    res["F3c_parsetype_literal"] = update(
        "F3c", LR_LIKE.replace("<mmtest:Struct>", "<mmtest:Lit rdf:parseType=\"Literal\"><b xmlns=\"http://www.w3.org/1999/xhtml\">bold</b> text</mmtest:Lit>\n   <mmtest:Struct>"))
    res["F3d_second_description_same_about"] = update(
        "F3d", LR_LIKE.replace("  </rdf:Description>\n </rdf:RDF>",
                               "  </rdf:Description>\n  <rdf:Description rdf:about=\"\" xmlns:mmtest=\"http://example.com/morimeta-test/1.0/\" mmtest:Second=\"y\"/>\n </rdf:RDF>"))
    return res


# ---------------------------------------------------------------- G: MakerNotes serials
def exp_makernotes(lab: Path) -> dict:
    res = {}
    for base in ["nikon_d2hs.jpg", "nikon_d70.jpg", "z8_mn_multiseg.jpg"]:
        src = CORPUS / base
        before = read_all(src, numeric=False)
        row = {"serial_tags_before": {k: v for k, v in before.items() if "Serial" in k}}
        for label, lines in [("delete_makernote_serial", ["-MakerNotes:SerialNumber="]),
                             ("set_makernote_serial_0", ["-MakerNotes:SerialNumber=0"]),
                             ("delete_all_makernotes", ["-MakerNotes:all="])]:
            o = lab / f"G-{label}-{base}"
            r = E.run(lines + ["-o", E.p(o), E.p(src)])
            if not o.exists():
                row[label] = {"written": False, "exit": r.code, "stderr": r.err.strip()}
                continue
            after = read_all(o, numeric=False)
            d = diff(before, after)
            mn_changed = [k for k in d["changed"] if not k.startswith(("Composite", "ExifTool"))]
            row[label] = {"written": True, "exit": r.code, "stderr": r.err.strip(), "warning": after.get("ExifTool:Warning"),
                          "serials_after": {k: v for k, v in after.items() if "Serial" in k},
                          "changed_tags": mn_changed[:40], "removed_count": len(d["removed"]),
                          "lens_id": [before.get("Composite:LensID"), after.get("Composite:LensID")]}
        res[base] = row
    # standard EXIF serials on z8.jpg
    src = CORPUS / "z8.jpg"
    o = lab / "G-exif-serials-z8.jpg"
    r = E.run(["-ExifIFD:SerialNumber=", "-ExifIFD:LensSerialNumber=", "-o", E.p(o), E.p(src)])
    res["z8.jpg exif serial delete"] = {"exit": r.code, "stderr": r.err.strip(),
                                        "serials_after": {k: v for k, v in read_all(o, numeric=False).items() if "Serial" in k} if o.exists() else None}
    return res


# ---------------------------------------------------------------- H: JUMBF (C2PA-like) detection
def jumbf_app11() -> bytes:
    c2pa_uuid = bytes.fromhex("6332706100110010800000AA00389B71")
    label = b"c2pa\x00"
    jumd = struct.pack(">I", 8 + 16 + 1 + len(label)) + b"jumd" + c2pa_uuid + b"\x03" + label
    payload_json = b'{"test":"morimeta synthetic, not a real manifest"}'
    jsonbox = struct.pack(">I", 8 + len(payload_json)) + b"json" + payload_json
    jumb = struct.pack(">I", 8 + len(jumd) + len(jsonbox)) + b"jumb" + jumd + jsonbox
    data = b"JP" + struct.pack(">H", 1) + struct.pack(">I", 1) + jumb
    return b"\xff\xeb" + struct.pack(">H", len(data) + 2) + data


def exp_jumbf(lab: Path) -> dict:
    src_bytes = (CORPUS / "writer.jpg").read_bytes()
    j = lab / "H-jumbf.jpg"
    j.write_bytes(src_bytes[:2] + jumbf_app11() + src_bytes[2:])
    t = read_all(j, numeric=False)
    o = lab / "H-jumbf-written.jpg"
    r = E.run(["-XMP-dc:Creator=x", "-o", E.p(o), E.p(j)])
    t2 = read_all(o, numeric=False) if o.exists() else {}
    return {"jumbf_tags_detected": {k: v for k, v in t.items() if k.startswith("JUMBF") or k.startswith("JUMD")},
            "after_write_stderr": r.err.strip(),
            "jumbf_tags_after_write": {k: v for k, v in t2.items() if k.startswith("JUMBF") or k.startswith("JUMD")},
            "app11_bytes_preserved": jumbf_app11() in o.read_bytes() if o.exists() else None}


# ---------------------------------------------------------------- V-02: NEF ImageDataHash after metadata-only write
def exp_nef_hash(lab: Path) -> dict:
    res = {}
    for f in sorted(NEFS.glob("*.NEF")):
        o = lab / f"hash-{f.name}"
        r = E.run(["-XMP-dc:Creator=x", "-o", E.p(o), E.p(f)])
        h = lambda p: E.run(["-api", "ImageHashType=SHA256", "-ImageDataHash", "-s3", E.p(p)]).out.strip()
        res[f.name] = {"write_exit": r.code, "stderr": r.err.strip(), "hash_equal": o.exists() and h(f) == h(o)}
        if o.exists():
            o.unlink()
    return res


def main() -> dict:
    E.ensure_layout()
    OUT.mkdir(parents=True, exist_ok=True)
    lab = E.fresh_dir("s3")
    results = {"exiftool_version": E.VERSION}
    for name, fn in [("A_mapping", exp_mapping), ("B_iptc_limits", exp_iptc_limits), ("C_iptc_convert", exp_iptc_convert),
                     ("E_time", exp_time), ("F_sidecar", exp_sidecar), ("G_makernotes", exp_makernotes),
                     ("H_jumbf", exp_jumbf), ("V02_nef_hash", exp_nef_hash)]:
        try:
            results[name] = fn(lab)
        except Exception as e:  # keep going; record the failure
            results[name] = {"exception": f"{type(e).__name__}: {e}"}
    (OUT / "fields.json").write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")
    return results


if __name__ == "__main__":
    r = main()
    for k, v in r.items():
        print(f"== {k}: {json.dumps(v, ensure_ascii=False)[:600]}")
