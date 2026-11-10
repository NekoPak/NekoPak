# NekoPak third-party notices

NekoPak is an independent community project, not affiliated with or endorsed by PikPak.
The M0 code in this repository was independently implemented from the supplied NekoPak
development specification. It is licensed under MIT (see LICENSE). No Piko,
pikpaktui, Bern, or other cloud client's source code was copied or adapted.

## Incorporated dependencies

Android integration (2026-11-09) uses jni 0.21.1 and ndk-context 0.1.1
(MIT OR Apache-2.0), the Tauri Android host and dialog/fs/opener Android components,
Kotlin standard libraries and resolved AndroidX/Material libraries. The owned
Keystore/JNI/lifecycle/SAF integration was written independently. Generated host
templates originate from Tauri CLI 2.12.4 commit
993038c49185307dfc895ab7657d77ca0d5ed7c7; original template paths and toolchain
overrides are recorded in docs/ANDROID_RESEARCH.md and scripts/android_project.py.
scripts/package_android.py includes actual Android runtime dependency coordinates,
available POM/NOTICE/license files and upstream license supplements inside each APK
and a companion licenses ZIP. Android uses the device WebView/runtime, not bundled
Chromium or a desktop JVM. No Piko AGPL implementation is incorporated.

The Android license inventory also carries the original Maven Central POMs for
com.google.guava:listenablefuture:1.0 and its guava-parent:26.0-android parent.
They establish the inherited Apache-2.0 declaration even on a clean Gradle runner.
Exact URLs, SHA256 values and unchanged metadata paths are recorded in
licenses/maven-sources.json and bundled as MAVEN_SOURCES.json. Only license/build
metadata is retained; no additional Guava implementation is copied into this repository.

The exact resolved versions and checksums are in Cargo.lock and package-lock.json.
Rust runtime/build components include Tauri, Wry, Tao, serde, serde_json, Tokio,
async-trait, rusqlite, SQLite (bundled by libsqlite3-sys), time, uuid, sha2, url,
reqwest/rustls/ring, zeroize and windows-sys (Windows Credential Manager bindings).
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
The cat/box application icon in public/brand/app-icon.svg was authored for this project;
it uses no PikPak mark or third-party image/font asset. The historical shell/icon.svg is
not the current desktop icon. Tauri's icon generator produces the Windows resources.

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
- [Official PikPak CLI](https://mypikpak.com/en-US/cli): HISTORICAL_RESEARCH_REMOVED on 2026-11-08. The independently written adapter, probe command and runtime dependency have been deleted. No binary/source is redistributed. Old observations are retained only under docs/history and docs/evidence.
- [OpenList](https://github.com/OpenListTeam/OpenList): NOT_USED; license not audited.
- [Bern](https://github.com/AimesSoft/Bern), MIT © 2026 MCDFSteve: DESIGN_REFERENCE, README/Cargo.toml/LICENSE at `a86bbb85bd525d6c9b0c011ef1571caae4273490`. Protocol/engine/host separation is relevant. No source or dependency included; no Bern benchmark reproduced.
- [FFmpeg](https://ffmpeg.org/), EXTERNAL_TOOL_OPTIONAL: existing imageio_ffmpeg/Gyan 7.1 essentials Windows executable used in explicit local media tests, SHA-256 `2ce797a0f88d7f067180338fb227f7b1928ea727bd9a4d7a1d022f7c52af71a3`. This particular build declares GPL/version3. No FFmpeg source/library/binary is copied, linked or bundled with NekoPak. Redistribution of a future media component needs its own build/license/source audit.

Future source copying must record upstream commit, original paths, destination
paths, modifications, copyright notices and license text before integration.


## Native API and supplied v3 UI (2026-11-08)

The native HTTP adapter is independently written from the supplied API mapping,
not an extracted client implementation. No client_secret, captcha salts, device
spoofing profile or authentication bypass was copied. Initial migration attempts to
retrieve pinned upstream sources failed with connection resets; the login fix later
verified the official public Web protocol and pinned pikpaktui sources through its
public GitHub Contents endpoint. No live account success is claimed. See
docs/NATIVE_LOGIN_RESEARCH.md and docs/API_ADAPTER.md for evidence and limitations.
The compatibility protocol is not an official documented public API. Connected Apps
quota rules must not be applied to it without validation, nor does this route imply
permission to ignore account restrictions.

The owner supplied nekopak-ui-v3-src.zip, SHA-256
86be86d3e7ab594ec73e2f10fac841b14966eb84121659e771c894df70b03e27.
Frontend src/**, tests/unit/**, tests/e2e/run.mjs and icon-generation source were
imported and adapted for Rust IPC, native sign-in, honest capability gating and
lossless revisions. The original source provenance report is retained at
docs/UI_SOURCES_AND_DEPENDENCIES.md; no Piko implementation was copied.

Bundled frontend runtime: preact 10.27.2 (MIT), @tauri-apps/api 2.12.4 (MIT OR Apache-2.0).
Generated assets: selected Material Symbols Rounded 0.48.1 glyph paths (Google,
Apache-2.0; file header preserved), colors generated using material-color-utilities
0.4.0 (Apache-2.0). Source paths: src/components/icons.generated.ts,
src/styles/schemes.css, scripts/gen-icons.mjs. Full notices are included by packaging.

## Login usability fix (2026-11-08)

The native provider now contains public Web login protocol metadata (client ID and
SDK/protocol versions) independently transcribed from the browser-delivered PikPak
Web client. These are public software identifiers, not account credentials. Exact
URLs and asset SHA-256 values are in docs/NATIVE_LOGIN_RESEARCH.md. No proprietary
SDK implementation, Android secret, CAPTCHA-solving code or user credential was copied.

Windows transport uses native-tls/Schannel with system certificate validation;
other-target TLS dependencies remain in the broad Cargo inventory. Tauri plugin-opener
2.7.1 (MIT OR Apache-2.0) opens only permitted HTTPS URLs in the default application.
Its implementation is used as a normal dependency, not copied. No browser, CLI or
additional TLS runtime is redistributed beyond linked Rust dependencies and system APIs.

## DRIVE verification and file operations (2026-11-08)

The DRIVE adapter independently implements publicly delivered Web protocol metadata:
client/version/package identifiers, the published configuration timestamp and public
MD5-chain parameters. These are not private client secrets or human CAPTCHA answers.
The current account's own `sub` and device ID are kept in Rust; human checks remain
required whenever requested by the service. No official JS implementation was copied.
Exact asset URLs, hashes and independent vectors: docs/DRIVE_PROTOCOL_RESEARCH.md.

File/metadata/mutation/quota/download behavior was cross-checked against the official
public Web scripts and pinned Apache-2.0 pikpaktui / MIT Rclone source; implementations
were independently written. Original paths and blob hashes: docs/FILE_PROTOCOL_RESEARCH.md.
Earlier statements that no CAPTCHA salts were transcribed refer to the initial login
slice and Android identity; this update explicitly records the public Web parameters.

New dependency: md-5 0.10.6 (MIT OR Apache-2.0), used only for compatibility with this
public request metadata protocol, not for file integrity or password storage. File-block
integrity uses SHA-256. The dependency license inventory includes the actual lockfile.

## Desktop downloads, language and tray (2026-11-08)

Piko v1.4.0 at d61c1580783911be557d4c39026c300517187bcb was checked again as the
stable behavior reference for directory selection, speed sampling, directory opening,
theme selection, language and tray. No AGPL code or visual asset was copied.
Original paths, fixed source links and limitations: docs/PIKO_DOWNLOAD_UX_REFERENCE.md.
That record also audits PikPak Enhancement Master at 024b31b30e4c797258957862eb64fc2fa364bda9
and pikpak-kotlin 2.5.0 at 9187fafbe807695893e565d5e2b273b871a1c7c0 as read-only
protocol/behavior references; these supersede their earlier NOT_USED/research status.

Added normal dependencies: tauri-plugin-dialog 2.8.4 and tauri-plugin-fs 2.7.1
(MIT OR Apache-2.0), rfd 0.17.2 (MIT) and tray-icon 0.25.1 (MIT OR Apache-2.0). Exact resolved versions
and license texts are verified by the release inventory, including transitive crates.
No generic filesystem/shell capability was granted to the web UI; Rust validates
task-owned paths and native picker results. Desktop icon generation is a build tool,
not a new runtime component.

## Full desktop integration (2026-11-08)

The native MP4/MKV/TS parsers, indexes and stream-copy remuxer are independently
implemented in Rust. FFmpeg is an existing external test oracle only; no FFmpeg
library/executable/source is linked or distributed. Source/format boundaries:
crates/nekopak-media/README.md and docs/DESKTOP_INTEGRATION_REPORT.md.

Cloud/share/archive/upload/reference protocol facts are recorded at fixed URLs,
commits and hashes in docs/NATIVE_EXTENDED_PROTOCOLS.md,
docs/SHARED_MEDIA_PROTOCOL_RESEARCH.md and docs/REFERENCE_ARCHIVE_PROTOCOL_RESEARCH.md.
Public website scripts were researched, not copied as product source.
sha1 0.10.7, hmac 0.12.1, httpdate 1.0.3 and base64 0.22.1 are ordinary dependencies
under their crate licenses in the generated inventory. SHA1 is required for GCID
and OSS compatibility; local downloaded-block integrity continues using SHA256.

Windows platform bindings use windows 0.62.2 (MIT OR Apache-2.0). WinFsp is a user-
installed optional runtime, not redistributed. The native ABI was independently
implemented from pinned headers and behavioral documentation, with references in
docs/MOUNT_IMPLEMENTATION.md. Its GPLv3 FLOSS exception notice is preserved:
**WinFsp - Windows File System Proxy, Copyright (C) Bill Zissimopoulos**
https://github.com/winfsp/winfsp . Cloud Files is a distinct Windows system API.

The Python cryptography package is used only by the release signing workflow,
not bundled in the desktop application. Private signing material is never included.

## Phone interface and network recovery (2026-11-10)

The phone navigation, touch lists, transfer cards, Android adaptive/round/monochrome
cat-box icon layers and session-based video selection preview were independently
implemented using the existing Preact/Tauri components. No new runtime dependency,
Piko UI source or third-party bitmap was added. Video preview uses the system WebView
decoder; FFmpeg remains an external test oracle only.

Fixed API-domain routing was researched in MIT pikpak-kotlin commit
8eaf5587a883ed4ea9bcca7b43a9194118719e63, original files
src/commonMain/kotlin/io/github/nihildigit/pikpak/PikPakDomain.kt and
src/commonTest/kotlin/io/github/nihildigit/pikpak/PikPakDomainMockTest.kt.
Blob hashes, the MIT license record and actual verification limits are in
docs/NETWORK_RECOVERY_RESEARCH.md. This is protocol/behavior reference; Rust routing,
anonymous measurements, reconnect, credential preferences and retry scheduling were
written independently. Public client domains do not imply an official public API.

Version0.1.2 adds an independently written bounded bencode parser and torrent-file
import from the system picker. The original info bytes are hashed according to
BEP3 (https://www.bittorrent.org/beps/bep_0003.html); public Web file-import behavior
was researched without copying its implementation. Exact source hashes and the
fixed cached Web build limitation are in docs/TORRENT_IMPORT_RESEARCH.md.
No additional parser/runtime, BT peer engine or AGPL source was incorporated.
The existing SHA1 dependency supplies BT protocol identity; downloaded bytes still
use SHA256 verification. The download-worker, connection-reuse and Android IME
changes use existing Rust/Tauri/AndroidX APIs.

The optional Windows Setup.exe is produced by Tauri's NSIS bundler (build-time tooling).
It installs the same executable, notices and dependency license files as the portable ZIP.
NSIS is not launched as part of normal application operation. WebView2's Microsoft
bootstrapper is downloaded only when the installed system runtime is missing; the
full runtime is not part of NekoPak's Lite payload.
