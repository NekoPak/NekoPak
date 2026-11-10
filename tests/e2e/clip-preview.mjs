// Actual system-browser video decoding with synthetic IPC and a real Range server.
// FFmpeg is a local test oracle only; it is never copied into NekoPak or an APK.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { chromium } from "playwright";
import { build } from "esbuild";

const root = process.cwd(), output = resolve(root, "target/clip-preview");
mkdirSync(output, { recursive: true });
const ffmpeg = process.env.NEKOPAK_TEST_FFMPEG ?? "C:/Users/elysia/AppData/Local/Programs/Python/Python312/Lib/site-packages/imageio_ffmpeg/binaries/ffmpeg-win-x86_64-v7.1.exe";
const video = resolve(output, "two-colors.mp4");
const oracle = spawnSync(ffmpeg, ["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=red:s=320x180:r=25:d=2", "-f", "lavfi", "-i", "color=c=blue:s=320x180:r=25:d=2", "-filter_complex", "[0:v][1:v]concat=n=2:v=1:a=0[v]", "-map", "[v]", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-g", "25", "-movflags", "+faststart", "-y", video], { windowsHide: true });
assert.equal(oracle.status, 0, String(oracle.stderr));
const media = readFileSync(video), ranges = [];
const server = createServer((req, res) => {
  if (req.url === "/harness.html") {res.setHeader("Content-Type","text/html");res.end(harness);return;}
  if (req.url === "/harness.js") {res.setHeader("Content-Type","application/javascript");res.end(readFileSync(resolve(output,"harness.js")));return;}
  if (req.url === "/styles.css") {res.setHeader("Content-Type","text/css");res.end(styles);return;}
  if (!req.url.startsWith("/synthetic-session/")) { res.writeHead(404); res.end(); return; }
  res.setHeader("Access-Control-Allow-Origin", "*");
  res.setHeader("Accept-Ranges", "bytes");
  res.setHeader("Content-Type", "video/mp4");
  const range = /^bytes=(\d+)-(\d*)$/.exec(req.headers.range ?? "");
  const start = range ? Number(range[1]) : 0, end = range?.[2] ? Number(range[2]) : media.length - 1;
  ranges.push(req.headers.range ?? "full");
  if (range) { res.statusCode = 206; res.setHeader("Content-Range", `bytes ${start}-${end}/${media.length}`); }
  res.setHeader("Content-Length", end - start + 1); res.end(media.subarray(start, end + 1));
});
await new Promise(ok => server.listen(0, "127.0.0.1", ok));
const mediaOrigin = `http://127.0.0.1:${server.address().port}`;
const browser = await chromium.launch({ channel: process.env.E2E_BROWSER_CHANNEL ?? (process.platform === "win32" ? "msedge" : undefined) });
const context = await browser.newContext({ viewport: { width: 390, height: 844 }, locale: "zh-CN", hasTouch: true });
const page = await context.newPage(), errors = [], checks = [];
page.on("pageerror", error => errors.push(error.message));
await page.addInitScript(({ mediaOrigin }) => {
  const calls = [], sessions = [];
  window.__clipFixture = { calls, sessions };
  window.__TAURI_INTERNALS__ = { invoke: async (command, args) => {
    calls.push({ command, args });
    if (command === "media_session_open") {
      const id = `session-${sessions.length + 1}`; sessions.push(id);
      return { session_id: id, url: `${mediaOrigin}/synthetic-session/${id}/content`, mime_type: "video/mp4" };
    }
    if (command === "media_session_close") return;
    throw new Error(`Unexpected fixture command ${command}`);
  } };
}, { mediaOrigin });
const harnessCode = `
import {render} from 'preact';
import {useState} from 'preact/hooks';
import {jsx} from 'preact/jsx-runtime';
import {ClipVideoPreview} from './src/components/ClipVideoPreview.tsx';
import {setLanguage} from './src/lib/i18n.ts';
import {resetSessionUi} from './src/state/session.ts';

setLanguage('zh-CN');
function Test(){const [state,setState]=useState({positionMs:500,startMs:500,endMs:3500,edge:'start',qualityId:'original'});window.__clipUpdate=patch=>setState(s=>({...s,...patch}));return jsx(ClipVideoPreview,{remoteId:'fixture-video',...state});}
window.__clipReset=resetSessionUi;window.__clipUnmount=()=>render(null,document.querySelector('#app'));render(jsx(Test,{}),document.querySelector('#app'));
`;
await build({stdin:{contents:harnessCode,resolveDir:root,sourcefile:"clip-harness.tsx",loader:"tsx"},bundle:true,format:"esm",platform:"browser",jsx:"automatic",jsxImportSource:"preact",outfile:resolve(output,"harness.js"),define:{"import.meta.env":"{}"}});
const styles=["schemes","tokens","base","components","features"].map(n=>readFileSync(resolve(root,`src/styles/${n}.css`),"utf8")).join("\n");
const harness=`<html><head><meta name="viewport" content="width=device-width,initial-scale=1"/><link rel="stylesheet" href="/styles.css"/></head><body><div id="app"></div><script type="module" src="/harness.js"></script></body></html>`;
async function sample() {
  return page.locator("video").evaluate(video => {
    const canvas = document.createElement("canvas"); canvas.width = 1; canvas.height = 1;
    const ctx = canvas.getContext("2d"); ctx.drawImage(video, 0, 0, 1, 1);
    return { time: video.currentTime, pixel: [...ctx.getImageData(0, 0, 1, 1).data], paused: video.paused };
  });
}
const pass = name => { checks.push({ name, passed: true }); console.log(`PASS ${name}`); };
try {
  await page.goto(`${mediaOrigin}/harness.html`, {waitUntil:"domcontentloaded"});
  await page.waitForFunction(() => document.querySelector("video")?.readyState >= 2);
  await page.waitForFunction(() => Math.abs(document.querySelector("video").currentTime - 0.5) < 0.1);
  const red = await sample(); assert(red.pixel[0] > 180 && red.pixel[2] < 80, JSON.stringify(red));
  await page.screenshot({ path: resolve(output, "selected-start-red.png") });
  pass("actual selected start frame decodes red");
  await page.evaluate(() => window.__clipUpdate({ positionMs: 3000, edge: "end" }));
  await page.waitForFunction(() => Math.abs(document.querySelector("video").currentTime - 3) < 0.1);
  const blue = await sample(); assert(blue.pixel[2] > 180 && blue.pixel[0] < 80, JSON.stringify(blue));
  await page.screenshot({ path: resolve(output, "selected-end-blue.png") });
  pass("changing selected edge seeks actual decoded blue frame");
  await page.evaluate(() => window.__clipUpdate({ positionMs: 2500, startMs: 2500, endMs: 3500, edge: "start" }));
  await page.waitForTimeout(150);
  await page.getByRole("button", { name: "播放预览", exact: true }).click();
  await page.waitForFunction(() => { const v = document.querySelector("video"); return v.paused && v.currentTime >= 3.45; }, { timeout: 6000 });
  const end = await sample(); assert(end.time < 3.7); pass("selection playback stops at selected end");
  await page.evaluate(() => window.__clipUpdate({ qualityId: "720p" }));
  await page.waitForFunction(() => window.__clipFixture.sessions.length === 2);
  await page.waitForFunction(() => window.__clipFixture.calls.some(c => c.command === "media_session_close" && c.args.session_id === "session-1"));
  pass("quality switch closes previous Rust media session");
  await page.evaluate(() => window.__clipReset());
  await page.waitForFunction(() => !document.querySelector("video"));
  await page.waitForFunction(() => window.__clipFixture.calls.some(c => c.command === "media_session_close" && c.args.session_id === "session-2"));
  assert.equal(await page.evaluate(() => window.__clipFixture.sessions.length), 2);
  pass("account reset revokes preview without reopening old source");
  await page.evaluate(() => window.__clipUnmount());
  assert(ranges.some(r => r.startsWith("bytes=")), "browser must read real Range bytes");
  assert.deepEqual(errors, []);
  writeFileSync(resolve(output, "results.json"), JSON.stringify({ scope: "Real H264 system-browser decoding with synthetic IPC and loopback Range server; no account or bundled decoder", checks, errors, rangeRequests: ranges.length }, null, 2) + "\n");
} finally {
  await browser.close(); server.closeAllConnections(); await new Promise(ok => server.close(ok));
}
