"""S7: Clean Export (JPEG) — is the Preview's itemised removal list exactly what the export removes,
and can the output be proven to contain only whitelisted segments and tags?

Evaluated under D-15 option (c) for validation only (not an approved scope).

For every source JPEG:
  1. inventory  = full tag read (-a -G0:1 -u -U) + marker-segment map (Rust jpegseg)
  2. preview    = predicted keep/remove per tag and per segment (KeepSpec below); unknown items
                  are shown as "unidentified -> removed"
  3. export     = ExifTool: -all= -tagsFromFile @ <keep tags> -XMP-x:XMPToolkit= -o out src
  4. check      = segment whitelist (jpegseg check) + tag whitelist + ImageDataHash(src) == (out)
  5. consistency: predicted removed set == actually removed set; no residue; no unexpected loss
  verdict: EXPORT (all pass) or BLOCK (reasons).  Negative controls must all be BLOCKed.

Usage:  python research/s7/clean_export.py
Writes: research/results/s7/clean-export.json
"""

from __future__ import annotations

import json
import shutil
import struct
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "s3"))
import etlib as E  # noqa: E402
from fields import jumbf_app11  # noqa: E402

OUT = E.RESEARCH / "results" / "s7"
JPEGSEG = E.RESEARCH / "spikes" / "target" / "release" / "jpegseg.exe"
CORPUS = E.WORK / "corpus" / "jpeg"

# ---- KeepSpec (METADATA_MODEL §10.1 defaults: camera, lens, exposure, capture time, author/copyright on)
KEEP = {
    "camera": ["EXIF:IFD0:Make", "EXIF:IFD0:Model"],
    "lens": ["EXIF:ExifIFD:LensMake", "EXIF:ExifIFD:LensModel", "EXIF:ExifIFD:LensInfo"],
    "exposure": ["EXIF:ExifIFD:ExposureTime", "EXIF:ExifIFD:FNumber", "EXIF:ExifIFD:ISO", "EXIF:ExifIFD:ExposureProgram",
                 "EXIF:ExifIFD:ExposureCompensation", "EXIF:ExifIFD:MeteringMode", "EXIF:ExifIFD:Flash",
                 "EXIF:ExifIFD:FocalLength", "EXIF:ExifIFD:FocalLengthIn35mmFormat", "EXIF:ExifIFD:WhiteBalance"],
    "capture_time": ["EXIF:ExifIFD:DateTimeOriginal", "EXIF:ExifIFD:SubSecTimeOriginal", "EXIF:ExifIFD:OffsetTimeOriginal"],
    "author": ["EXIF:IFD0:Artist", "EXIF:IFD0:Copyright", "XMP:XMP-dc:Creator", "XMP:XMP-dc:Rights",
               "XMP:XMP-xmpRights:Marked", "XMP:XMP-xmpRights:UsageTerms", "XMP:XMP-xmpRights:WebStatement"],
    "forced_orientation_color": ["EXIF:IFD0:Orientation", "EXIF:ExifIFD:ColorSpace", "EXIF:ExifIFD:Gamma",
                                 "EXIF:InteropIFD:InteropIndex"],
    # structure copied from the source so ExifTool does not substitute its defaults
    # (YCbCrPositioning is a chroma-siting hint; resolution is print DPI) — no personal data
    "forced_structure": ["EXIF:IFD0:XResolution", "EXIF:IFD0:YResolution", "EXIF:IFD0:ResolutionUnit",
                         "EXIF:IFD0:YCbCrPositioning", "EXIF:ExifIFD:ExifVersion", "EXIF:ExifIFD:ComponentsConfiguration",
                         "EXIF:InteropIFD:InteropVersion"],
}
KEEP_TAGS = {t for ts in KEEP.values() for t in ts}
# structural groups that may be present in the output (not personal data)
STRUCTURAL_PREFIXES = ("ICC_Profile:", "APP14:Adobe:", "File:", "Composite:", "ExifTool:", "SourceFile")
# tags ExifTool itself adds when (re)creating EXIF; allowed in output, reported as "added structural"
STRUCTURAL_ADDED = {"EXIF:IFD0:XResolution", "EXIF:IFD0:YResolution", "EXIF:IFD0:ResolutionUnit",
                    "EXIF:IFD0:YCbCrPositioning", "EXIF:ExifIFD:ExifVersion", "EXIF:ExifIFD:ComponentsConfiguration",
                    "EXIF:InteropIFD:InteropVersion"}
NON_STORED = ("File:", "Composite:", "ExifTool:", "SourceFile")

SEGMENTS_ALLOWED = {"SOI", "EOI", "DQT", "DHT", "DRI", "SOS", "APP0:JFIF", "APP1:Exif", "APP1:XMP",
                    "APP2:ICC_PROFILE", "APP14:Adobe"}

CATEGORIES = [  # (category, predicate on key) — for Preview grouping only
    ("gps", lambda k: ":GPS:" in k or "GPS" in k.split(":")[-1]),
    ("serial_numbers", lambda k: "SerialNumber" in k),
    ("owner", lambda k: "OwnerName" in k),
    ("history_ids", lambda k: "xmpMM" in k or "DocumentID" in k or "InstanceID" in k or "ImageUniqueID" in k or "DocumentAncestors" in k),
    ("embedded_previews", lambda k: any(x in k for x in ("ThumbnailImage", "PreviewImage", "PhotoshopThumbnail", "MPImage", "JpgFromRaw", "ThumbnailOffset", "ThumbnailLength"))),
    ("people_regions", lambda k: "RegionInfo" in k or "PersonInImage" in k or "mwg-rs" in k),
    ("comments", lambda k: k.endswith(":Comment") or "UserComment" in k or ":XP" in k),
    ("content_credentials", lambda k: k.startswith("JUMBF")),
    ("maker_notes", lambda k: k.startswith("MakerNotes:")),
    ("structural", lambda k: k.startswith("JFIF:") or any(x in k for x in ("FlashpixVersion", "ExifImageWidth", "ExifImageHeight"))),
    ("software", lambda k: k.endswith(":Software") or "CreatorTool" in k or "XMPToolkit" in k),
    ("location_names", lambda k: any(x in k for x in ("City", "Country", "State", "Sublocation", "Location"))),
    ("descriptive", lambda k: any(x in k for x in ("Title", "Description", "Subject", "Keywords", "Caption", "Headline"))),
]


def category(k: str) -> str:
    for name, pred in CATEGORIES:
        if pred(k):
            return name
    return "other"


def dup_pairs(pairs):
    d = {}
    for k, v in pairs:
        n = 2
        kk = k
        while kk in d:
            kk = f"{k}#{n}"
            n += 1
        d[kk] = v
    return d


def read_tags(p: Path) -> dict:
    r = E.run(["-json", "-a", "-G0:1", "-u", "-U", "-api", "StructFormat=JSONQ", E.p(p)])
    return json.loads(r.stdout.decode("utf-8"), object_pairs_hook=dup_pairs)[0]


def stored(tags: dict) -> dict:
    return {k: v for k, v in tags.items() if not k.startswith(NON_STORED) and k != "ExifTool:Warning"}


def base(k: str) -> str:
    return k.split("#")[0]


def jpegseg(mode: str, files: list[Path]) -> dict:
    r = subprocess.run([str(JPEGSEG), mode] + [str(f) for f in files], capture_output=True)
    return {Path(k).name: v for k, v in json.loads(r.stdout).items()}


def image_hash(p: Path) -> str:
    return E.run(["-api", "ImageHashType=SHA256", "-ImageDataHash", "-s3", E.p(p)]).out.strip()


def export_cmd(src: Path, out: Path) -> list[str]:
    copy = [f"-{t.split(':', 1)[1]}" for t in sorted(KEEP_TAGS)]  # "-IFD0:Make" etc. (family-1 group)
    return ["-all=", "-tagsFromFile", "@", "-ICC_Profile"] + copy + ["-XMP-x:XMPToolkit=", "-o", E.p(out), E.p(src)]


def preview(src_tags: dict, segs: dict) -> dict:
    keep, remove = {}, {}
    for k, v in stored(src_tags).items():
        b = base(k)
        if b in KEEP_TAGS or b.startswith(STRUCTURAL_PREFIXES):
            keep[k] = v
        else:
            remove[k] = {"category": category(b), "value": v if isinstance(v, (int, float)) or len(str(v)) < 60 else str(v)[:57] + "..."}
    seg_keep, seg_remove = [], []
    for s in segs.get("segments", []):
        lab = s["label"]
        lab_n = "SOF" if lab.startswith("SOF") else lab
        (seg_keep if (lab_n in SEGMENTS_ALLOWED or lab_n == "SOF") else seg_remove).append(
            {"label": lab, "bytes": s["payload_len"] + 4, "unidentified": lab.split(":")[-1].startswith("unknown") if ":" in lab else False})
    if segs.get("trailer_len"):
        seg_remove.append({"label": "TRAILER(after EOI)", "bytes": segs["trailer_len"], "unidentified": True})
    return {"keep_tags": keep, "remove_tags": remove, "keep_segments": seg_keep, "remove_segments": seg_remove}


def evaluate(src: Path, out: Path, lab: Path, exporter=None) -> dict:
    src_tags = read_tags(src)
    src_segs = jpegseg("inspect", [src])[src.name]
    if "parse_error" in src_segs:
        return {"verdict": "BLOCK", "reasons": [f"source not parseable: {src_segs['parse_error']}"]}
    pv = preview(src_tags, src_segs)
    cmd = (exporter or export_cmd)(src, out)
    r = E.run(cmd)
    if not out.exists():
        return {"verdict": "BLOCK", "reasons": [f"export failed: {r.err.strip()}"], "preview": summarize(pv)}
    return check_output(src, src_tags, pv, out, r.err.strip())


def check_output(src: Path, src_tags: dict, pv: dict, out: Path, export_stderr: str = "") -> dict:
    reasons = []
    out_tags = stored(read_tags(out))
    seg_check = jpegseg("check", [out])[out.name]
    if not seg_check.get("ok"):
        reasons += [f"segment: {x}" for x in seg_check.get("reasons", [seg_check.get("parse_error")])]
    residue = [k for k in out_tags if not (base(k) in KEEP_TAGS or base(k).startswith(STRUCTURAL_PREFIXES) or base(k) in STRUCTURAL_ADDED)]
    if residue:
        reasons.append(f"tags outside whitelist in output: {residue[:10]}")
    s_src = set(stored(src_tags))
    s_out = set(out_tags)
    actually_removed = s_src - s_out
    predicted_removed = set(pv["remove_tags"])
    unexpected_loss = sorted(actually_removed - predicted_removed)      # predicted keep, but gone
    predicted_but_present = sorted(predicted_removed - actually_removed)  # predicted removed, still there
    if predicted_but_present:
        reasons.append(f"predicted removal not performed: {predicted_but_present[:10]}")
    if unexpected_loss:
        reasons.append(f"preview inconsistent: predicted keep but removed: {unexpected_loss[:10]}")
    added = sorted(s_out - s_src)
    bad_added = [k for k in added if base(k) not in STRUCTURAL_ADDED and not base(k).startswith(STRUCTURAL_PREFIXES)]
    if bad_added:
        reasons.append(f"unexpected tags added: {bad_added[:10]}")
    changed = sorted(k for k in s_src & s_out if src_tags.get(k) != out_tags.get(k) and not base(k).startswith(("ICC_Profile:",)))
    h_src, h_out = image_hash(src), image_hash(out)
    if not h_src or h_src != h_out:
        reasons.append(f"ImageDataHash differs or unavailable ({h_src[:12]} vs {h_out[:12]})")
    out_segs = jpegseg("inspect", [out])[out.name]
    return {
        "verdict": "EXPORT" if not reasons else "BLOCK",
        "reasons": reasons,
        "preview": summarize(pv),
        "consistency": {
            "predicted_removed": len(predicted_removed), "actually_removed": len(actually_removed),
            "equal": predicted_removed == actually_removed,
            "unexpected_loss_of_kept_tags": unexpected_loss, "predicted_but_present": predicted_but_present,
            "added_structural": [k for k in added if k not in bad_added], "kept_but_value_changed": changed[:20],
        },
        "output_segments": [s["label"] for s in out_segs.get("segments", [])] + (["TRAILER"] if out_segs.get("trailer_len") else []),
        "export_stderr": export_stderr,
    }


def summarize(pv: dict) -> dict:
    cats: dict = {}
    for k, v in pv["remove_tags"].items():
        cats.setdefault(v["category"], 0)
        cats[v["category"]] += 1
    return {"remove_by_category": cats, "remove_segments": pv["remove_segments"], "keep_tag_count": len(pv["keep_tags"])}


# ---------------------------------------------------------------- hazard corpus
def insert_after_soi(b: bytes, seg: bytes) -> bytes:
    return b[:2] + seg + b[2:]


def app(n: int, payload: bytes) -> bytes:
    return bytes([0xFF, 0xE0 + n]) + struct.pack(">H", len(payload) + 2) + payload


def build_hazards(d: Path) -> list[Path]:
    d.mkdir(parents=True, exist_ok=True)
    out = []
    base_ = CORPUS / "z8.jpg"
    k = d / "H1-kitchen-sink.jpg"
    E.run(["-GPSLatitude=43.0642", "-GPSLatitudeRef=N", "-GPSLongitude=141.3469", "-GPSLongitudeRef=E",
           "-ExifIFD:OwnerName=Owner Person", "-Comment=secret comment", "-ExifIFD:UserComment=user comment",
           "-XMP-xmpMM:DocumentID=xmp.did:123", "-XMP-xmpMM:InstanceID=xmp.iid:456",
           "-XMP-xmpMM:OriginalDocumentID=ABC", "-XMP-iptcExt:PersonInImage=Jane Doe",
           "-XMP-photoshop:City=Sapporo", "-XMP-dc:Title-x-default=Title", "-XMP-dc:Description-x-default=" + ("x" * 70000),
           "-IPTC:By-line=Morii", "-IPTC:City=Sapporo", "-IFD0:Software=Lightroom", "-XMP-xmp:CreatorTool=Lightroom",
           "-ICC_Profile<=" + E.p(E.TIMAGES / "ICC_Profile.icc"), "-o", E.p(k), E.p(base_)])
    b = k.read_bytes()
    b = insert_after_soi(b, jumbf_app11())
    b = insert_after_soi(b, app(5, b"MMTEST\0" + b"hidden payload " * 10))
    b = insert_after_soi(b, app(15, b"\x00\x01\x02unknown app15"))
    b = insert_after_soi(b, app(14, b"Adobe\x00\x64\x00\x00\x00\x00\x01"))
    b = b + (CORPUS / "writer.jpg").read_bytes()  # a whole second JPEG after EOI
    k.write_bytes(b)
    out.append(k)
    # H2 real MPF + trailer sample from ExifTool tests, H3 extended XMP sample, H4 FPXR, H5 multi-seg EXIF
    for name, src in [("H2-mpf.jpg", None), ("H3-extxmp.jpg", E.TIMAGES / "ExtendedXMP.jpg"),
                      ("H5-multiseg-makernotes.jpg", CORPUS / "z8_mn_multiseg.jpg")]:
        if src is None:
            # find an ExifTool sample with an MPF segment
            segs = jpegseg("inspect", sorted(E.TIMAGES.glob("*.jpg")))
            mpf = [n for n, v in segs.items() if any(s["label"] == "APP2:MPF" for s in v.get("segments", []))]
            if not mpf:
                continue
            src = E.TIMAGES / mpf[0]
        shutil.copy(src, d / name)
        out.append(d / name)
    return out


def negative_controls(lab: Path) -> dict:
    src = lab / "hazards" / "H1-kitchen-sink.jpg"
    res = {}
    # N1 naive exporter: only GPS removed
    res["N1 exporter removes GPS only"] = evaluate(src, lab / "N1.jpg", lab, exporter=lambda s, o: ["-GPS:all=", "-o", E.p(o), E.p(s)])
    # N2 correct export, then trailer appended
    good = lab / "N-good.jpg"
    E.run(export_cmd(src, good))
    n2 = lab / "N2.jpg"
    n2.write_bytes(good.read_bytes() + b"LEAK")
    res["N2 trailer appended to good output"] = check_output(src, read_tags(src), preview(read_tags(src), jpegseg("inspect", [src])[src.name]), n2)
    # N3 unknown APP segment inserted into good output
    n3 = lab / "N3.jpg"
    n3.write_bytes(insert_after_soi(good.read_bytes(), app(9, b"LEAK\0data")))
    res["N3 APP9 inserted into good output"] = check_output(src, read_tags(src), preview(read_tags(src), jpegseg("inspect", [src])[src.name]), n3)
    # N4 thumbnail re-added into good output
    n4 = lab / "N4.jpg"
    thumb = lab / "thumb.jpg"
    thumb.write_bytes(E.run_raw_stdin(f"-b\n-ThumbnailImage\n{E.p(src)}\n".encode()).stdout)
    E.run(["-ThumbnailImage<=" + E.p(thumb), "-o", E.p(n4), E.p(good)])
    res["N4 thumbnail re-added"] = check_output(src, read_tags(src), preview(read_tags(src), jpegseg("inspect", [src])[src.name]), n4)
    return {k: {"verdict": v["verdict"], "reasons": v["reasons"][:4]} for k, v in res.items()}


def main() -> dict:
    E.ensure_layout()
    OUT.mkdir(parents=True, exist_ok=True)
    lab = E.fresh_dir("s7")
    hz = build_hazards(lab / "hazards")
    sources = hz + sorted(CORPUS.glob("*.jpg")) + sorted(E.TIMAGES.glob("*.jpg"))
    rows = {}
    for s in sources:
        key = f"{s.parent.name}/{s.name}"
        try:
            rows[key] = evaluate(s, lab / "out" / f"{s.parent.name}-{s.name}", lab)
        except Exception as e:
            rows[key] = {"verdict": "BLOCK", "reasons": [f"harness exception: {type(e).__name__}: {e}"]}
        (lab / "out").mkdir(exist_ok=True)
    neg = negative_controls(lab)
    summary = {
        "sources": len(rows),
        "export": sum(1 for r in rows.values() if r["verdict"] == "EXPORT"),
        "block": sum(1 for r in rows.values() if r["verdict"] == "BLOCK"),
        "consistency_equal": sum(1 for r in rows.values() if r.get("consistency", {}).get("equal")),
        "negative_controls_blocked": sum(1 for r in neg.values() if r["verdict"] == "BLOCK"),
        "negative_controls": len(neg),
    }
    doc = {"summary": summary, "keep_spec": KEEP, "segments_allowed": sorted(SEGMENTS_ALLOWED),
           "structural_added_allowed": sorted(STRUCTURAL_ADDED), "negative_controls": neg, "files": rows}
    (OUT / "clean-export.json").write_text(json.dumps(doc, ensure_ascii=False, indent=2, default=str), encoding="utf-8")
    return doc


if __name__ == "__main__":
    d = main()
    print(json.dumps(d["summary"]))
    for k, v in d["negative_controls"].items():
        print(" ", k, v["verdict"], v["reasons"][:2])
    for k, v in d["files"].items():
        if v["verdict"] == "BLOCK":
            print("BLOCK", k, [x[:140] for x in v["reasons"][:3]])
