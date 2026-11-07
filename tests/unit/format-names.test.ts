import { describe, expect, it } from "vitest";
import { addU64, compareU64, formatBytes, formatBytesExact, formatEta, parseU64, percent } from "../../src/lib/format";
import { splitName, validateName, windowsIssues, fileKind } from "../../src/lib/names";

describe("u64 handling (03_CONTRACTS §1: decimal strings, no precision loss)", () => {
  it("parses valid u64 and rejects garbage / overflow", () => {
    expect(parseU64("0")).toBe(0n);
    expect(parseU64("18446744073709551615")).toBe(18446744073709551615n);
    expect(parseU64("18446744073709551616")).toBeNull();
    expect(parseU64("-1")).toBeNull();
    expect(parseU64("1.5")).toBeNull();
    expect(parseU64(undefined)).toBeNull();
  });

  it("keeps 2^53+1 exact", () => {
    expect(formatBytesExact("9007199254740993")).toBe("9,007,199,254,740,993 字节");
    expect(compareU64("9007199254740993", "9007199254740992")).toBe(1);
    expect(addU64(["9007199254740993", "1"])).toBe(9007199254740994n);
  });

  it("formats sizes with Windows-style base-1024 units", () => {
    expect(formatBytes("0")).toBe("0 B");
    expect(formatBytes("1023")).toBe("1023 B");
    expect(formatBytes("1024")).toBe("1.00 KB");
    expect(formatBytes("1503238554")).toBe("1.40 GB");
    expect(formatBytes(String(1024n ** 4n))).toBe("1.00 TB");
    expect(formatBytes("9007199254740993")).toBe("8.00 PB");
    expect(formatBytes("not-a-number")).toBe("—");
  });

  it("never invents a percentage", () => {
    expect(percent("50", undefined)).toBeNull();
    expect(percent("50", "0")).toBeNull();
    expect(percent("1", "3")).toBe(33.3);
    expect(percent("5", "3")).toBe(100);
    expect(formatEta("10", undefined, 100)).toBeNull();
    expect(formatEta("10", "1000", undefined)).toBeNull();
    expect(formatEta("0", "600", 10)).toBe("1 分 0 秒");
  });
});

describe("names", () => {
  it("keeps the extension for middle ellipsis", () => {
    expect(splitName("片段.mp4")).toEqual({ base: "片段", ext: ".mp4" });
    expect(splitName(".gitignore")).toEqual({ base: ".gitignore", ext: "" });
    expect(splitName("末尾句点.")).toEqual({ base: "末尾句点.", ext: "" });
    expect(splitName("a.tar.zst")).toEqual({ base: "a.tar", ext: ".zst" });
  });

  it("flags Windows-hostile names", () => {
    expect(windowsIssues("CON")).toContain("Windows 保留设备名");
    expect(windowsIssues("nul.txt")).toContain("Windows 保留设备名");
    expect(windowsIssues("COM1.log")).toContain("Windows 保留设备名");
    expect(windowsIssues("末尾句点.")).toContain("以空格或句点结尾");
    expect(windowsIssues("a?b")).toHaveLength(1);
    expect(windowsIssues("café")).toContain("含组合字符（NFD 形式）");
    expect(windowsIssues("普通文件.txt")).toEqual([]);
  });

  it("validates new names against siblings (exact, NFC and case)", () => {
    const sib = ["Report.pdf", "café.txt"];
    expect(validateName("", sib).error).toBeTruthy();
    expect(validateName("a/b", sib).error).toContain("/");
    expect(validateName("..", sib).error).toBeTruthy();
    expect(validateName("Report.pdf", sib).error).toBe("此文件夹中已有同名项");
    expect(validateName("café.txt", sib).error).toBe("此文件夹中已有同名项");
    const ci = validateName("report.pdf", sib);
    expect(ci.error).toBeUndefined();
    expect(ci.warnings.join()).toContain("仅大小写不同");
    expect(validateName("x".repeat(86) + "汉", sib).error).toBeUndefined();
    expect(validateName("汉".repeat(86), sib).error).toContain("255");
    expect(validateName("Report.pdf", sib, "Report.pdf").error).toBe("新名称与原名称相同");
  });

  it("classifies kinds by mime then extension", () => {
    expect(fileKind("x.MKV", "file")).toBe("video");
    expect(fileKind("x", "file", "image/heic")).toBe("image");
    expect(fileKind("dir.mp4", "folder")).toBe("folder");
  });
});
