# Native media engine

`native::{probe, prepare, prepare_format, render}` uses immutable, account-scoped `RangeSource`
objects supplied by Rust Core. This module has no HTTP client, credentials, URLs,
OS player launcher, codec runtime, or external media process in its native path.

`probe` returns `MediaIndex` with serializable `MediaInfo` and track metadata.
`prepare` turns a millisecond `ClipRange` into a serializable `ClipPlan`. The
plan's public summary includes requested/aligned start, end, output container,
duration, bytes and warnings; private fields contain new container bytes and
validated source spans. Persist the full plan inside Rust, never send it through
UI IPC. `MediaIndex::plan` is the synchronous MP4-only convenience variant.

`render` writes to a caller-owned **private temporary** `File`. It checks the plan
again after deserialization, checks source identity and exact read lengths, and
observes cancellation between bounded reads. The caller owns final flush,
verification, atomic no-overwrite publication, task persistence and account
changes. Progress reports `(output_bytes_written, output_bytes_total)`.
Pause/restart re-renders from the beginning using any verified source-range
cache; this does not claim resumable container muxing.

## Native containers

- **MP4:** bounded top-level headers and `moov`; `stts`, `ctts` v0/v1, `stsc`,
  `stsz`, `stco`/`co64`, `stss`, and simple edit-list timelines. Chooses a real
  preceding video sync sample, retains every supported audio track/language,
  reconstructs fast-start tables and uses 64-bit chunk offsets. Export is MP4.
- **Matroska/WebM:** EBML, SeekHead, Info, Tracks, Cues and bounded cluster/block
  header inspection. Rewrites cluster/block timestamps, preserves selected
  payload order and track metadata, and exports Matroska. Chapters and attachments
  are explicitly omitted. The output currently has no regenerated Cues index.
- **MPEG-TS:** packet boundaries, PAT/PMT section CRC, stream PID/codec mapping,
  real PES presentation timestamps and random-access/IDR indicators. Sparse
  timestamp seeking plus bounded backwards GOP inspection finds the actual
  video start. Output remains TS with its nonzero source timestamps preserved.

MP4/MKV/TS cropping is stream copy and starts at the previous keyframe, not
frame-exact re-encoding. `prepare_format(..., "mp4")` additionally demuxes TS
PES/ADTS/Annex-B or indexed Matroska blocks and reconstructs a fast-start MP4.
It preserves H.264/HEVC configuration, composition offsets, all AAC tracks and
language metadata. Delayed audio is represented using an empty edit. TS AVC/HEVC
NAL data is converted to length prefixes, and AAC ADTS headers are removed.
Matroska laced audio is separated into samples; its default video duration is
required to construct monotonic decode timestamps. Payloads remain source spans
with short container/length prefixes rather than an in-memory complete movie.
The native path does not bundle or launch FFmpeg.

A full-quality MP4 remux necessarily scans the full chosen representation; a
clip scans only the bounded selected interval. A caller should reuse its range
cache between preparation and rendering. A different container is never obtained
by merely renaming the output suffix.

Unsupported inputs fail explicitly, including fragmented MP4, multiple video
tracks, unsupported audio/container combinations, complex edit lists, encrypted
media, Matroska without Cues, content-encoded Matroska tracks, multi-packet TS
program tables, multiple TS programs, or a TS GOP outside the bounded scan.
TS-to-MP4 supports AAC audio with complete ADTS frames in a PES; unsupported audio,
split ADTS frames, changing parameter sets/configuration and multiple raw blocks
fail explicitly. Matroska-to-MP4 currently supports AVC/HEVC video plus AAC,
requires default video duration and rejects unmapped subtitle/data tracks rather
than silently discarding them. General VFR Matroska and exact subtitle conversion
remain outside that remux path.
These remain implementation limitations, not completed upstream parity.

## Executed validation (2026-11-08, Windows x64)

- `cargo clippy -p nekopak-media --all-targets --offline -- -D warnings`: passed.
- `cargo test -p nekopak-media --lib --offline`: 11 passed.
- With explicit `NEKOPAK_TEST_FFMPEG` and reviewed
  `NEKOPAK_TEST_FFMPEG_SHA256`,
  `cargo test -p nekopak-media --test native_integration --offline -- --include-ignored`:
  8 passed, including all normally ignored fixture/oracle cases.

The oracle is the existing imageio_ffmpeg FFmpeg 7.1 Windows executable, SHA-256
`2ce797a0f88d7f067180338fb227f7b1928ea727bd9a4d7a1d022f7c52af71a3`.
It generates synthetic video/audio and decodes Rust outputs only; it is not
copied, linked or redistributed. Tests cover H.264/HEVC, B frames, positive and
negative composition offsets, two AAC tracks with English/Japanese metadata,
frame hashes compared with the correct source interval, TS/MKV-to-MP4 remuxing,
whole-TS remux retaining all 300 video frames, exact output length,
range budgets below full-file size, cancellation, changed source/plan bounds,
malformed sizes/counts/headers and bounded long-GOP failure. They do not stand in
for arbitrary-media compatibility, complete audio timeline proof, or real
PikPak account/service acceptance.

The older `render_clip` API remains a separate, explicit external-FFmpeg local
experiment for compatibility with existing tests. It is not the native path.
All new parser/muxer implementation is independent code in this repository.
Piko was used only as the product behavior reference documented in
`docs/UPSTREAM_PARITY_RESEARCH.md`; no AGPL implementation or assets were copied.
