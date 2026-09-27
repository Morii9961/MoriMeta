"""S1 probe: how does exiftool.pl (13.59) decode argfile lines?  Empirical table for the encoder.

Each case is an exact argfile line (as bytes on stdin).  The value written to XMP-dc:Description
(x-default) is read back with -b (binary, no control-character substitution) and compared.

Usage:  python research/s1/argfile_probe.py
Writes: research/results/s1/argfile-probe.json
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import etlib as E  # noqa: E402

BS = "\\"
TAG = "-XMP-dc:Description-x-default="

# (name, argfile line, intended value)
CASES = [
    ("plain_simple", TAG + "abc", "abc"),
    ("plain_dollar_at_brace", TAG + "a$b@c${status}", "a$b@c${status}"),
    ("plain_one_leading_space", TAG + " x", " x"),
    ("plain_two_leading_spaces", TAG + "  x", "  x"),
    ("plain_trailing_spaces", TAG + "x  ", "x  "),
    ("plain_tab_inside", TAG + "a\tb", "a\tb"),
    ("plain_raw_cr_inside", TAG + "a\rb", "a\rb"),
    ("plain_trailing_raw_cr", TAG + "ab\r", "ab\r"),
    ("plain_backslash_n_literal", TAG + "a" + BS + "nb", "a" + BS + "nb"),
    ("plain_hash_value", TAG + "#1", "#1"),
    ("cstr_newline", "#[CSTR]" + TAG + "a" + BS + "nb", "a\nb"),
    ("cstr_crlf", "#[CSTR]" + TAG + "a" + BS + "r" + BS + "nb", "a\r\nb"),
    ("cstr_trailing_cr", "#[CSTR]" + TAG + "ab" + BS + "r", "ab\r"),
    ("cstr_trailing_newline", "#[CSTR]" + TAG + "ab" + BS + "n", "ab\n"),
    ("cstr_leading_space", "#[CSTR]" + TAG + " x", " x"),
    ("cstr_quote", "#[CSTR]" + TAG + 'q' + BS + '"x', 'q"x'),
    ("cstr_backslash", "#[CSTR]" + TAG + "x" + BS + BS + "y", "x" + BS + "y"),
    ("cstr_dollar_bare", "#[CSTR]" + TAG + "a$b", "a$b"),
    ("cstr_dollar_escaped", "#[CSTR]" + TAG + "a" + BS + "$b", "a$b"),
    ("cstr_at_bare", "#[CSTR]" + TAG + "a@b", "a@b"),
    ("cstr_at_escaped", "#[CSTR]" + TAG + "a" + BS + "@b", "a@b"),
    ("cstr_tab_escape", "#[CSTR]" + TAG + "a" + BS + "tb", "a\tb"),
    ("cstr_trailing_backslash", "#[CSTR]" + TAG + "ab" + BS + BS, "ab" + BS),
    ("cstr_bell_escape", "#[CSTR]" + TAG + "a" + BS + "ab", "a\x07b"),
    ("cstr_empty_value_deletes", "#[CSTR]" + TAG, None),
]


def main() -> list[dict]:
    E.ensure_layout()
    lab = E.fresh_dir("s1-argprobe")
    src = E.TIMAGES / "Writer.jpg"
    rows = []
    for i, (name, line, want) in enumerate(CASES):
        out = lab / f"{i:02d}-{name}.jpg"
        data = (line + "\n-o\n" + E.p(out) + "\n" + E.p(src) + "\n").encode("utf-8")
        r = E.run_raw_stdin(data)
        got = None
        if out.exists():
            rb = E.run_raw_stdin(("-b\n-XMP-dc:Description\n" + E.p(out) + "\n").encode())
            got = rb.stdout.decode("utf-8") if rb.stdout else None
        rows.append({"case": name, "line": line, "intended": want, "stored": got,
                     "exact": got == want, "exit": r.code, "stderr": r.err.strip()})
    out = E.RESEARCH / "results" / "s1" / "argfile-probe.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(rows, ensure_ascii=False, indent=2), encoding="utf-8")
    return rows


if __name__ == "__main__":
    for r in main():
        print(f"{r['case']:28} {'EXACT' if r['exact'] else 'DIFF '} stored={json.dumps(r['stored'], ensure_ascii=False)} "
              f"intended={json.dumps(r['intended'], ensure_ascii=False)} {r['stderr'][:80]}")
