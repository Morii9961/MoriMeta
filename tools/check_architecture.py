#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Architecture rules CI keeps (DEVELOPMENT_PLAN §5.4 "dependency direction" and "capability diff").

1. Crate dependencies point one way (ARCHITECTURE §4.1): mm-domain, mm-exiftool, mm-fs and mm-store
   use at most mm-domain; mm-core uses those four; mm-cli and the desktop adapter use any of them;
   nothing uses mm-cli or the adapter. Dev-dependencies are not counted.
2. mm-domain stays pure (no IO, no async, no processes): its external dependencies are an explicit
   list, so adding one is a reviewed change to this file.
3. The frontend can call exactly the app's own commands (SECURITY_MODEL §6): the command list in
   `build.rs`, the grants in `capabilities/default.json`, the handlers registered in `main.rs` and
   the commands the UI calls are the same set; there is one capability file and no plugin permission; the frontend imports
   no Tauri plugin package; the content security policy loads nothing from outside the app.

4. Every call in `ipc/index.ts` passes exactly the command's parameters, named as Tauri names them.
5. The UI uses only settings the backend knows, and the development mock offers the same ones.

    python tools/check_architecture.py
"""

from __future__ import annotations

import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ADAPTER = ROOT / "apps" / "desktop" / "src-tauri"
LEAVES = {"mm-domain", "mm-exiftool", "mm-fs", "mm-store"}
ALLOWED = {
    "mm-domain": set(),
    "mm-exiftool": {"mm-domain"},
    "mm-fs": {"mm-domain"},
    "mm-store": {"mm-domain"},
    "mm-core": LEAVES,
    "mm-cli": LEAVES | {"mm-core"},
    "morimeta": LEAVES | {"mm-core"},
}
DOMAIN_EXTERNAL = {"chrono", "serde", "serde_json"}


def manifests() -> dict[str, dict]:
    out = {}
    for p in sorted((ROOT / "crates").glob("*/Cargo.toml")) + [ADAPTER / "Cargo.toml"]:
        m = tomllib.loads(p.read_text(encoding="utf-8"))
        out[m["package"]["name"]] = m
    return out


def runtime_deps(m: dict) -> set[str]:
    names = set(m.get("dependencies", {})) | set(m.get("build-dependencies", {}))
    for table in m.get("target", {}).values():
        names |= set(table.get("dependencies", {})) | set(table.get("build-dependencies", {}))
    return names


def check_dependencies(problems: list[str]) -> None:
    crates = manifests()
    unknown = set(crates) - set(ALLOWED)
    if unknown:
        problems.append(f"crates without a rule in tools/check_architecture.py: {sorted(unknown)}")
    for name, m in crates.items():
        deps = runtime_deps(m)
        internal = {d for d in deps if d.startswith("mm-") or d == "morimeta"}
        for d in sorted(internal - ALLOWED.get(name, set())):
            problems.append(f"{name} depends on {d} (ARCHITECTURE §4.1 allows {sorted(ALLOWED.get(name, set())) or 'none'})")
        if name == "mm-domain":
            for d in sorted(deps - internal - DOMAIN_EXTERNAL):
                problems.append(f"mm-domain depends on {d}: mm-domain stays pure (add it to DOMAIN_EXTERNAL only after review)")


def check_capabilities(problems: list[str]) -> None:
    build = (ADAPTER / "build.rs").read_text(encoding="utf-8")
    block = re.search(r"const COMMANDS: &\[&str\] = &\[(.*?)\];", build, re.DOTALL)
    commands = set(re.findall(r'"([a-z_]+)"', block.group(1))) if block else set()
    if not commands:
        problems.append("build.rs: COMMANDS not found")

    caps = sorted((ADAPTER / "capabilities").glob("*"))
    if [c.name for c in caps] != ["default.json"]:
        problems.append(f"capabilities/: expected only default.json, found {[c.name for c in caps]}")
    cap = json.loads((ADAPTER / "capabilities" / "default.json").read_text(encoding="utf-8"))
    granted = set()
    for p in cap.get("permissions", []):
        if not isinstance(p, str) or not p.startswith("allow-") or ":" in p:
            problems.append(f"capabilities/default.json: {p!r} is not one of the app's own commands")
        else:
            granted.add(p.removeprefix("allow-").replace("-", "_"))
    if cap.get("windows") != ["main"] or cap.get("remote"):
        problems.append("capabilities/default.json: must apply to the main window only, never to remote content")

    main = (ADAPTER / "src" / "main.rs").read_text(encoding="utf-8")
    handler = re.search(r"generate_handler!\[(.*?)\]", main, re.DOTALL)
    registered = set(re.findall(r"(?:\w+::)*([a-z_]+)\s*,", handler.group(1) + ",")) if handler else set()

    for what, names in (("granted in capabilities/default.json", granted), ("registered in main.rs", registered)):
        for c in sorted(commands - names):
            problems.append(f"{c}: in build.rs COMMANDS but not {what}")
        for c in sorted(names - commands):
            problems.append(f"{c}: {what} but not in build.rs COMMANDS")

    # what the UI calls (apps/desktop/src/ipc is the only caller; the mock and tests do not count):
    # a call to a command that is not granted fails at run time, a granted command no one calls is
    # surface that only an attacker would use
    called = set()
    for f in (ROOT / "apps" / "desktop" / "src").rglob("*.ts*"):
        if "mock" in f.name or ".test." in f.name:
            continue
        called |= set(re.findall(r"\b(?:call|invoke)(?:<[^>]*>)?\(\s*'([a-z_]+)'", f.read_text(encoding="utf-8")))
    for c in sorted(called - commands):
        problems.append(f"the UI calls {c}, which build.rs COMMANDS does not list")
    for c in sorted(commands - called):
        problems.append(f"{c} is granted to the UI but nothing in apps/desktop/src calls it: remove it")

    pkg = json.loads((ROOT / "apps" / "desktop" / "package.json").read_text(encoding="utf-8"))
    for d in sorted({**pkg.get("dependencies", {}), **pkg.get("devDependencies", {})}):
        if d.startswith("@tauri-apps/plugin-"):
            problems.append(f"package.json: {d}: the frontend gets no plugin API (SECURITY_MODEL §6)")

    conf = json.loads((ADAPTER / "tauri.conf.json").read_text(encoding="utf-8"))
    app = conf.get("app", {})
    if app.get("withGlobalTauri"):
        problems.append("tauri.conf.json: withGlobalTauri exposes the API to every script")
    csp = app.get("security", {}).get("csp")
    if not isinstance(csp, str):
        problems.append("tauri.conf.json: no content security policy")
    else:
        for bad in ("'unsafe-eval'", "http://*", "https:", "*"):
            sources = [s for d in csp.split(";") for s in d.split()[1:]]
            if bad in sources or (bad == "https:" and any(s.startswith("https:") for s in sources)):
                problems.append(f"tauri.conf.json: the CSP allows {bad}")
        script = next((d.split()[1:] for d in csp.split(";") if d.split()[:1] == ["script-src"]), None)
        if script != ["'self'"]:
            problems.append(f"tauri.conf.json: script-src must be 'self' only, is {script}")
    if set(conf.get("plugins", {})) - {"updater"}:
        problems.append(f"tauri.conf.json: plugins beyond the updater: {sorted(set(conf['plugins']) - {'updater'})}")


INJECTED = re.compile(r"^(AppHandle|CoreState|State<|tauri::State<|Window|WebviewWindow|tauri::Window|tauri::AppHandle)")


def split_top(s: str) -> list[str]:
    """Split at commas outside <...>, (...), [...] and {...}."""
    out, depth, cur = [], 0, ""
    for ch in s:
        if ch in "<([{":
            depth += 1
        elif ch in ">)]}":
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur)
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur)
    return [x.strip() for x in out if x.strip()]


def camel(name: str) -> str:
    head, *rest = name.split("_")
    return head + "".join(w[:1].upper() + w[1:] for w in rest)


def check_command_args(problems: list[str]) -> None:
    """4. The keys the UI passes to a command are the command's parameters as Tauri names them
    (camelCase); a missing required one fails only when that command runs."""
    params: dict[str, dict[str, bool]] = {}
    for f in sorted((ADAPTER / "src").glob("*.rs")):
        src = f.read_text(encoding="utf-8")
        for m in re.finditer(r"#\[tauri::command\]\s*pub (?:async )?fn (\w+)\((.*?)\)\s*(?:->|\{)", src, re.S):
            args = {}
            for p in split_top(m.group(2)):
                name, _, ty = p.partition(":")
                ty = ty.strip()
                if INJECTED.match(ty):
                    continue
                args[camel(name.strip())] = ty.startswith("Option<")
            params[m.group(1)] = args
    ipc = (ROOT / "apps" / "desktop" / "src" / "ipc" / "index.ts").read_text(encoding="utf-8")
    for m in re.finditer(r"\b(?:call|invoke)(?:<[^>]*>)?\(\s*'([a-z_]+)'\s*(?:,\s*\{)?", ipc):
        cmd = m.group(1)
        keys: set[str] = set()
        if m.group(0).rstrip().endswith("{"):
            depth, i = 1, m.end()
            while depth and i < len(ipc):
                depth += {"{": 1, "}": -1}.get(ipc[i], 0)
                i += 1
            body = ipc[m.end() : i - 1]
            keys = {k.split(":")[0].strip() for k in split_top(body)}
        if cmd not in params:
            continue  # unknown commands are reported by the permission check
        want = params[cmd]
        for k in sorted(keys - set(want)):
            problems.append(f"the UI passes {k!r} to {cmd}, which has no such parameter")
        for k in sorted(k for k, optional in want.items() if not optional and k not in keys):
            problems.append(f"the UI calls {cmd} without its parameter {k!r}")


def check_setting_names(problems: list[str]) -> None:
    """5. The settings the UI reads and writes are ones the backend knows (mm-core settings.rs);
    the development mock offers the same set."""
    consts = {}
    for f in (ROOT / "crates").glob("*/src/*.rs"):
        for m in re.finditer(r'pub const (\w+): &str = "([^"]+)"', f.read_text(encoding="utf-8")):
            consts[m.group(1)] = m.group(2)
    reg_src = (ROOT / "crates" / "mm-core" / "src" / "settings.rs").read_text(encoding="utf-8")
    known = set()
    for m in re.finditer(r'\bname:\s*(?:"([^"]+)"|(?:crate::)?(?:\w+::)*(\w+)),', reg_src):
        known.add(m.group(1) or consts.get(m.group(2), f"<unresolved {m.group(2)}>"))
    if len(known) < 5 or any(k.startswith("<") for k in known):
        problems.append(f"mm-core settings registry not read correctly: {sorted(known)}")
        return
    used: dict[str, str] = {}
    for f in (ROOT / "apps" / "desktop" / "src").rglob("*.ts*"):
        if "mock" in f.name or ".test." in f.name or f.name == "messages.ts":
            continue
        src = f.read_text(encoding="utf-8")
        pat = r"(?:settingSet\(|\bget\(|\.name === )\s*'([a-z_]+\.[a-z_.]+)'"
        for m in re.finditer(pat, src):
            used.setdefault(m.group(1), f.name)
    for name, where in sorted(used.items()):
        if name not in known:
            problems.append(f"{where} uses the setting {name!r}, which the backend does not know")
    mock = (ROOT / "apps" / "desktop" / "src" / "ipc" / "mockSettings.ts").read_text(encoding="utf-8")
    offered = set(re.findall(r"'([a-z_]+\.[a-z_.]+)':", mock.split("DEFAULTS", 1)[1].split("\n}", 1)[0]))
    if offered != known:
        problems.append(f"the development mock's settings differ from the backend's: missing {sorted(known - offered)}, extra {sorted(offered - known)}")


def main() -> int:
    problems: list[str] = []
    check_dependencies(problems)
    check_capabilities(problems)
    check_command_args(problems)
    check_setting_names(problems)
    for p in problems:
        print("problem:", p, file=sys.stderr)
    if not problems:
        print("architecture rules hold: dependency direction, pure mm-domain, the frontend's permissions")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
