"""S3 third-party compatibility corpus: files written the way MoriMeta writes (or plans to write)
them, plus a checklist CSV to record what each application shows.

JPEG creator cases go through the real product path (`mm-cli plan-creator` + `apply`).
Time, GPS and NEF-sidecar cases use ExifTool with the candidate mapping from METADATA_MODEL §5.3
and §7 (not yet product code) and are labelled `candidate` in the CSV.

Usage:  python research/s3/compat_corpus.py [--out DIR]
Needs:  research/scripts/fetch_exiftool.py, research/s3/corpus.py (for NEF-derived samples),
        `cargo build --release -p mm-cli`
Writes: DIR (default research/.work/compat-lab/<date>/) with the files, cases.csv and checklist.csv
"""

from __future__ import annotations

import csv
import datetime
import json
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import etlib as E  # noqa: E402

REPO = E.RESEARCH.parent
CLI = REPO / "target" / "release" / "mm-cli.exe"
NEF = E.WORK / "corpus" / "nikon" / "Nikon_Z8_high_efficiency_low.NEF"
JPEG_CORPUS = E.WORK / "corpus" / "jpeg"

APPS = [
    ("LR15", "Lightroom Classic 15.x"),
    ("ACR", "Adobe Bridge / Camera Raw"),
    ("C1", "Capture One"),
    ("NXS", "Nikon NX Studio"),
    ("DT", "darktable"),
    ("DK", "digiKam"),
    ("PM", "Photo Mechanic"),
    ("WIN", "Windows Explorer / Photos"),
    ("WEB", "Chrome / Edge / Firefox"),
]


def cli(data: Path, *args: str) -> dict:
    p = subprocess.run([str(CLI), "--data", str(data), "--exiftool", str(E.PKG), *args], capture_output=True)
    out = json.loads(p.stdout.decode("utf-8") or "{}")
    if p.returncode not in (0,):
        raise SystemExit(f"mm-cli {args[0]} failed: {out}")
    return out


def et(lines: list[str]) -> None:
    r = E.run(lines)
    if r.code != 0:
        raise SystemExit(f"exiftool failed: {r.err}")


def main() -> int:
    out = Path(sys.argv[sys.argv.index("--out") + 1]) if "--out" in sys.argv else E.WORK / "compat-lab" / datetime.date.today().isoformat()
    if out.exists():
        shutil.rmtree(out)
    (out / "files").mkdir(parents=True)
    data = out / "_mm-data"
    cases: list[dict] = []

    def case(cid: str, file: str, path: str, written: str, check: str, expect: str, apps: str, source: str) -> None:
        cases.append({"case": cid, "file": file, "path": path, "written": written, "check": check,
                      "expected": expect, "apps": apps, "source": source})

    # --- JPEG creator through mm-cli (product path) ---
    j1 = out / "files" / "C1_creator_cjk.jpg"
    shutil.copy(E.TIMAGES / "Writer.jpg", j1)
    names = out / "_names.txt"
    names.write_text("森 Morii\n", encoding="utf-8")
    plan = out / "_plan_c1.json"
    cli(data, "plan-creator", "--set-from", str(names), "--out", str(plan), str(j1))
    cli(data, "apply", str(plan))
    case("C1", j1.name, "", "creator 森 Morii (EXIF Artist UTF-8 + XMP dc:creator)", "Author / Creator field",
         "森 Morii (no mojibake)", "LR15 ACR C1 NXS DK PM WIN", "mm-cli")

    j2 = out / "files" / "C2_creator_latin_iptc.jpg"
    shutil.copy(JPEG_CORPUS / "iptc_latin.jpg", j2)
    plan = out / "_plan_c2.json"
    cli(data, "plan-creator", "--set", "Zoe Morii", "--out", str(plan), str(j2))
    cli(data, "apply", str(plan))
    case("C2", j2.name, "", "creator Zoe Morii in EXIF, XMP and existing Latin IPTC; IPTCDigest updated",
         "Creator; any IPTC/XMP mismatch warning", "Zoe Morii everywhere; no mismatch", "LR15 ACR C1 PM WIN", "mm-cli")

    # --- time cases (candidate mapping; D-18 needs C5/C6) ---
    base = JPEG_CORPUS / "z8.jpg"  # has OffsetTimeOriginal +02:00 and XMP xmp:CreateDate
    for cid, label, lines, expect in [
        ("C4", "absolute_keep_offset", ["-ExifIFD:DateTimeOriginal=2026:09:04 12:27:00", "-ExifIFD:SubSecTimeOriginal=",
                                         "-ExifIFD:CreateDate=2026:09:04 12:27:00", "-ExifIFD:SubSecTimeDigitized=",
                                         "-IFD0:DateTimeOriginal=2026:09:04 12:27:00", "-XMP-xmp:CreateDate=2026:09:04 12:27:00+02:00"],
         "Capture time 2026-09-04 12:27:00 (offset +02:00)"),
        ("C5", "shift_with_new_offset", ["-ExifIFD:DateTimeOriginal=2023:06:02 19:53:25", "-ExifIFD:OffsetTimeOriginal=+09:00",
                                          "-ExifIFD:CreateDate=2023:06:02 19:53:25", "-ExifIFD:OffsetTimeDigitized=+09:00",
                                          "-IFD0:DateTimeOriginal=2023:06:02 19:53:25", "-XMP-xmp:CreateDate=2023:06:02 19:53:25.67+09:00"],
         "Shown time and time zone: 19:53:25 +09:00 (D-18: does the app use the offset?)"),
        ("C6", "no_offset", ["-ExifIFD:OffsetTimeOriginal=", "-ExifIFD:OffsetTimeDigitized=", "-ExifIFD:OffsetTime=",
                              "-XMP-xmp:CreateDate=2023:06:02 18:53:25.67"],
         "Shown time 18:53:25, no time zone"),
    ]:
        f = out / "files" / f"{cid}_{label}.jpg"
        et(lines + ["-o", E.p(f), E.p(base)])
        case(cid, f.name, "", "; ".join(lines), "Capture date/time; time-zone display; sort order", expect,
             "LR15 ACR C1 NXS DK PM WIN", "candidate")

    # --- IPTC converted to UTF-8 with CJK creator (METADATA_MODEL §6 option ①) ---
    f = out / "files" / "C7_iptc_utf8_cjk.jpg"
    et(["-ex", "-tagsFromFile", "@", "-IPTC:all", "--IPTC:By-line", "-IPTC:CodedCharacterSet=UTF8",
        "-IPTC:By-line=森 Morii", "-XMP-dc:Creator=森 Morii", "-IFD0:Artist=森 Morii", "-Photoshop:IPTCDigest=new",
        "-o", E.p(f), E.p(JPEG_CORPUS / "iptc_latin.jpg")])
    case("C7", f.name, "", "IPTC converted to UTF-8; By-line 森 Morii", "Creator; IPTC charset handling", "森 Morii (no mojibake)",
         "LR15 ACR C1 PM WIN", "candidate")

    # --- MakerNotes serial blanked (V-19) ---
    f = out / "files" / "C8_makernote_serial_blank.jpg"
    et(["-MakerNotes:SerialNumber=", "-o", E.p(f), E.p(JPEG_CORPUS / "nikon_d70.jpg")])
    shutil.copy(JPEG_CORPUS / "nikon_d70.jpg", out / "files" / "C8_reference_original.jpg")
    case("C8", f.name, "", "Nikon MakerNotes:SerialNumber set to empty", "Lens name, camera serial, any error",
         "Lens: AF-S DX Zoom-Nikkor 18-70mm f/3.5-4.5G IF-ED (same as reference); serial empty", "NXS LR15 C1", "candidate")

    # --- NEF + minimal sidecar (V-03, V-07); NEF is CC0 from raw.pixls.us ---
    if NEF.exists():
        for cid, label, lines, check, expect in [
            ("N1", "creator", ["-XMP-dc:Creator=森 Morii", "-XMP-dc:Rights-x-default=© Morii 2026"],
             "Creator / Copyright of the raw", "森 Morii / © Morii 2026"),
            ("N2", "time_all_three", ["-XMP-exif:DateTimeOriginal=2026:09:04 12:27:00+02:00",
                                       "-XMP-photoshop:DateCreated=2026:09:04 12:27:00+02:00", "-XMP-xmp:CreateDate=2026:09:04 12:27:00+02:00"],
             "Capture time of the raw", "2026-09-04 12:27:00 (+02:00)"),
            ("N3", "time_photoshop_only", ["-XMP-photoshop:DateCreated=2026:09:04 12:27:00+02:00"],
             "Capture time of the raw (which XMP field is used?)", "record what is shown"),
            ("N4", "time_exif_only", ["-XMP-exif:DateTimeOriginal=2026:09:04 12:27:00+02:00"],
             "Capture time of the raw (which XMP field is used?)", "record what is shown"),
            ("N5", "gps", ["-XMP-exif:GPSLatitude=43.0642", "-XMP-exif:GPSLongitude=141.3469", "-XMP-exif:GPSAltitude=20",
                            "-XMP-exif:GPSAltitudeRef=Above Sea Level"],
             "Map location", "Sapporo, 43.0642 N 141.3469 E"),
        ]:
            d = out / "files" / f"{cid}_{label}"
            d.mkdir()
            nef = d / f"DSC_{cid}.NEF"
            shutil.copy(NEF, nef)
            et(["-ex", *lines, "-o", E.p(d / f"DSC_{cid}.xmp")])
            case(cid, f"{d.name}/{nef.name}", "", "minimal sidecar: " + "; ".join(lines), check, expect,
                 "LR15 ACR C1 NXS DT DK PM WIN", "candidate")
        case("N6", "N1_creator/DSC_N1.NEF", "", "(manual) develop the raw in LR 15 so it writes .acr, then update the sidecar with mm tools",
             ".acr untouched; sidecar change visible after Read Metadata from Files", "record", "LR15", "manual")
    else:
        print("NEF sample missing: NEF cases skipped (see research/corpus.lock.json)")

    # --- Clean Export output for browser display (orientation / colour) ---
    exp_src = E.TIMAGES / "ExifTool.jpg"
    f = out / "files" / "E1_clean_export.jpg"
    et(["-all=", "-tagsFromFile", "@", "-ICC_Profile", "-IFD0:Orientation", "-XMP-x:XMPToolkit=", "-o", E.p(f), E.p(exp_src)])
    shutil.copy(exp_src, out / "files" / "E1_reference_original.jpg")
    case("E1", f.name, "", "Clean Export (whitelist) of ExifTool.jpg", "Orientation and colour vs reference", "identical appearance",
         "WEB WIN", "candidate (S7 method)")

    with (out / "cases.csv").open("w", newline="", encoding="utf-8-sig") as fh:
        w = csv.DictWriter(fh, fieldnames=list(cases[0].keys()))
        w.writeheader()
        w.writerows(cases)
    with (out / "checklist.csv").open("w", newline="", encoding="utf-8-sig") as fh:
        w = csv.writer(fh)
        w.writerow(["case", "file", "app", "app_version", "check", "expected", "observed", "pass(y/n)", "notes"])
        for c in cases:
            for code, name in APPS:
                if code in c["apps"].split():
                    w.writerow([c["case"], c["file"], name, "", c["check"], c["expected"], "", "", ""])
    shutil.rmtree(data, ignore_errors=True)
    for tmp in out.glob("_*"):
        tmp.unlink() if tmp.is_file() else None
    print(json.dumps({"out": str(out), "cases": len(cases), "checklist_rows": sum(1 for _ in open(out / "checklist.csv", encoding="utf-8-sig")) - 1}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
