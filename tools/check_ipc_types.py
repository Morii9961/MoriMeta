#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""The UI's IPC types against the Rust structs they mirror (ARCHITECTURE §4.2, `ipc/types.ts`).

`apps/desktop/src/ipc/types.ts` and `features/presets/model.ts` are written by hand. For each
interface PAIRS names the Rust struct it mirrors and the direction it travels:

- `out` (backend → UI): every field the UI declares as required must be serialized, and a field
  serialized only when present (`skip_serializing_if`) must be optional in TypeScript; extra Rust
  fields are allowed (the UI ignores them). UI_ONLY lists optional fields the UI adds itself.
- `in` / `both` (the UI sends it): the field sets are equal; serde would silently drop a field the
  Rust struct does not have.

UNIONS does the same for the UI's unions of tagged objects and the internally tagged Rust enums
(`#[serde(tag = ...)]`, variants in snake_case). `#[serde(flatten)]` fields are expanded through FLATTEN. Field names are compared as serialized
(`rename` honoured). A new interface or struct needs a line here.

    python tools/check_ipc_types.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TS_FILES = [ROOT / "apps/desktop/src/ipc/types.ts", ROOT / "apps/desktop/src/features/presets/model.ts"]
RUST_DIRS = [ROOT / "apps/desktop/src-tauri/src", ROOT / "crates"]

# TypeScript interface: (Rust file, Rust struct, direction)
PAIRS: dict[str, tuple[str, str, str]] = {
    "About": ("apps/desktop/src-tauri/src/cmd.rs", "AboutDto", "out"),
    "AppInfo": ("apps/desktop/src-tauri/src/cmd.rs", "AppInfo", "out"),
    "Asset": ("apps/desktop/src-tauri/src/dto.rs", "AssetDto", "out"),
    "AssetDetail": ("crates/mm-core/src/inspect.rs", "AssetDetail", "out"),
    "Attention": ("apps/desktop/src-tauri/src/dto.rs", "AttentionDto", "out"),
    "BackupInfo": ("apps/desktop/src-tauri/src/dto.rs", "BackupDto", "out"),
    "BackupUsage": ("crates/mm-core/src/retention.rs", "Usage", "out"),
    "BatchEdit": ("apps/desktop/src-tauri/src/dto.rs", "BatchEditDto", "in"),
    "CleanEntry": ("apps/desktop/src-tauri/src/cmd.rs", "CleanEntryDto", "out"),
    "CleanPlan": ("apps/desktop/src-tauri/src/cmd.rs", "CleanPlanDto", "out"),
    "DryRun": ("apps/desktop/src-tauri/src/cmd.rs", "DryRunDto", "out"),
    "ExecProgress": ("apps/desktop/src-tauri/src/dto.rs", "ExecProgressDto", "out"),
    "ExifToolState": ("apps/desktop/src-tauri/src/core.rs", "ExifToolState", "out"),
    "Exported": ("crates/mm-core/src/clean_export.rs", "Exported", "out"),
    "FieldAggregate": ("crates/mm-core/src/inspect.rs", "FieldAggregate", "out"),
    "FieldChange": ("crates/mm-domain/src/plan.rs", "FieldChange", "out"),
    "FieldView": ("crates/mm-core/src/inspect.rs", "FieldView", "out"),
    "FileDetail": ("crates/mm-core/src/history.rs", "FileDetail", "out"),
    "FileOutcome": ("apps/desktop/src-tauri/src/dto.rs", "FileOutcomeDto", "out"),
    "HistoryImport": ("crates/mm-core/src/history.rs", "HistoryImport", "out"),
    "ImportSummary": ("apps/desktop/src-tauri/src/dto.rs", "ImportSummary", "out"),
    "KeepSpec": ("crates/mm-domain/src/clean.rs", "KeepSpec", "both"),
    "KindCounts": ("apps/desktop/src-tauri/src/dto.rs", "KindCounts", "out"),
    "OpDetail": ("crates/mm-core/src/history.rs", "OpDetail", "out"),
    "OpReport": ("apps/desktop/src-tauri/src/dto.rs", "OpReportDto", "out"),
    "OpSummary": ("crates/mm-core/src/history.rs", "OpSummary", "out"),
    "OperationBackup": ("crates/mm-core/src/retention.rs", "OpBackup", "out"),
    "PlanEntry": ("apps/desktop/src-tauri/src/dto.rs", "EntryDto", "out"),
    "PlanPage": ("apps/desktop/src-tauri/src/dto.rs", "PageDto", "out"),
    "PlanSummary": ("crates/mm-domain/src/plan.rs", "PlanSummary", "out"),
    "PlanView": ("apps/desktop/src-tauri/src/dto.rs", "PlanView", "out"),
    "Prediction": ("crates/mm-domain/src/clean.rs", "Prediction", "out"),
    "Preflight": ("crates/mm-core/src/preflight.rs", "Preflight", "out"),
    "Preset": ("crates/mm-domain/src/rules.rs", "Preset", "both"),
    "PresetInfo": ("apps/desktop/src-tauri/src/cmd.rs", "PresetDto", "out"),
    "PrunePreview": ("crates/mm-core/src/backups.rs", "PrunePreview", "out"),
    "RecoverySummary": ("crates/mm-core/src/recovery.rs", "RecoverySummary", "out"),
    "RemovedSegment": ("crates/mm-domain/src/clean.rs", "RemovedSegment", "out"),
    "Row": ("apps/desktop/src-tauri/src/dto.rs", "RowDto", "out"),
    "Rule": ("crates/mm-domain/src/rules.rs", "Rule", "both"),
    "Setting": ("apps/desktop/src-tauri/src/dto.rs", "SettingDto", "out"),
    "StartupInfo": ("apps/desktop/src-tauri/src/core.rs", "StartupInfo", "out"),
    "UpdateInfo": ("apps/desktop/src-tauri/src/updater.rs", "Info", "out"),
    "WriteTargets": ("crates/mm-domain/src/plan.rs", "WriteTargets", "out"),
}
# TypeScript union of tagged objects: (Rust file, internally tagged enum, direction)
UNIONS: dict[str, tuple[str, str, str]] = {
    "AppEvent": ("apps/desktop/src-tauri/src/dto.rs", "AppEvent", "out"),
    "ListEdit": ("apps/desktop/src-tauri/src/dto.rs", "ListEdit", "in"),
    "TextEdit": ("apps/desktop/src-tauri/src/dto.rs", "TextEdit", "in"),
    "GpsEdit": ("apps/desktop/src-tauri/src/dto.rs", "GpsEditDto", "in"),
    "TimeEdit": ("apps/desktop/src-tauri/src/dto.rs", "TimeEditDto", "in"),
}
# a flattened field's type: (Rust file, struct)
FLATTEN = {
    "PlanEntry": ("crates/mm-domain/src/plan.rs", "PlanEntry"),
    "Attention": ("crates/mm-core/src/inspect.rs", "Attention"),
    "OpSummary": ("crates/mm-core/src/history.rs", "OpSummary"),
}
# optional fields the UI sets on what it received
UI_ONLY = {("CleanPlan", "spec")}


def rust_struct(file: str, name: str) -> tuple[dict[str, str], list[str]] | None:
    """Serialized field name → attributes, and the flattened types."""
    src = (ROOT / file).read_text(encoding="utf-8")
    m = re.search(rf"pub struct {name}(?:<[^>]*>)?\s*\{{(.*?)\n\}}", src, re.S)
    if not m:
        return None
    fields: dict[str, str] = {}
    flat: list[str] = []
    attrs = ""
    for line in m.group(1).split("\n"):
        s = line.strip()
        if s.startswith("#["):
            attrs += s
            continue
        f = re.match(r"pub (\w+)\s*:\s*(.+?),?$", s)
        if f:
            if re.search(r"\bskip\b(?!_)", attrs) or "skip_serializing)" in attrs:
                pass
            elif "flatten" in attrs:
                flat.append(re.sub(r"^&'\w+\s+|^Box<|>$", "", f.group(2)).strip())
            else:
                rn = re.search(r'rename\s*=\s*"([^"]+)"', attrs)
                fields[rn.group(1) if rn else f.group(1)] = attrs
            attrs = ""
        elif s and not s.startswith("//"):
            attrs = ""
    return fields, flat


def ts_interfaces() -> dict[str, dict[str, bool]]:
    """Interface → field name → optional."""
    out: dict[str, dict[str, bool]] = {}
    bases: dict[str, list[str]] = {}
    for f in TS_FILES:
        src = f.read_text(encoding="utf-8")
        for m in re.finditer(r"export interface (\w+)(?:<[^>]*>)?(?:\s+extends\s+([\w, ]+))?\s*\{(.*?)\n\}", src, re.S):
            fields: dict[str, bool] = {}
            if m.group(2):
                bases[m.group(1)] = [b.strip() for b in m.group(2).split(",")]
            depth = 0
            for line in m.group(3).split("\n"):
                s = line.strip()
                if depth == 0:
                    fm = re.match(r"(\w+)(\??)\s*:", s)
                    if fm:
                        fields[fm.group(1)] = fm.group(2) == "?"
                depth += s.count("{") - s.count("}")
            out[m.group(1)] = fields
    # an interface that extends another has its fields too
    for name, bs in bases.items():
        for b in bs:
            for f, opt in out.get(b, {}).items():
                out[name].setdefault(f, opt)
    return out


def snake(name: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def rust_enum(file: str, name: str) -> dict[str, set[str]] | None:
    """Tag value → serialized fields of an internally tagged enum (`#[serde(tag = ...)]`). A
    newtype variant has the fields of its struct, found through PAIRS."""
    src = (ROOT / file).read_text(encoding="utf-8")
    m = re.search(rf"pub enum {name}\s*\{{(.*?)\n\}}", src, re.S)
    if not m:
        return None
    body = re.sub(r"//[^\n]*", "", m.group(1))
    out: dict[str, set[str]] = {}
    for v in re.finditer(r"(\w+)\s*(\{[^}]*\}|\(([^)]*)\))?\s*,?", body):
        vname, payload, newtype = v.group(1), v.group(2), v.group(3)
        if not vname[0].isupper():
            continue
        tag = snake(vname)
        if newtype:
            inner = newtype.strip()
            struct = next((pair for pair in PAIRS.values() if pair[1] == inner), None)
            fields = rust_struct(struct[0], inner)[0] if struct else {}
            out[tag] = set(fields)
        elif payload:
            out[tag] = set(re.findall(r"(\w+)\s*:", payload))
        else:
            out[tag] = set()
    return out


def ts_union(name: str) -> dict[str, dict[str, bool]] | None:
    """Tag value → field → optional, for `export type X = { tag: 'a'; f: T } | ...`."""
    for f in TS_FILES:
        src = f.read_text(encoding="utf-8")
        m = re.search(rf"export type {name}\s*=\s*(.*?)(?=\nexport |\n\n|\Z)", src, re.S)
        if not m:
            continue
        out: dict[str, dict[str, bool]] = {}
        for member in re.findall(r"\(?\{([^{}]*)\}(?:\s*&\s*(\w+))?\)?", m.group(1)):
            inner, ext = member
            fields = {fm.group(1): fm.group(2) == "?" for fm in re.finditer(r"(\w+)(\??)\s*:", inner)}
            tag = next((re.search(rf"{k}\s*:\s*'(\w+)'", inner).group(1) for k in fields if re.search(rf"{k}\s*:\s*'\w+'", inner)), None)
            if tag is None:
                continue
            fields = {k: o for k, o in fields.items() if not re.search(rf"{k}\s*:\s*'{tag}'", inner)}
            if ext:
                fields.update(ts_interfaces().get(ext, {}))
            out[tag] = fields
        return out
    return None


def check_unions(problems: list[str]) -> None:
    for name, (file, enum, way) in sorted(UNIONS.items()):
        r = rust_enum(file, enum)
        u = ts_union(name)
        if r is None or u is None:
            problems.append(f"{name}: {'enum ' + enum + ' not found in ' + file if r is None else 'union type not found in the UI'}")
            continue
        where = f"{name} (UI) / {enum} ({file})"
        for tag in sorted(set(r) - set(u)):
            problems.append(f"{where}: the backend has {tag!r}, the UI's union does not")
        for tag in sorted(set(u) - set(r)):
            problems.append(f"{where}: the UI has {tag!r}, the backend does not")
        for tag in sorted(set(r) & set(u)):
            for f, optional in u[tag].items():
                if f not in r[tag]:
                    problems.append(f"{where}: {tag!r} — the UI has {f}, the backend does not")
            if way in ("in", "both"):
                for f in sorted(r[tag] - set(u[tag])):
                    problems.append(f"{where}: {tag!r} — the backend expects {f}, the UI does not send it")


def main() -> int:
    problems: list[str] = []
    ts = ts_interfaces()
    for name in sorted(set(ts) - set(PAIRS)):
        problems.append(f"{name}: an IPC interface without a Rust struct in PAIRS (tools/check_ipc_types.py)")
    for name, (file, struct, way) in sorted(PAIRS.items()):
        if name not in ts:
            problems.append(f"{name}: in PAIRS but not an interface in the UI's IPC types")
            continue
        found = rust_struct(file, struct)
        if not found:
            problems.append(f"{name}: struct {struct} not found in {file}")
            continue
        rfields, flat = found
        for ft in flat:
            if ft not in FLATTEN or not (sub := rust_struct(*FLATTEN[ft])):
                problems.append(f"{name}: {struct} flattens {ft}, which FLATTEN does not resolve")
                continue
            rfields.update(sub[0])
        tfields = ts[name]
        where = f"{name} (UI) / {struct} ({file})"
        for f, optional in sorted(tfields.items()):
            if f not in rfields and not (optional and (name, f) in UI_ONLY):
                problems.append(f"{where}: the UI reads {f}, the backend does not send it")
            elif f in rfields and "skip_serializing_if" in rfields[f] and not optional and way == "out":
                problems.append(f"{where}: {f} is left out when empty, so the UI must declare it optional ({f}?)")
        if way in ("in", "both"):
            for f in sorted(set(rfields) - set(tfields)):
                problems.append(f"{where}: the backend expects {f}, the UI's type does not have it")
    check_unions(problems)
    for p in problems:
        print("problem:", p, file=sys.stderr)
    if not problems:
        print(f"IPC types agree: {len(PAIRS)} interfaces and {len(UNIONS)} tagged unions against their Rust types")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
