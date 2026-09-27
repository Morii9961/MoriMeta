"""Build the synthetic JPEG corpus used by S3 and S7 (from pinned ExifTool samples + CC0 NEFs).

  z8.jpg        full-size JPEG from the Z8 NEF's JpgFromRaw, with all NEF metadata copied in
                (EXIF, Nikon MakerNotes incl. serials, XMP) and an IFD1 thumbnail
  writer.jpg    t/images/Writer.jpg (simple EXIF)
  iptc_latin.jpg  IPTC without CodedCharacterSet, By-line "Café" (cp1252 byte), XMP, IPTCDigest current
  iptc_utf8.jpg   IPTC with CodedCharacterSet=UTF8
  xmp_only.jpg  t/images/XMP.jpg

Usage: python research/s3/corpus.py   -> research/.work/corpus/jpeg/
"""

from __future__ import annotations

import json
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import etlib as E  # noqa: E402

OUT = E.WORK / "corpus" / "jpeg"
NEF = E.WORK / "corpus" / "nikon" / "Nikon_Z8_high_efficiency_low.NEF"


def must(r: E.Result, what: str) -> None:
    if r.code != 0:
        raise SystemExit(f"{what} failed: {r.err}")


def build() -> dict:
    if OUT.exists():
        shutil.rmtree(OUT)
    OUT.mkdir(parents=True)
    work = OUT / "_work"
    work.mkdir()

    # z8.jpg
    raw_jpg = work / "jpgfromraw.jpg"
    r = E.run_raw_stdin(f"-b\n-JpgFromRaw\n{E.p(NEF)}\n".encode())
    raw_jpg.write_bytes(r.stdout)
    thumb = work / "thumb.jpg"
    thumb.write_bytes(E.run_raw_stdin(f"-b\n-IFD0:ThumbnailImage\n{E.p(NEF)}\n".encode()).stdout)
    z8 = OUT / "z8.jpg"
    must(E.run(["-tagsFromFile", E.p(NEF), "-all:all", "--MakerNotes", "-ThumbnailImage<=" + E.p(thumb),
                "-o", E.p(z8), E.p(raw_jpg)]), "z8")
    # NEF maker notes (~215 KB) do not fit one APP1: ExifTool writes non-standard multi-segment EXIF
    must(E.run(["-tagsFromFile", E.p(NEF), "-all:all", "-MakerNotes", "-ThumbnailImage<=" + E.p(thumb),
                "-o", E.p(OUT / "z8_mn_multiseg.jpg"), E.p(raw_jpg)]), "z8_mn")

    # in-camera Nikon JPEGs with genuine maker notes (ExifTool test images)
    shutil.copy(E.TIMAGES / "NikonD70.jpg", OUT / "nikon_d70.jpg")
    shutil.copy(E.TIMAGES / "NikonD2Hs.jpg", OUT / "nikon_d2hs.jpg")
    shutil.copy(E.TIMAGES / "Writer.jpg", OUT / "writer.jpg")
    shutil.copy(E.TIMAGES / "XMP.jpg", OUT / "xmp_only.jpg")

    # IPTC Latin (no CodedCharacterSet), with XMP and a current IPTCDigest
    must(E.run(["-IPTC:By-line=Café", "-IPTC:CopyrightNotice=(c) old", "-IPTC:ObjectName=Title",
                "-XMP-dc:Creator=Café", "-Photoshop:IPTCDigest=new", "-o", E.p(OUT / "iptc_latin.jpg"),
                E.p(E.TIMAGES / "Writer.jpg")]), "iptc_latin")
    # IPTC UTF-8
    must(E.run(["-IPTC:CodedCharacterSet=UTF8", "-IPTC:By-line=Café", "-IPTC:ObjectName=Title",
                "-Photoshop:IPTCDigest=new", "-o", E.p(OUT / "iptc_utf8.jpg"), E.p(E.TIMAGES / "Writer.jpg")]), "iptc_utf8")
    shutil.rmtree(work)
    return {f.name: E.sha256(f) for f in sorted(OUT.glob("*.jpg"))}


if __name__ == "__main__":
    print(json.dumps(build(), indent=2))
