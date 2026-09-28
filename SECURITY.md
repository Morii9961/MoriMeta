# Security Policy

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub:
**[Report a vulnerability](https://github.com/Morii9961/MoriMeta/security/advisories/new)**
(Security tab → "Report a vulnerability"). Do not open a public issue.

Useful details: the affected version or commit, what an attacker controls (for example a crafted
image or sidecar file), what happens, and steps or a sample to reproduce. Please do not attach
photos that contain private information.

This is a volunteer project without a service-level agreement; reports are acknowledged as soon
as possible, and fixes are coordinated with the reporter before details are published.

## Scope

MoriMeta is pre-alpha: there is no release, installer or signed binary yet. In scope are the code
in this repository and the way it runs the bundled ExifTool — for example metadata from a
malicious file reaching an ExifTool command line, a file written outside the planned paths, a
backup or journal that allows losing a user's file, or path handling that escapes a folder
([`docs/SECURITY_MODEL.md`](docs/SECURITY_MODEL.md)).

Vulnerabilities in ExifTool itself should also be reported to its author
([exiftool.org](https://exiftool.org/)); MoriMeta pins the ExifTool version it uses and verifies
its SHA-256 (`research/exiftool.lock.json`).

## Privacy

MoriMeta has no telemetry, no crash reporting and no accounts; photos and metadata stay on the
computer. The only network request planned for the product is an update check the user can turn
off, which carries no photos, metadata or paths.
