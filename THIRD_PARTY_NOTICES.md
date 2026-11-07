# NekoPak third-party notices

NekoPak is an independent community project, not affiliated with or endorsed by PikPak.
The M0 code in this repository was independently implemented from the supplied NekoPak
development specification. It is licensed under MIT (see LICENSE). No Piko,
pikpaktui, Bern, or other cloud client's source code was copied or adapted.

## Incorporated dependencies

The exact resolved versions and checksums are in Cargo.lock and package-lock.json.
Rust runtime/build components include Tauri, Wry, Tao, serde, serde_json, Tokio,
async-trait, rusqlite, SQLite (bundled by libsqlite3-sys), time, uuid, sha2 and url.
The build tools include TypeScript and the Tauri CLI. These are actual dependencies,
unlike the research candidates below. Tauri/Wry/Tao are MIT OR Apache-2.0;
rusqlite is MIT; SQLite is public-domain software. The full resolved dependency
license inventory is generated from Cargo metadata by scripts/package_release.py.

Where published crates omit full license text, verified upstream license files are
supplemented from pinned source commits under licenses/upstream, with URL and SHA-256
records in licenses/sources.json. This copies license notices, not implementation
code. selectors 0.38.0 declares MPL-2.0; Mozilla's canonical license text is included.
Exact source archive URLs are provided for every registry dependency, including
MPL-covered sources. No such dependency source was modified.

The portable ZIP includes DEPENDENCY_LICENSES.json and the LICENSE/NOTICE files
available in each resolved crate's published source archive. That inventory also
includes build, test, and other-platform dependencies; it is intentionally broader
than the set of machine code linked into this Windows binary. It does not certify
compliance or replace a release-time legal/security audit.

Some unused other-platform packages in the all-target lock graph do not supply a
license file or verified upstream commit: these remain explicitly listed in the
size report's license_text_unresolved field. This Windows build does not assert
that Linux, Android, Apple, GNU Windows or UEFI distributions are license-audited.

WebView2 is a Microsoft system runtime, required separately and not bundled.
No official PikPak binary, Rclone, WinFsp, JVM, Chromium, FFmpeg or mpv is included.
The simple N icon in shell/icon.svg was authored for this project; it uses no
PikPak mark or third-party image/font asset.

## Research/reference status (2026-11-07)

- [Piko](https://github.com/NihilDigit/piko), AGPL-3.0-or-later: BEHAVIOR/DESIGN_REFERENCE at v1.4.0 `d61c1580783911be557d4c39026c300517187bcb`; selected segment/UI/download-cache source and tests read to identify user behavior, no implementation copied or adapted. NekoPak media implementation is independent. Details: docs/UPSTREAM_PARITY_RESEARCH.md.
- [pikpaktui](https://github.com/Bengerthelorf/pikpaktui), Apache-2.0: RESEARCH_ONLY at `68b7a6bdf6e4abe2a049974dec2c38e736f2a25c`; Cargo manifest reviewed, including blocking reqwest/TUI/image dependencies. No runtime or source reuse.
- [52funny/pikpakcli](https://github.com/52funny/pikpakcli), MIT per supplied kit: NOT_USED; future research candidate.
- [PikPak Enhancement Master](https://github.com/digbug82/PikPak_Enhancement_Master), AGPL-3.0 per kit: NOT_USED.
- [pikpak-kotlin](https://github.com/NihilDigit/pikpak-kotlin), MIT: PROTOCOL/BEHAVIOR_REFERENCE at `8eaf5587a883ed4ea9bcca7b43a9194118719e63`; Range/shared-cache/quota documentation and selected source reviewed. No code/runtime reuse yet.
- [MediaMP](https://github.com/open-ani/mediamp), Apache-2.0 per kit: NOT_USED.
- [Rclone](https://github.com/rclone/rclone), MIT: PROTOCOL_REFERENCE at `82cb84b2014bec6a167dc28eb53233b2d4abe1b3`; PikPak backend/docs reviewed, no code copied, not run or bundled. Future external adapter candidate.
- [WinFsp](https://github.com/winfsp/winfsp), GPLv3 plus FLOSS exception per kit: EXTERNAL_TOOL_OPTIONAL in future; no linking, driver installation or redistribution.
- [Cloud Files API](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfregistersyncroot): NOT_USED; future Windows platform integration, separate from drive-letter mounting.
- [Official PikPak CLI](https://mypikpak.com/en-US/cli): EXTERNAL_TOOL_OPTIONAL, v0.6.0 Windows x64 binary hash `b39a2bf29d69e561ab3a6f7ec2bfd899930008ed311eb3bd99c3697802e93b04` verified against official checksums; version/help/unauthenticated JSON probed. Independently written read-only adapter, opt-in external profile, no binary/source redistribution. Authenticated success/Range/redistribution rights remain unverified. See docs/CLI_ADAPTER.md.
- [OpenList](https://github.com/OpenListTeam/OpenList): NOT_USED; license not audited.
- [Bern](https://github.com/AimesSoft/Bern), MIT © 2026 MCDFSteve: DESIGN_REFERENCE, README/Cargo.toml/LICENSE at `a86bbb85bd525d6c9b0c011ef1571caae4273490`. Protocol/engine/host separation is relevant. No source or dependency included; no Bern benchmark reproduced.
- [FFmpeg](https://ffmpeg.org/), EXTERNAL_TOOL_OPTIONAL: existing imageio_ffmpeg/Gyan 7.1 essentials Windows executable used in explicit local media tests, SHA-256 `2ce797a0f88d7f067180338fb227f7b1928ea727bd9a4d7a1d022f7c52af71a3`. This particular build declares GPL/version3. No FFmpeg source/library/binary is copied, linked or bundled with NekoPak. Redistribution of a future media component needs its own build/license/source audit.

Future source copying must record upstream commit, original paths, destination
paths, modifications, copyright notices and license text before integration.

