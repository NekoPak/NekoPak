/**
 * Deterministic mock fixtures (no network, no randomness without a seed).
 * Seven demo groups from 03_CONTRACTS §6 plus file names that exercise
 * CJK, emoji/ZWJ, spaces, very long names, Windows reserved names,
 * NFC/NFD composition, RTL text and > 2^53 sizes.
 */
import type { RemoteEntry } from "../contract";
import type { TransferTaskExt } from "../contract-proposals";
import { ROOT_ID } from "../contract-proposals";

export type ScenarioId =
  | "free" | "premium" | "unauthorized" | "quota_exhausted"
  | "network_down" | "huge_dir" | "name_conflict";

export interface ScenarioInfo {
  id: ScenarioId;
  label: string;
  description: string;
}

export const SCENARIOS: ScenarioInfo[] = [
  { id: "free", label: "免费账户", description: "已登录 Free，Connected Apps 每日下行 5 GB 上限" },
  { id: "premium", label: "Premium", description: "已登录 Premium，月度配额可读取" },
  { id: "unauthorized", label: "未授权", description: "未登录，走首次授权流程" },
  { id: "quota_exhausted", label: "限额耗尽", description: "下行与云下载配额用尽，列表与历史仍可查看" },
  { id: "network_down", label: "网络中断", description: "远端请求返回 TRANSIENT_NETWORK，任务进入重试" },
  { id: "huge_dir", label: "目录超大", description: "根目录 20,000 项，分页较慢" },
  { id: "name_conflict", label: "名称冲突", description: "重名、大小写与 NFC/NFD 冲突，下载到同名目标失败" },
];

export interface FaultFlags {
  /** Downloads crossing 37% lose the network, retry, then continue. */
  drop_at_37: boolean;
  /** Emits stale (older revision) progress events after newer ones. */
  out_of_order: boolean;
  /** Verification fails at 100% → INTEGRITY_FAILED. */
  integrity_fail: boolean;
  /** Next remote call fails with AUTH_EXPIRED. */
  auth_expired: boolean;
  /** Each command waits this long (ms) before answering. */
  latency_ms: number;
}

export const DEFAULT_FAULTS: FaultFlags = {
  drop_at_37: false,
  out_of_order: false,
  integrity_fail: false,
  auth_expired: false,
  latency_ms: 120,
};

/* ------------------------------------------------------------------ */

export function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const BASE_TIME = Date.parse("2026-11-07T09:25:00Z");
const iso = (minutesAgo: number) => new Date(BASE_TIME - minutesAgo * 60_000).toISOString();

const GiB = 1024n ** 3n;
const MiB = 1024n ** 2n;

let seq = 0;
function file(parent: string, name: string, size: bigint, minutesAgo: number, extra: Partial<RemoteEntry> = {}): RemoteEntry {
  seq++;
  return {
    id: extra.id ?? `f-${parent}-${seq}`,
    parent_id: parent,
    name,
    kind: "file",
    size_bytes: size.toString(),
    modified_at: iso(minutesAgo),
    etag: `"e${seq.toString(16)}"`,
    ...extra,
  };
}
function folder(parent: string, id: string, name: string, minutesAgo: number): RemoteEntry {
  return { id, parent_id: parent, name, kind: "folder", size_bytes: "0", modified_at: iso(minutesAgo) };
}

/** Folder ids with special mock behaviour. */
export const SPECIAL = {
  readOnlyShare: "d-shared-ro",
  vanished: "d-vanished",
  huge: "d-huge",
  courses: "d-courses",
  movies: "d-movies",
  photos: "d-photos",
  projects: "d-projects",
  docs: "d-docs",
  empty: "d-empty",
} as const;

export const LONG_NAME =
  "这是一个非常非常长的文件名用于测试列表中的截断行为以及悬停提示是否完整显示 — A very long file name to verify middle ellipsis keeps the extension visible (final, v12, 2026-11-07).tar.zst";

export function buildTree(scenario: ScenarioId): RemoteEntry[] {
  seq = 0;
  const R = ROOT_ID;
  const out: RemoteEntry[] = [
    folder(R, SPECIAL.docs, "我的资料", 30),
    folder(R, SPECIAL.movies, "影视", 300),
    folder(R, SPECIAL.projects, "项目 Projects", 60 * 26),
    folder(R, SPECIAL.photos, "照片 📷 2026", 60 * 24 * 3),
    folder(R, SPECIAL.readOnlyShare, "共享给我（只读）", 60 * 24 * 9),
    folder(R, SPECIAL.vanished, "已在其他设备删除的文件夹", 60 * 24 * 12),
    folder(R, SPECIAL.huge, "超大目录（12,000 项）", 60 * 24 * 20),
    folder(R, SPECIAL.empty, "空文件夹", 60 * 24 * 40),

    file(R, "片段.mp4", 1503238554n, 12, { mime_type: "video/mp4", hash: { algorithm: "sha1", value: "3f786850e387550fdab836ed7e6dc881de23001b" } }),
    file(R, "🐱 猫猫图鉴 🐾.png", 2_481_337n, 95, { mime_type: "image/png" }),
    file(R, "👩‍💻 开发日志.md", 18_204n, 140),
    file(R, "日本語のファイル名.txt", 3_210n, 600),
    file(R, "한국어 파일.docx", 88_120n, 720),
    file(R, "تقرير سنوي ٢٠٢٦.pdf", 4_120_331n, 1440),
    file(R, "café (NFC).txt", 512n, 2000),
    file(R, "cafe\u0301 (NFD).txt", 512n, 2001),
    file(R, "CON", 0n, 3000),
    file(R, "NUL.txt", 64n, 3001),
    file(R, "aux.log", 2_048n, 3002),
    file(R, "末尾有空格 ", 10n, 3003),
    file(R, "末尾句点.", 10n, 3004),
    file(R, "   前导空格.txt", 10n, 3005),
    file(R, LONG_NAME, 734_003_200n, 3100),
    file(R, "稀疏 1TB 测试镜像.img", 1024n * GiB, 4000),
    file(R, "精度测试 2^53+1 字节.bin", 9007199254740993n, 4100),
    file(R, "空文件.txt", 0n, 4200),
    file(R, "Report.pdf", 120_334n, 5000),
    file(R, "report.pdf", 120_990n, 5001),

    // 我的资料
    folder(SPECIAL.docs, SPECIAL.courses, "2026 课程", 45),
    file(SPECIAL.docs, "线性代数笔记.pdf", 12n * MiB + 33_221n, 50, { hash: { algorithm: "md5", value: "9e107d9d372bb6826bd81d3542a419d6" } }),
    file(SPECIAL.docs, "实验报告.docx", 2n * MiB + 4096n, 70),
    file(SPECIAL.docs, "C 语言程序设计 习题集.zip", 220n * MiB, 300),
    file(SPECIAL.courses, "量子信息导论 第1讲.mp4", 1_610_612_736n, 80, { mime_type: "video/mp4" }),
    file(SPECIAL.courses, "量子信息导论 第2讲.mp4", 1_702_887_424n, 79, { mime_type: "video/mp4" }),
    file(SPECIAL.courses, "微积分 期中复习.pdf", 5n * MiB, 200),
    file(SPECIAL.courses, "中国近现代史纲要 小组展示.pptx", 38n * MiB, 260),

    // 影视
    file(SPECIAL.movies, "4K 演示 — HDR10 (2160p).mkv", 58n * GiB + 123_456_789n, 310, { mime_type: "video/x-matroska" }),
    file(SPECIAL.movies, "纪录片 S01E01.mp4", 2n * GiB, 900, { mime_type: "video/mp4" }),
    file(SPECIAL.movies, "纪录片 S01E02.mp4", 2n * GiB + 7n * MiB, 899, { mime_type: "video/mp4" }),
    file(SPECIAL.movies, "原声带 Disc 1.flac", 410n * MiB, 1500),

    // 项目
    file(SPECIAL.projects, "NekoPak 设计稿 v0.1.fig", 18n * MiB, 120),
    file(SPECIAL.projects, "Cargo.toml", 1_240n, 125),
    file(SPECIAL.projects, "超大压缩包.zip", 96n * GiB, 4000),

    // 照片
    ...Array.from({ length: 36 }, (_, i) =>
      file(SPECIAL.photos, `IMG_${String(2026_1101 + i)}_${String(i % 24).padStart(2, "0")}3012.heic`, BigInt(2_000_000 + i * 73_211), 60 * 24 * 3 + i, { mime_type: "image/heic" }),
    ),

    // 只读共享
    file(SPECIAL.readOnlyShare, "社团活动安排.xlsx", 64_000n, 60 * 24 * 9),
    file(SPECIAL.readOnlyShare, "合唱排练.mp3", 7n * MiB, 60 * 24 * 9),
  ];

  if (scenario === "name_conflict") {
    out.push(
      file(R, "合同.pdf", 300_100n, 10, { id: "f-dup-a" }),
      file(R, "合同.pdf", 300_222n, 11, { id: "f-dup-b" }),
      file(R, "Photo.JPG", 1_400_000n, 12),
      file(R, "photo.jpg", 1_400_120n, 13),
    );
  }

  const rand = mulberry32(42);
  const exts = [".mp4", ".pdf", ".jpg", ".zip", ".txt", ".flac", ".docx", ".rs"];
  const genHuge = (parent: string, count: number, prefix: string) => {
    for (let i = 0; i < count; i++) {
      const ext = exts[Math.floor(rand() * exts.length)];
      const size = BigInt(Math.floor(rand() * 4_000_000_000));
      out.push(file(parent, `${prefix} ${String(i + 1).padStart(5, "0")}${ext}`, size, Math.floor(rand() * 500_000), { id: `h-${parent}-${i}` }));
    }
  };
  genHuge(SPECIAL.huge, 12_000, "批量样本");
  if (scenario === "huge_dir") genHuge(R, 20_000, "根目录样本");

  return out;
}

export function buildTrash(): RemoteEntry[] {
  return [
    { id: "t-old-1", parent_id: ROOT_ID, name: "旧版本 安装包.exe", kind: "file", size_bytes: String(88n * MiB), modified_at: iso(60 * 24 * 2) },
    { id: "t-old-2", parent_id: SPECIAL.docs, name: "草稿（已废弃）", kind: "folder", size_bytes: "0", modified_at: iso(60 * 24 * 5) },
    { id: "t-old-3", parent_id: SPECIAL.movies, name: "重复下载 (1).mp4", kind: "file", size_bytes: String(GiB + 5n * MiB), modified_at: iso(60 * 24 * 8) },
  ];
}

export function buildTasks(now: number): TransferTaskExt[] {
  const t = (minutesAgo: number) => new Date(now - minutesAgo * 60_000).toISOString();
  const base = { backend: "mock" as const };
  return [
    { ...base, id: "task-1", remote_id: "f-root-9", name: "片段.mp4", kind: "download", state: "running", transferred_bytes: "345744867", total_bytes: "1503238554", speed_bytes_per_sec: 6_400_000, destination: "%USERPROFILE%\\Downloads\\NekoPak\\片段.mp4", created_at: t(6), updated_at: t(0), revision: 10 },
    { ...base, id: "task-2", remote_id: "f-dup-x", name: "量子信息导论 第2讲.mp4", kind: "download", state: "queued", transferred_bytes: "0", total_bytes: "1702887424", destination: "%USERPROFILE%\\Downloads\\NekoPak\\量子信息导论 第2讲.mp4", created_at: t(5), updated_at: t(5), revision: 1 },
    { ...base, id: "task-3", remote_id: "f-movie-4k", name: "4K 演示 — HDR10 (2160p).mkv", kind: "download", state: "paused", transferred_bytes: "38135713381", total_bytes: "62400000000", destination: "D:\\Media\\4K 演示 — HDR10 (2160p).mkv", created_at: t(180), updated_at: t(150), revision: 40 },
    { ...base, id: "task-4", remote_id: "f-sparse", name: "稀疏 1TB 测试镜像.img", kind: "download", state: "running", transferred_bytes: "87960930222", total_bytes: "1099511627776", speed_bytes_per_sec: 11_800_000, destination: "E:\\Images\\稀疏 1TB 测试镜像.img", created_at: t(400), updated_at: t(0), revision: 900 },
    { ...base, id: "task-5", remote_id: "f-bigzip", name: "超大压缩包.zip", kind: "download", state: "failed", transferred_bytes: "41231686041", total_bytes: "103079215104", destination: "C:\\Users\\Public\\超大压缩包.zip", created_at: t(900), updated_at: t(880), revision: 77, error: { code: "DISK_FULL", message: "目标磁盘 C: 剩余空间不足（需要 57.6 GB）", retryable: true, backend: "mock", request_id: "mock-req-5d1e" } },
    { ...base, id: "task-6", remote_id: "cloud-ubuntu", name: "Ubuntu 26.04 Desktop amd64.iso", kind: "cloud_download", state: "running", transferred_bytes: "1932735283", total_bytes: "6442450944", destination: "/影视", created_at: t(20), updated_at: t(0), revision: 30 },
    { ...base, id: "task-7", remote_id: "cloud-dead", name: "magnet:?xt=urn:btih:<已隐藏>", kind: "cloud_download", state: "failed", transferred_bytes: "0", destination: "/", created_at: t(1500), updated_at: t(1490), revision: 3, error: { code: "NOT_FOUND", message: "资源已失效或没有可用来源", retryable: false, backend: "mock", request_id: "mock-req-91aa" } },
    { ...base, id: "task-8", remote_id: "f-doc-2", name: "实验报告.docx", kind: "upload", state: "completed", transferred_bytes: "2101248", total_bytes: "2101248", destination: "/我的资料", created_at: t(2000), updated_at: t(1999), revision: 12 },
    { ...base, id: "task-9", remote_id: "f-doc-1", name: "线性代数笔记.pdf", kind: "download", state: "completed", transferred_bytes: "12616389", total_bytes: "12616389", destination: "%USERPROFILE%\\Downloads\\NekoPak\\线性代数笔记.pdf", created_at: t(3000), updated_at: t(2990), revision: 22 },
    { ...base, id: "task-10", remote_id: "f-old", name: "纪录片 S01E01.mp4", kind: "download", state: "cancelled", transferred_bytes: "104857600", total_bytes: "2147483648", destination: "D:\\Media\\纪录片 S01E01.mp4", created_at: t(5000), updated_at: t(4990), revision: 8 },
    { ...base, id: "task-11", remote_id: "f-share", name: "社团活动安排.xlsx", kind: "upload", state: "failed", transferred_bytes: "0", total_bytes: "64000", destination: "/共享给我（只读）", created_at: t(6000), updated_at: t(6000), revision: 2, error: { code: "PERMISSION_DENIED", message: "目标文件夹为只读共享", retryable: false, backend: "mock", request_id: "mock-req-0c3f" } },
  ];
}
