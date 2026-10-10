#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""CycloneDX SBOM for a MoriMeta Windows build (SECURITY_MODEL §11, RELEASE_PLAN §11).

Lists exactly what THIRD_PARTY_NOTICES lists (tools/third_party_notices.py): the Rust crates
linked into the app for x86_64-pc-windows-msvc with the dependency edges between them, the npm
packages bundled into the frontend, and the pinned ExifTool Windows package. Hashes come from the
lock files: SHA-256 of each crate archive (Cargo.lock), SHA-512 of each npm tarball
(package-lock.json) and SHA-256 of the ExifTool zip (research/exiftool.lock.json). Nothing is
downloaded.

    python tools/sbom.py --out PATH [--check]

`--check` fails when a third-party component has no license or no hash.
"""

from __future__ import annotations

import argparse
import base64
import datetime
import json
import sys
import tomllib
import uuid
from pathlib import Path
from urllib.parse import quote

sys.path.insert(0, str(Path(__file__).resolve().parent))
from third_party_notices import ADAPTER, FRONTEND, ROOT, app_version, cargo_graph, npm_packages  # noqa: E402

REPO = "https://github.com/Morii9961/MoriMeta"
OWN_LICENSE = "GPL-3.0-or-later"


def licenses(expr: str) -> list[dict]:
    # Cargo's old "MIT/Apache-2.0" form is not an SPDX expression
    expr = " ".join(expr.replace("/", " OR ").split())
    return [{"expression": expr}] if expr else []


def crate_checksums() -> dict[tuple[str, str], str]:
    lock = tomllib.loads((ADAPTER / "Cargo.lock").read_text(encoding="utf-8"))
    return {(p["name"], p["version"]): p["checksum"] for p in lock["package"] if "checksum" in p}


def npm_lock() -> tuple[dict, dict]:
    """From package-lock.json: (name, version) -> (CycloneDX algorithm, hex digest), and
    (name, version) -> the (name, version) of each dependency as npm resolved it."""
    packages = json.loads((FRONTEND / "package-lock.json").read_text(encoding="utf-8")).get("packages", {})
    algs = {"sha512": "SHA-512", "sha384": "SHA-384", "sha256": "SHA-256", "sha1": "SHA-1"}

    def key(path: str) -> tuple[str, str]:
        p = packages[path]
        return (p.get("name") or path.rsplit("node_modules/", 1)[-1], p.get("version", ""))

    def resolve(path: str, dep: str) -> str | None:
        # Node's lookup: the package's own node_modules, then each enclosing one
        base = path
        while True:
            candidate = f"{base}/node_modules/{dep}" if base else f"node_modules/{dep}"
            if candidate in packages:
                return candidate
            if not base:
                return None
            base = base.rsplit("/node_modules/", 1)[0] if "/node_modules/" in base else ""

    hashes, deps = {}, {}
    for path, p in packages.items():
        if not path:
            continue
        if "integrity" in p:
            alg, _, digest = p["integrity"].split()[0].partition("-")
            if alg in algs:
                hashes[key(path)] = (algs[alg], base64.b64decode(digest).hex())
        names = {**p.get("peerDependencies", {}), **p.get("dependencies", {})}
        deps[key(path)] = sorted({key(r) for n in names if (r := resolve(path, n))})
    return hashes, deps


def npm_purl(name: str, version: str) -> str:
    return f"pkg:npm/{quote(name, safe='/')}@{quote(version)}"


def build(version: str) -> dict:
    root_id, packages, edges = cargo_graph()
    checksums = crate_checksums()
    components = []
    ref = {}
    for pid, p in sorted(packages.items(), key=lambda kv: (kv[1]["name"], kv[1]["version"])):
        if pid == root_id:
            ref[pid] = "morimeta"
            continue
        own = not p["source"]
        ref[pid] = f"crate:{p['name']}@{p['version']}"
        c = {
            "type": "library",
            "bom-ref": ref[pid],
            "name": p["name"],
            "version": p["version"],
            "licenses": licenses(OWN_LICENSE if own else p.get("license") or ""),
        }
        if own:
            c["description"] = "MoriMeta's own crate"
        else:
            c["purl"] = f"pkg:cargo/{p['name']}@{p['version']}"
            digest = checksums.get((p["name"], p["version"]))
            if digest:
                c["hashes"] = [{"alg": "SHA-256", "content": digest}]
            if p.get("repository"):
                c["externalReferences"] = [{"type": "vcs", "url": p["repository"]}]
        components.append(c)

    integrity, npm_deps = npm_lock()
    direct = json.loads((FRONTEND / "package.json").read_text(encoding="utf-8")).get("dependencies", {})
    npm_refs = []
    shipped = npm_packages()
    in_bom = {(r["name"], r["version"]) for r in shipped}
    npm_edges = {
        npm_purl(*k): sorted(npm_purl(*d) for d in npm_deps.get(k, []) if d in in_bom) for k in sorted(in_bom)
    }
    for r in shipped:
        purl = npm_purl(r["name"], r["version"])
        c = {
            "type": "library",
            "bom-ref": purl,
            "name": r["name"],
            "version": r["version"],
            "purl": purl,
            "licenses": licenses(r["license"]),
        }
        h = integrity.get((r["name"], r["version"]))
        if h:
            c["hashes"] = [{"alg": h[0], "content": h[1]}]
        components.append(c)
        if r["name"] in direct:
            npm_refs.append(purl)

    exif = json.loads((ROOT / "research" / "exiftool.lock.json").read_text(encoding="utf-8"))
    win = exif["artifacts"]["windows_x64"]
    components.append(
        {
            "type": "application",
            "bom-ref": "exiftool",
            "name": "ExifTool Windows package",
            "version": exif["version"],
            "description": "ExifTool by Phil Harvey with Strawberry Perl, shipped unmodified and run as "
            "exiftool_files\\perl.exe exiftool_files\\exiftool.pl (DECISIONS D-17); the CC0 launcher "
            "is not shipped. Strawberry Perl component licenses: Licenses_Strawberry_Perl.zip.",
            "licenses": licenses("Artistic-1.0-Perl OR GPL-1.0-or-later"),
            "hashes": [{"alg": "SHA-256", "content": win["sha256"]}],
            "externalReferences": [
                {"type": "distribution", "url": win["urls"][0]},
                {"type": "source-distribution", "url": exif["artifacts"]["source"]["urls"][0]},
                {"type": "website", "url": "https://exiftool.org"},
            ],
        }
    )

    dependencies = [
        {"ref": ref[pid], "dependsOn": sorted({ref[d] for d in deps})}
        for pid, deps in sorted(edges.items(), key=lambda kv: ref[kv[0]])
    ]
    for d in dependencies:
        if d["ref"] == "morimeta":
            d["dependsOn"] = sorted(set(d["dependsOn"]) | set(npm_refs) | {"exiftool"})
    dependencies += [{"ref": r, "dependsOn": on} for r, on in npm_edges.items()]
    dependencies.append({"ref": "exiftool", "dependsOn": []})

    return {
        "$schema": "http://cyclonedx.org/schema/bom-1.6.schema.json",
        "bomFormat": "CycloneDX",
        "specVersion": "1.6",
        "serialNumber": f"urn:uuid:{uuid.uuid4()}",
        "version": 1,
        "metadata": {
            "timestamp": datetime.datetime.now(datetime.UTC).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "tools": {
                "components": [
                    {"type": "application", "name": "tools/sbom.py", "externalReferences": [{"type": "vcs", "url": REPO}]}
                ]
            },
            "component": {
                "type": "application",
                "bom-ref": "morimeta",
                "name": "MoriMeta",
                "version": version,
                "licenses": licenses(OWN_LICENSE),
                "purl": f"pkg:github/Morii9961/MoriMeta@v{version}",
                "externalReferences": [{"type": "vcs", "url": REPO}],
            },
            "properties": [{"name": "morimeta:target", "value": "x86_64-pc-windows-msvc"}],
        },
        "components": components,
        "dependencies": dependencies,
    }


def problems(bom: dict) -> list[str]:
    out = []
    for c in bom["components"]:
        if c.get("description") == "MoriMeta's own crate":
            continue
        name = f"{c['name']} {c['version']}"
        if not c.get("licenses"):
            out.append(f"{name}: no license")
        if not c.get("hashes"):
            out.append(f"{name}: no hash in the lock file")
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--check", action="store_true")
    a = ap.parse_args()
    bom = build(app_version())
    a.out.parent.mkdir(parents=True, exist_ok=True)
    a.out.write_text(json.dumps(bom, indent=1, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    purls = [c.get("purl", "") for c in bom["components"]]
    crates = sum(p.startswith("pkg:cargo/") for p in purls)
    npm = sum(p.startswith("pkg:npm/") for p in purls)
    print(f"{a.out}: {crates} crates, {npm} npm packages, {len(purls) - crates - npm} other components")
    found = problems(bom)
    for p in found:
        print("problem:", p, file=sys.stderr)
    return 1 if a.check and found else 0


if __name__ == "__main__":
    sys.exit(main())
