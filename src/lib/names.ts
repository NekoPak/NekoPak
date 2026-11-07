/** File-name helpers: display splitting, kinds, and validation for new names. */

export function splitName(name: string): { base: string; ext: string } {
  const dot = name.lastIndexOf(".");
  // Keep short extensions visible; ignore dotfiles and "name." endings.
  if (dot <= 0 || dot === name.length - 1) return { base: name, ext: "" };
  const ext = name.slice(dot);
  if (ext.length > 12 || /\s/.test(ext)) return { base: name, ext: "" };
  return { base: name.slice(0, dot), ext };
}

export type FileKind = "folder" | "video" | "audio" | "image" | "doc" | "archive" | "code" | "other";

const EXT_KIND: Record<string, FileKind> = {};
const assign = (kind: FileKind, exts: string) => exts.split(" ").forEach((e) => (EXT_KIND[e] = kind));
assign("video", "mp4 mkv mov avi webm m4v ts flv wmv");
assign("audio", "mp3 flac wav aac ogg m4a opus");
assign("image", "png jpg jpeg gif webp heic bmp svg avif");
assign("doc", "pdf doc docx xls xlsx ppt pptx txt md rtf odt csv epub");
assign("archive", "zip 7z rar tar gz zst xz iso img dmg bz2");
assign("code", "rs ts js json toml yaml yml py c h cpp kt go sh fig");

export function fileKind(name: string, kind: "file" | "folder", mime?: string): FileKind {
  if (kind === "folder") return "folder";
  if (mime) {
    if (mime.startsWith("video/")) return "video";
    if (mime.startsWith("audio/")) return "audio";
    if (mime.startsWith("image/")) return "image";
  }
  const { ext } = splitName(name);
  return EXT_KIND[ext.slice(1).toLowerCase()] ?? "other";
}

export const KIND_LABEL: Record<FileKind, string> = {
  folder: "文件夹",
  video: "视频",
  audio: "音频",
  image: "图片",
  doc: "文档",
  archive: "压缩包/镜像",
  code: "代码/设计",
  other: "其他",
};

const WIN_RESERVED = /^(con|prn|aux|nul|com[0-9¹²³]|lpt[0-9¹²³])(\..*)?$/i;
const WIN_BAD_CHARS = /[<>:"\\|?*]/;
// eslint-disable-next-line no-control-regex
const CONTROL_CHARS = /[\u0000-\u001f\u007f]/;

/** Issues that only matter when the remote name is materialised on Windows. */
export function windowsIssues(name: string): string[] {
  const issues: string[] = [];
  if (WIN_RESERVED.test(name.trim())) issues.push("Windows 保留设备名");
  if (WIN_BAD_CHARS.test(name)) issues.push("含 Windows 不允许的字符 < > : \" \\ | ? *");
  if (/[ .]$/.test(name)) issues.push("以空格或句点结尾");
  if (name !== name.normalize("NFC")) issues.push("含组合字符（NFD 形式）");
  return issues;
}

export interface NameCheck {
  error?: string;
  warnings: string[];
}

export function utf8Length(s: string): number {
  return new TextEncoder().encode(s).length;
}

/**
 * Validate a new remote name. Hard errors block submit; warnings explain
 * how a later Windows download will need to escape the name.
 */
export function validateName(name: string, siblings: readonly string[], current?: string): NameCheck {
  const warnings: string[] = [];
  if (name.trim().length === 0) return { error: "名称不能为空", warnings };
  if (name === "." || name === "..") return { error: "不能使用 . 或 ..", warnings };
  if (name.includes("/")) return { error: "名称不能包含 /", warnings };
  if (CONTROL_CHARS.test(name)) return { error: "名称不能包含控制字符", warnings };
  if (utf8Length(name) > 255) return { error: "名称超过 255 字节（UTF-8）", warnings };
  if (current !== undefined && name === current) return { error: "新名称与原名称相同", warnings };

  const nfc = name.normalize("NFC");
  for (const s of siblings) {
    if (s === current) continue;
    if (s === name || s.normalize("NFC") === nfc) {
      return { error: "此文件夹中已有同名项", warnings };
    }
    if (s.toLowerCase() === name.toLowerCase()) {
      warnings.push(`与“${s}”仅大小写不同，Windows 本地会视为同名`);
    }
  }
  for (const issue of windowsIssues(name)) warnings.push(`${issue}，下载到 Windows 时将被转义`);
  return { warnings };
}
