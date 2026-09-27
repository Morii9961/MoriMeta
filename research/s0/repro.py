"""S0: re-run the ExifTool experiments cited in RESEARCH_NOTES v0.2 (F-26..F-28, F-30..F-39)
on the pinned version, in both invocation modes:

  launcher : exiftool.exe (renamed Oliver Betz launcher, the official Windows package)
  perl     : exiftool_files/perl.exe exiftool_files/exiftool.pl  (launcher bypassed; candidate)

Usage:  python research/s0/repro.py [launcher|perl|both]
Writes: research/results/s0/repro-<mode>.json
"""

from __future__ import annotations

import json
import os
import shutil
import statistics
import subprocess
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import etlib as E  # noqa: E402

OUT = E.RESEARCH / "results" / "s0"


def check(fid, claim, fn):
    try:
        observed, ok = fn()
    except Exception as e:  # a crash is a failed reproduction, not a script abort
        observed, ok = f"exception: {type(e).__name__}: {e}", False
    return {"id": fid, "claim": claim, "reproduced": ok, "observed": observed}


def main(mode: str) -> dict:
    E.ensure_layout()
    lab = E.fresh_dir(f"s0-{mode}")
    img = E.TIMAGES
    results = []

    # F-26 IPTC over-length value is truncated but still written (minor warning).
    def f26():
        src = lab / "f26.jpg"; shutil.copy(img / "IPTC.jpg", src)
        out = lab / "f26-out.jpg"
        r = E.run(["-IPTC:By-line=" + "A" * 40, "-o", E.p(out), E.p(src)], mode=mode)
        v = E.read_tags(out, ["IPTC:By-line"], mode=mode).get("IPTC:By-line")
        ok = out.exists() and "exceeds length limit" in r.err and v == "A" * 32
        return {"exit": r.code, "stderr": r.err.strip(), "stored_len": len(v or "")}, ok
    results.append(check("F-26", "IPTC By-line 40 chars -> minor warning, truncated to 32, file still written", f26))

    # F-27 writing a lang-alt tag without language suffix drops the other languages.
    def f27():
        base = lab / "f27-base.jpg"
        E.run(["-XMP-dc:Rights-x-default=A", "-XMP-dc:Rights-de=B", "-o", E.p(base), E.p(img / "ExifTool.jpg")], mode=mode)
        plain, suffixed = lab / "f27-plain.jpg", lab / "f27-xdef.jpg"
        E.run(["-XMP-dc:Rights=C", "-o", E.p(plain), E.p(base)], mode=mode)
        E.run(["-XMP-dc:Rights-x-default=C", "-o", E.p(suffixed), E.p(base)], mode=mode)
        tp = E.read_tags(plain, ["XMP-dc:Rights*"], mode=mode, extra=["-lang", ""]) if False else E.read_tags(plain, ["XMP-dc:all"], mode=mode)
        ts = E.read_tags(suffixed, ["XMP-dc:all"], mode=mode)
        de_plain = "XMP-dc:Rights-de" in tp
        de_suff = ts.get("XMP-dc:Rights-de")
        return {"plain_keys": sorted(k for k in tp if "Rights" in k), "suffixed_keys": sorted(k for k in ts if "Rights" in k)}, (not de_plain) and de_suff == "B"
    results.append(check("F-27", "-XMP-dc:Rights=V removes other languages; -XMP-dc:Rights-x-default=V keeps them", f27))

    # F-28 (re-specified): v0.2 claimed an XMP-only edit triggers 'IPTCDigest is not current'.
    # Photoshop.pm compares the stored IPTCDigest with the digest of the current IPTC block, so the
    # warning should appear when IPTC changes without a digest update, not when only XMP changes.
    def f28():
        base = lab / "f28-base.jpg"
        E.run(["-IPTC:By-line=x", "-XMP-dc:Creator=x", "-Photoshop:IPTCDigest=new", "-o", E.p(base), E.p(img / "Writer.jpg")], mode=mode)
        xmp_only, iptc_only, iptc_new = lab / "f28-xmp.jpg", lab / "f28-iptc.jpg", lab / "f28-iptc-new.jpg"
        E.run(["-XMP-dc:Creator=y", "-o", E.p(xmp_only), E.p(base)], mode=mode)
        E.run(["-IPTC:By-line=y", "-o", E.p(iptc_only), E.p(base)], mode=mode)
        E.run(["-IPTC:By-line=y", "-Photoshop:IPTCDigest=new", "-o", E.p(iptc_new), E.p(base)], mode=mode)
        w = lambda f: E.read_tags(f, ["ExifTool:Warning"], mode=mode).get("ExifTool:Warning")
        obs = {"base": w(base), "xmp_only_edit": w(xmp_only), "iptc_edit_without_digest": w(iptc_only),
               "iptc_edit_with_digest_new": w(iptc_new)}
        original_claim = obs["xmp_only_edit"] is not None and "IPTCDigest" in str(obs["xmp_only_edit"])
        corrected = (obs["xmp_only_edit"] is None and "IPTCDigest is not current" in str(obs["iptc_edit_without_digest"])
                     and obs["iptc_edit_with_digest_new"] is None)
        obs["original_claim_reproduced"] = original_claim
        obs["corrected_claim_reproduced"] = corrected
        return obs, corrected
    results.append(check("F-28", "(corrected) IPTC edit without IPTCDigest update -> 'IPTCDigest is not current'; XMP-only edit -> no warning", f28))

    # F-30 package structure.
    def f30():
        files = [q for q in E.PKG.rglob("*") if q.is_file() and q.name != "exiftool.exe"]
        size = sum(q.stat().st_size for q in files)
        perlver = subprocess.run([str(E.PERL), "-e", "print $^V"], capture_output=True, env=E.clean_env(), cwd=E.CWD,
                                 creationflags=E.CREATE_NO_WINDOW).stdout.decode()
        lic = (E.PKG / "exiftool_files" / "LICENSE").read_text(errors="replace").splitlines()[0:2]
        dlls = sorted(q.name for q in (E.PKG / "exiftool_files").glob("*.dll"))
        obs = {"file_count": len(files), "bytes": size, "perl_version": perlver, "license_head": lic, "dlls": dlls}
        return obs, perlver.startswith("v5.32") and "perl532.dll" in dlls
    results.append(check("F-30", "Package = launcher + exiftool_files (~510 files, ~35 MB) with Perl 5.32.1, GPL-3 LICENSE, Strawberry licenses", f30))

    # F-31 PERL5LIB/PERL5OPT injection; and the clean environment prevents it.
    def f31():
        evil = lab / "evil"; evil.mkdir()
        marker = lab / "evil-ran.txt"
        (evil / "evil.pm").write_text(
            "package evil; open(my $f, '>', '" + str(marker).replace("\\", "/") + "'); print $f 'ran'; close $f; 1;\n")
        env = dict(os.environ); env["PERL5LIB"] = str(evil); env["PERL5OPT"] = "-Mevil"
        E.run(["-ver"], mode=mode, env=env)
        injected = marker.exists()
        if injected:
            marker.unlink()
        E.run(["-ver"], mode=mode)  # clean env
        blocked = not marker.exists()
        return {"inherited_env_executes_module": injected, "clean_env_blocks": blocked}, injected and blocked
    results.append(check("F-31", "PERL5LIB+PERL5OPT in inherited env executes attacker module; clean env prevents it", f31))

    # F-32 relative names in argfiles: '-o.jpg' parsed as option, '#x.jpg' silently skipped; absolute paths fine.
    def f32():
        d = lab / "f32"; d.mkdir()
        for n in ["-o.jpg", "#x.jpg"]:
            shutil.copy(img / "ExifTool.jpg", d / n)
        cmd = E.base_cmd(mode) + ["-config", "", "-charset", "filename=utf8", "-@", "-"]
        rel = subprocess.run(cmd, input=b"-json\n-FileName\n-o.jpg\n#x.jpg\n", capture_output=True, cwd=d,
                             env=E.clean_env(), creationflags=E.CREATE_NO_WINDOW)
        abs_ = E.run(["-json", "-FileName", E.p(d / "-o.jpg"), E.p(d / "#x.jpg")], mode=mode)
        rel_names = [x.get("FileName") for x in (json.loads(rel.stdout) if rel.stdout.strip() else [])]
        abs_names = [x.get("FileName") for x in abs_.json()]
        ok = "#x.jpg" not in rel_names and "-o.jpg" not in rel_names and sorted(abs_names) == ["#x.jpg", "-o.jpg"]
        return {"relative": {"names": rel_names, "stderr": rel.stderr.decode(errors="replace").strip()},
                "absolute": {"names": abs_names}}, ok
    results.append(check("F-32", "argfile: relative '-o.jpg' treated as option and '#x.jpg' skipped as comment; absolute paths work", f32))

    # F-33 UTF-8 values and CJK file names via stdin argfile round-trip byte-exactly.
    def f33():
        src = lab / "照片 测试é.jpg"; shutil.copy(img / "ExifTool.jpg", src)
        out = lab / "输出_森.jpg"
        val = "森 林 — é ü 🌲 “quotes”"
        r = E.run(["-XMP-dc:Title-x-default=" + val, "-o", E.p(out), E.p(src)], mode=mode)
        t = E.read_tags(out, ["XMP-dc:Title", "System:FileName"], mode=mode)
        ok = t.get("XMP-dc:Title") == val and t.get("System:FileName") == out.name and t.get("SourceFile") == E.p(out)
        return {"exit": r.code, "title_equal": t.get("XMP-dc:Title") == val, "sourcefile_equal": t.get("SourceFile") == E.p(out)}, ok
    results.append(check("F-33", "UTF-8 value and CJK file names via stdin argfile are read/written byte-exactly", f33))

    # F-11 (related) supplementary-plane file name.
    def f11():
        src = lab / "emoji_🌲.jpg"; shutil.copy(img / "ExifTool.jpg", src)
        r = E.run(["-json", "-G1", "-System:FileName", E.p(src)], mode=mode)
        name = r.json()[0].get("System:FileName") if r.stdout.strip() else None
        out = lab / "emoji_out_🌲.jpg"
        w = E.run(["-XMP-dc:Creator=e", "-o", E.p(out), E.p(src)], mode=mode)
        ok = name == src.name and out.exists() and w.code == 0
        return {"read_exit": r.code, "name_ok": name == src.name, "write_exit": w.code, "write_ok": out.exists(),
                "stderr": (r.err + w.err).strip()}, ok
    results.append(check("F-11b", "Supplementary-plane (emoji) file names can be read and written via UTF-8 argfile (v0.2 F-11 listed them as problematic)", f11))

    # F-34 CJK into IPTC without UTF-8 CodedCharacterSet -> warning, stored as '?'.
    def f34():
        out = lab / "f34.jpg"
        r = E.run(["-IPTC:By-line=森", "-o", E.p(out), E.p(img / "ExifTool.jpg")], mode=mode)
        v = E.read_tags(out, ["IPTC:By-line"], mode=mode).get("IPTC:By-line")
        return {"stderr": r.err.strip(), "stored": v}, ("could not be encoded" in r.err and v == "?")
    results.append(check("F-34", "IPTC:By-line=森 (no CodedCharacterSet) -> warning, stored as '?'", f34))

    # F-35 -o write keeps ImageDataHash; NEF can compute ImageDataHash.
    def f35():
        out = lab / "f35.jpg"
        E.run(["-XMP-dc:Creator=hash-test", "-o", E.p(out), E.p(img / "ExifTool.jpg")], mode=mode)
        h = lambda f: E.read_tags(f, ["ImageDataHash"], mode=mode, extra=["-api", "ImageHashType=SHA256"]).get("File:ImageDataHash") \
            or E.read_tags(f, ["ImageDataHash"], mode=mode, extra=["-api", "ImageHashType=SHA256"]).get("Composite:ImageDataHash")
        a, b = h(img / "ExifTool.jpg"), h(out)
        nef = E.read_tags(img / "Nikon.nef", ["ImageDataHash"], mode=mode, extra=["-api", "ImageHashType=SHA256"])
        nefh = next((v for k, v in nef.items() if k.endswith("ImageDataHash")), None)
        return {"jpeg_src": a, "jpeg_out": b, "nef_hash": nefh}, bool(a) and a == b and bool(nefh)
    results.append(check("F-35", "ImageDataHash(SHA256) unchanged after metadata-only -o write; computable for NEF", f35))

    # F-36 writing the truncated sample NEF without -m is refused with a minor error.
    def f36():
        out = lab / "f36.nef"
        r = E.run(["-XMP-dc:Creator=x", "-o", E.p(out), E.p(img / "Nikon.nef")], mode=mode)
        return {"exit": r.code, "stderr": r.err.strip(), "output_exists": out.exists()}, (not out.exists()) and "[minor]" in r.err
    results.append(check("F-36", "Sample NEF write without -m refused ('[minor] ...'), no output created", f36))

    # F-37 -o onto an existing file errors and does not overwrite.
    def f37():
        tgt = lab / "f37.jpg"; tgt.write_bytes(b"existing")
        r = E.run(["-XMP-dc:Creator=x", "-o", E.p(tgt), E.p(img / "ExifTool.jpg")], mode=mode)
        return {"exit": r.code, "stderr": r.err.strip(), "target_unchanged": tgt.read_bytes() == b"existing"}, \
            r.code != 0 and tgt.read_bytes() == b"existing"
    results.append(check("F-37", "-o to an existing file fails (non-zero) and leaves it unchanged", f37))

    # F-38 stay_open read throughput with stdout+stderr consumed concurrently (500 small JPEGs, warm cache).
    def f38(sample):
        d = lab / f"f38-{sample}"; d.mkdir()
        srcs = [q for q in img.glob("*.jpg")][:25] if sample == "mixed" else [img / "Writer.jpg"]
        files = []
        for i in range(500):
            q = d / f"{i:04d}.jpg"; shutil.copy(srcs[i % len(srcs)], q); files.append(q)
        cmd = E.base_cmd(mode) + ["-config", "", "-charset", "filename=utf8", "-stay_open", "True", "-@", "-"]
        pr = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              cwd=E.CWD, env=E.clean_env(), creationflags=E.CREATE_NO_WINDOW)
        err_chunks = []
        t_err = threading.Thread(target=lambda: err_chunks.append(pr.stderr.read()), daemon=True); t_err.start()

        def cmd_once(paths, seq):
            body = "\n".join(["-json", "-G1", "-a", "-n"] + [E.p(x) for x in paths] + [f"-execute{seq}"]) + "\n"
            pr.stdin.write(body.encode()); pr.stdin.flush()
            buf = b""
            token = f"{{ready{seq}}}".encode()
            while token not in buf:
                chunk = pr.stdout.read1(65536)
                if not chunk:
                    raise RuntimeError("exiftool exited")
                buf += chunk
            return buf

        cmd_once(files[:5], 1)  # warm-up
        t0 = time.perf_counter()
        n = 0
        for k, i in enumerate(range(0, 500, 100)):
            cmd_once(files[i:i + 100], 10 + k); n += len(files[i:i + 100])
        dt = time.perf_counter() - t0
        pr.stdin.write(b"-stay_open\nFalse\n"); pr.stdin.flush(); pr.wait(10)
        return {"sample": sample, "files": n, "seconds": round(dt, 3), "files_per_s": round(n / dt, 1)}, True  # measurement
    results.append(check("F-38a", "stay_open read throughput, 500 copies of one simple JPEG (Writer.jpg), chunks of 100", lambda: f38("simple")))
    results.append(check("F-38b", "stay_open read throughput, 500 files cycling 25 mixed test JPEGs (maker notes etc.)", lambda: f38("mixed")))

    # F-39 cold start of `-ver`.
    def f39():
        ts = []
        for _ in range(10):
            ts.append(E.run(["-ver"], mode=mode).seconds)
        return {"median_s": round(statistics.median(ts), 3), "min_s": round(min(ts), 3), "max_s": round(max(ts), 3)}, True
    results.append(check("F-39", "Cold start time of -ver (10 runs)", f39))

    doc = {"mode": mode, "exiftool_version": E.VERSION,
           "ver_output": E.run(["-ver"], mode=mode).out.strip(), "results": results}
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / f"repro-{mode}.json").write_text(json.dumps(doc, ensure_ascii=False, indent=2), encoding="utf-8")
    return doc


if __name__ == "__main__":
    which = sys.argv[1] if len(sys.argv) > 1 else "both"
    modes = ["launcher", "perl"] if which == "both" else [which]
    for m in modes:
        d = main(m)
        print(f"== {m}  (exiftool -ver: {d['ver_output']})")
        for r in d["results"]:
            print(f"  {r['id']:6} {'OK ' if r['reproduced'] else 'NO '} {json.dumps(r['observed'], ensure_ascii=False)[:230]}")
