// Signed APK upgrade and minified startup on the dedicated emulator only.
// Usage: node tests/e2e/android-release.mjs <current.apk> <baseline.apk>
// No account is entered. The isolated test app is reset before installing the baseline.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const [candidate, baseline] = process.argv.slice(2).map(path => resolve(path));
assert(candidate && baseline, "Pass the current signed APK and a previously signed baseline APK.");
const report = JSON.parse(readFileSync(candidate.replace(/\.apk$/, "-report.json"), "utf8"));
const pkg = "io.nekopak.desktop", serial = "emulator-5580";
assert.equal(report.application_id, pkg);
assert.equal(report.profile, "release");
assert.equal(report.debuggable, false);
assert.equal(report.signature_verified, true);
assert.equal(createHash("sha256").update(readFileSync(candidate)).digest("hex"), report.sha256);
assert.deepEqual(report.certificate_sha256, ["3c7f59b7dd1ae693aa5dd5309966a2eee83bc9055a650bb32d9c035780b588ba"]);
const adb = process.env.ANDROID_ADB ?? join(process.env.ANDROID_HOME ?? "", "platform-tools/adb.exe");
const output = resolve(process.env.NEKOPAK_RELEASE_E2E_DIR ?? join(root, "target/android-release-runtime"));
mkdirSync(output, { recursive: true });
const device = (...args) => execFileSync(adb, ["-s", serial, ...args], {
  encoding: "utf8", windowsHide: true, maxBuffer: 8 * 1024 * 1024,
});
assert.equal(device("emu", "avd", "name").split(/\r?\n/)[0].trim(), "nekopak-smoke-api35");
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const checks = [];
const dumpPaths = [];
const dumpRun = Date.now();
let bridgeRetries = 0;
const pass = name => { checks.push({ name, passed: true }); console.log(`PASS ${name}`); };
function screenshot(name) {
  writeFileSync(join(output, name), execFileSync(adb, ["-s", serial, "exec-out", "screencap", "-p"], {
    windowsHide: true, maxBuffer: 12 * 1024 * 1024,
  }));
}
function hierarchy() {
  const path = `/data/local/tmp/nekopak-release-window-${dumpRun}-${dumpPaths.length + 1}.xml`;
  dumpPaths.push(path);
  const result = device("shell", "uiautomator", "dump", path);
  assert(result.includes("dumped to:"), "Android accessibility hierarchy is not ready.");
  return device("shell", "cat", path);
}
async function waitForHierarchy(ready, description) {
  const deadline = Date.now() + 40000;
  let lastProblem = `${description} not visible`;
  while (Date.now() < deadline) {
    let xml;
    try { xml = hierarchy(); }
    catch (error) {
      bridgeRetries++;
      lastProblem = `Accessibility bridge not ready: ${error.status ?? error.message}`;
      await delay(500);
      continue;
    }
    if (ready(xml)) return xml;
    await delay(500);
  }
  throw new Error(`Timed out waiting for ${description}. ${lastProblem}`);
}
async function loginPage() {
  const xml = await waitForHierarchy(
    xml => xml.includes("登录 PikPak") && xml.includes("密码") && xml.includes("记住账号"),
    "minified signed APK Chinese login form",
  );
  assert(!/凭据库.{0,30}(失败|损坏|不可用)/.test(xml), "Release must not show a credential vault error.");
}
async function tapText(label) {
  const xml = await waitForHierarchy(xml => xml.includes(`text="${label}"`) || xml.includes(`content-desc="${label}"`), label);
  const node = [...xml.matchAll(/<node\b[^>]*>/g)].map(match => match[0]).find(node => node.includes(`text="${label}"`) || node.includes(`content-desc="${label}"`));
  const rect = /bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/.exec(node);
  assert(rect, `No bounds for ${label}`);
  device("shell", "input", "tap", String(Math.round((Number(rect[1]) + Number(rect[3])) / 2)), String(Math.round((Number(rect[2]) + Number(rect[4])) / 2)));
}

try {
  const installed = device("shell", "pm", "path", pkg).trim();
  if (installed) {
    device("shell", "am", "force-stop", pkg);
    assert.match(device("uninstall", pkg), /Success/);
  }
  assert.match(device("install", baseline), /Success/);
  const before = device("shell", "dumpsys", "package", pkg);
  const previousVersionCode = Number(before.match(/versionCode=(\d+)/)?.[1]);
  assert(previousVersionCode > 0 && previousVersionCode < Number(report.version_code));
  device("shell", "cmd", "locale", "set-app-locales", pkg, "--locales", "zh-CN");
  // -r must perform an update with the same certificate; there is no uninstall
  // between the baseline and candidate installations.
  assert.match(device("install", "-r", candidate), /Success/);
  const after = device("shell", "dumpsys", "package", pkg);
  assert(after.includes(`versionCode=${report.version_code}`));
  assert(after.includes(`versionName=${report.version}`));
  assert(device("shell", "cmd", "locale", "get-app-locales", pkg).includes("zh-CN"));
  pass("Signed baseline upgrades in place with the same identity and preserves app locale");

  for (const boot of [1, 2]) {
    device("shell", "am", "force-stop", pkg);
    device("shell", "am", "start", "-n", `${pkg}/.MainActivity`);
    await loginPage();
    const pid = device("shell", "pidof", pkg).trim();
    assert(/^\d+$/.test(pid));
    screenshot(`release-login-${boot}.png`);
    await delay(10000);
    assert.equal(device("shell", "pidof", pkg).trim(), pid);
    const errors = device("shell", "logcat", "-d", `--pid=${pid}`, "-s", "AndroidRuntime:E");
    assert(!errors.includes("FATAL EXCEPTION"), "Android runtime reported a fatal exception.");
    pass(`Minified ARM64 release cold boot ${boot}: native Chinese login and stable process`);
  }
  if (process.env.NEKOPAK_VERIFY_PUBLIC_UPDATE === "1") {
    await tapText("先看看");
    await tapText("我的");
    await tapText("关于");
    await tapText("检查更新");
    await waitForHierarchy(xml => xml.includes("当前通道没有更高版本。"), "production update source result in the minified APK");
    screenshot("release-public-update.png");
    pass("Minified ARM64 APK checks the public production preview source without an account");
  }
  device("shell", "am", "start", "-a", "android.settings.APPLICATION_DETAILS_SETTINGS", "-d", `package:${pkg}`);
  await waitForHierarchy(xml => xml.includes('text="NekoPak"') && xml.includes('package="com.android.settings"'), "NekoPak system app-info page");
  screenshot("release-app-info-icon.png");
  writeFileSync(join(output, "results.json"), JSON.stringify({
    date: new Date().toISOString(), scope: "Signed ARM64 APK on isolated API35 x86_64 emulator with ARM translation; no physical-device or account claim",
    version: report.version, previous_version_code: previousVersionCode, version_code: report.version_code,
    apk_sha256: report.sha256, certificate_sha256: report.certificate_sha256, checks, accessibility_bridge_retries: bridgeRetries,
    pending: ["physical ARM64 phone", "real account and CAPTCHA"],
  }, null, 2) + "\n");
} finally {
  device("shell", "am", "force-stop", pkg);
  device("shell", "cmd", "locale", "set-app-locales", pkg);
  if (dumpPaths.length) device("shell", "rm", "-f", ...dumpPaths);
}
