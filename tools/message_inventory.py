"""List every user-visible text the Rust backend builds, for the message-code decision.

The UI shows these strings today (Preview statuses and notes, per-file results, errors). A
translated UI needs them as "message code + parameters" (docs/DESIGN_REVIEW_ENGINEERING.md §4);
the codes are to be agreed with the design session. This script only inventories what exists.

Usage: python tools/message_inventory.py   (writes docs/MESSAGE_INVENTORY.md, UTF-8)
       python tools/message_inventory.py --templates        (the texts as JSON)
       python tools/message_inventory.py --check CATALOG    (the UI's translation catalog covers
                                                            every text, DECISIONS §3 item 8)
"""

from __future__ import annotations

import collections
import io
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]

# (category, what the UI shows it as, pattern that starts the construction)
KINDS = [
    ("plan.blocked", "Preview: not written", r"EntryStatus::Blocked\("),
    ("plan.unsupported", "Preview: format read-only", r"EntryStatus::Unsupported\("),
    ("plan.note", "Preview: note on an entry", r"notes\.push\("),
    ("file.skipped", "Result: skipped", r"Outcome::Skipped\("),
    ("file.failed", "Result: failed", r"Outcome::Failed\("),
    ("file.conflict", "Result: conflict", r"Outcome::Conflict\("),
    ("file.attention", "Result: needs attention", r"Outcome::Attention\("),
    ("file.disk_full", "Result: paused, volume full", r"Outcome::DiskFull\("),
    ("file.cancelled", "Result: cancelled", r"Outcome::Cancelled\("),
    ("error.input", "Error dialog (input)", r"CoreError::Input\("),
    ("error.domain", "Error / validation text", r"\bErr\((format!\(|\")"),
]
LITERAL = re.compile(r'"((?:[^"\\]|\\.)*)"')
CONST = re.compile(r"\bconst [A-Z_0-9]+: &str =")
SHARED = "Where the constant is used (status, note, result or error)"


def rust_files():
    for p in sorted((ROOT / "crates").glob("*/src/**/*.rs")):
        if "mm-cli" in p.parts:
            continue  # the development driver; its texts never reach the UI
        yield p


def body_without_tests(text: str) -> str:
    cut = text.find("#[cfg(test)]")
    return text if cut < 0 else text[:cut]


def scan():
    rows = []
    for p in rust_files():
        lines = body_without_tests(p.read_text(encoding="utf-8")).split("\n")
        rel = p.relative_to(ROOT).as_posix()
        for i, line in enumerate(lines):
            for cat, shown, pat in KINDS:
                m = re.search(pat, line)
                if not m:
                    continue
                # from the constructor on, over the next lines of the same statement
                chunk = " ".join([line[m.start() :]] + [l.strip() for l in lines[i + 1 : i + 4]])
                lit = LITERAL.search(chunk)
                if lit and re.search(r"[A-Za-z]{2}", lit.group(1)):
                    rows.append((cat, shown, rel, i + 1, lit.group(1)))
                break
            # a text kept in a constant and used where statuses, results and errors are built
            m = CONST.search(line)
            if m:
                chunk = " ".join([line[m.end() :]] + [l.strip() for l in lines[i + 1 : i + 3]])
                lit = LITERAL.search(chunk)
                if lit and re.search(r"[A-Za-z]{2,} [A-Za-z]{2,}", lit.group(1)):
                    rows.append(("shared", SHARED, rel, i + 1, lit.group(1)))
    # the reasons Probe::refusal gives (mm-fs), shown as Blocked / Skipped
    fs = (ROOT / "crates/mm-fs/src/lib.rs").read_text(encoding="utf-8")
    start = fs.find("pub fn refusal(")
    if start >= 0:
        block = fs[start : fs.find("\n    }\n", start)]
        base = fs[:start].count("\n") + 1
        for off, line in enumerate(block.split("\n")):
            lit = LITERAL.search(line)
            if lit:
                rows.append(("file.refusal", "Preview and result: not written",
                             "crates/mm-fs/src/lib.rs", base + off, lit.group(1)))
    return rows


def flags(text: str) -> str:
    out = []
    if re.search(r"\{e\}|\{err\}|\{why\}|\{r\}", text):
        out.append("embeds system/engine text")
    if re.search(r"\{(p|path|dest|out)\}|\{\}: \{e\}", text):
        out.append("names a file")
    if re.search(r"\{[a-z_0-9:?]*\}", text) and not out:
        out.append("parameters")
    return ", ".join(out)


def normalize(text: str) -> str:
    """The text as the program builds it: line continuations joined, escapes resolved."""
    text = re.sub(r"\\\s+", "", text)
    return text.replace('\\"', '"').replace("\\n", " ").replace("\\\\", "\\")


def templates() -> list[str]:
    """Every distinct text, in source order: the keys of the UI's translation catalog."""
    out: list[str] = []
    for _, _, _, _, text in scan():
        t = normalize(text)
        if t not in out:
            out.append(t)
    return out


def check(catalog_path: str) -> int:
    """The UI's catalog (DECISIONS §3 item 8) must translate every text, and nothing else, in
    its `inventory` section; `extra` holds texts the scan cannot find and must not repeat one."""
    import json

    cat = json.loads((ROOT / catalog_path).read_text(encoding="utf-8"))
    have = set(cat.get("inventory", {}))
    want = templates()
    missing = [t for t in want if t not in have]
    stale = sorted(have - set(want))
    doubled = sorted(set(cat.get("extra", {})) & set(want))
    empty = [k for sec in ("inventory", "extra") for k, v in cat.get(sec, {}).items() if not v.strip()]
    for title, items in (("missing", missing), ("stale", stale), ("in extra and inventory", doubled), ("empty", empty)):
        for t in items:
            print(f"{title}: {t}")
    ok = not (missing or stale or doubled or empty)
    print(f"{catalog_path}: {len(want)} texts, {'ok' if ok else 'NOT ok'}")
    return 0 if ok else 1


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--check":
        sys.exit(check(sys.argv[2]))
    if len(sys.argv) == 2 and sys.argv[1] == "--templates":
        import json

        sys.stdout.reconfigure(encoding="utf-8")
        print(json.dumps(templates(), ensure_ascii=False, indent=1))
        return
    out = io.StringIO()
    sys.stdout, real = out, sys.stdout
    try:
        render()
    finally:
        sys.stdout = real
    dest = ROOT / "docs" / "MESSAGE_INVENTORY.md"
    dest.write_text(out.getvalue(), encoding="utf-8", newline="\n")
    print(f"{dest.relative_to(ROOT)}: {out.getvalue().count(chr(10))} lines")


def render():
    rows = scan()
    by = collections.Counter(r[0] for r in rows)
    print("# Backend message inventory")
    print()
    print("> Generated by `python tools/message_inventory.py` from the Rust sources of the core crates "
          "(tests and the `mm-cli` development driver excluded). "
          "Input for choosing message codes with the design session "
          "(DESIGN_REVIEW_ENGINEERING §4); no codes are proposed here. "
          "\"Embeds system/engine text\" marks messages that carry an OS or ExifTool error verbatim: "
          "those need a code plus the raw text as a detail, not a translation. "
          "The scan finds texts at the places they are built (status, result and error "
          "constructors, notes) and in string constants used there (`shared`); a text kept in a "
          "variable first and passed on later is not listed.")
    print()
    print(f"{len(rows)} texts. By category:")
    print()
    print("| Category | Shown as | Count |")
    print("|---|---|---:|")
    shown = {k[0]: k[1] for k in KINDS}
    shown["file.refusal"] = "Preview and result: not written"
    shown["shared"] = SHARED
    for cat, n in sorted(by.items()):
        print(f"| `{cat}` | {shown.get(cat, '')} | {n} |")
    print()
    for cat in sorted(by):
        print(f"## `{cat}`")
        print()
        print("| Where | Text | Note |")
        print("|---|---|---|")
        for c, _, rel, line, text in rows:
            if c != cat:
                continue
            t = text.replace("|", "\\|").replace("\\\n", " ")
            t = re.sub(r"\\\s+", " ", t)
            print(f"| `{rel}:{line}` | {t} | {flags(text)} |")
        print()


if __name__ == "__main__":
    main()
