"""S0: is `perl.exe exiftool.pl` (launcher bypassed) behaviourally equivalent to the official launcher?

Checks
  E1  read output (-json -a -G1 -u -n) identical for every file in t/images
  E2  write output (-o) byte-identical for writable samples
  E3  Unicode + long (> 260 chars) paths: read and write in both modes
  E4  process model: launcher/perl.exe spawn no additional interpreter process (checked separately)

Usage:  python research/s0/perl_equiv.py
Writes: research/results/s0/perl-equivalence.json
"""

from __future__ import annotations

import json
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import etlib as E  # noqa: E402

VOLATILE = {"System:FileAccessDate", "System:FileModifyDate", "System:FileCreateDate", "System:FileInodeChangeDate"}


def read_all(path: Path, mode: str) -> dict:
    r = E.run(["-json", "-a", "-G1", "-u", "-n", E.p(path)], mode=mode)
    try:
        d = r.json()[0]
    except Exception:
        return {"__stdout__": r.out, "__stderr__": r.err, "__exit__": r.code}
    d = {k: v for k, v in d.items() if k not in VOLATILE}
    d["__stderr__"] = r.err
    d["__exit__"] = r.code
    return d


def main() -> dict:
    E.ensure_layout()
    lab = E.fresh_dir("s0-equiv")
    res: dict = {}

    # E1
    files = sorted(q for q in E.TIMAGES.iterdir() if q.is_file())
    diffs = []
    for f in files:
        a, b = read_all(f, "launcher"), read_all(f, "perl")
        if a != b:
            keys = sorted(k for k in set(a) | set(b) if a.get(k) != b.get(k))
            diffs.append({"file": f.name, "keys": keys[:20]})
    res["E1_read_identical"] = {"files": len(files), "differing_files": diffs, "pass": not diffs}

    # E2
    writable = ["Writer.jpg", "ExifTool.jpg", "Nikon.jpg", "ExifTool.tif", "PNG.png", "XMP.xmp", "RIFF.webp", "DNG.dng"]
    wdiffs = []
    for name in writable:
        outs = {}
        for mode in ("launcher", "perl"):
            o = lab / f"{mode}-{name}"
            r = E.run(["-XMP-dc:Creator=森 Morii", "-XMP-dc:Rights-x-default=© 2026", "-EXIF:Artist=Morii",
                       "-o", E.p(o), E.p(E.TIMAGES / name)], mode=mode)
            outs[mode] = (E.sha256(o) if o.exists() else None, r.code, r.err.strip())
        if outs["launcher"] != outs["perl"]:
            wdiffs.append({"file": name, "launcher": outs["launcher"], "perl": outs["perl"]})
    res["E2_write_identical"] = {"files": writable, "differing": wdiffs, "pass": not wdiffs}

    # E3 Unicode + long path
    long_dir = lab
    seg = "长路径测试目录_long_path_segment_" + "x" * 30  # ~60 chars
    for _ in range(5):
        long_dir = long_dir / seg
    long_dir_os = Path("\\\\?\\" + str(long_dir))
    long_dir_os.mkdir(parents=True, exist_ok=True)
    src = Path("\\\\?\\" + str(long_dir / "源文件 é.jpg"))
    shutil.copyfile(E.TIMAGES / "Writer.jpg", src)
    e3 = {"path_length": len(str(long_dir / "源文件 é.jpg"))}
    for mode in ("launcher", "perl"):
        r = E.run(["-json", "-G1", "-System:FileName", E.p(long_dir / "源文件 é.jpg")], mode=mode)
        out = long_dir / f"输出-{mode}.jpg"
        w = E.run(["-XMP-dc:Creator=x", "-o", E.p(out), E.p(long_dir / "源文件 é.jpg")], mode=mode)
        e3[mode] = {"read_exit": r.code, "read_ok": bool(r.stdout.strip()) and r.json()[0].get("System:FileName") == "源文件 é.jpg",
                    "write_exit": w.code, "write_ok": Path("\\\\?\\" + str(out)).exists(), "stderr": (r.err + w.err).strip()[:300]}
    e3["pass"] = all(e3[m]["read_ok"] and e3[m]["write_ok"] for m in ("launcher", "perl"))
    res["E3_unicode_long_path"] = e3

    out = E.RESEARCH / "results" / "s0" / "perl-equivalence.json"
    out.write_text(json.dumps(res, ensure_ascii=False, indent=2), encoding="utf-8")
    return res


if __name__ == "__main__":
    r = main()
    for k, v in r.items():
        print(k, "PASS" if v.get("pass") else "FAIL", json.dumps({x: y for x, y in v.items() if x != "pass"}, ensure_ascii=False)[:600])
