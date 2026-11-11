// Local throughput regression: actual Tauri/Rust pipeline versus identical HTTP bytes.
// Synthetic server only; no account, saved credentials or external hosts.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { chromium } from "playwright";

const root = resolve(import.meta.dirname, "../..");
const directory = join(root, "target", `throughput-${randomUUID()}`);
const destination = join(directory, "downloads");
mkdirSync(destination, { recursive: true });
const bytes = Buffer.alloc(16 * 1024 * 1024, 73), sha256 = createHash("sha256").update(bytes).digest("hex");
let origin, metadataRequests = 0, rangeRequests = 0;
const sockets = new Set(), rangeTimes = [], violations = [];
const file = {id:"speed-file",parent_id:"",name:"speed.bin",kind:"drive#file",size:String(bytes.length),hash:"synthetic-stable-gcid",modified_time:"2026-11-10T00:00:00Z",phase:"PHASE_TYPE_COMPLETE",trashed:false,tags:[]};
const server = createServer(async (request, response) => {
  try {
    const url = new URL(request.url, origin);
    const json = value => { response.setHeader("content-type", "application/json"); response.end(JSON.stringify(value)); };
    if (url.pathname === "/v1/shield/captcha/init") return json({captcha_token:"fixture",expires_in:300});
    if (url.pathname === "/v1/auth/signin") return json({access_token:"fixture",refresh_token:"fixture-refresh",token_type:"Bearer",expires_in:3600,sub:"speed-fixture"});
    if (url.pathname === "/v1/user/me") return json({sub:"speed-fixture"});
    if (url.pathname === "/drive/v1/files/speed-file") {
      metadataRequests++; await new Promise(resolve => setTimeout(resolve, 40));
      return json({...file,web_content_link:`${origin}/content`});
    }
    if (url.pathname === "/drive/v1/files") return json({files:[file]});
    if (url.pathname === "/drive/v1/about") return json({quota:{limit:"1000000000",usage:"0"}});
    if (url.pathname === "/vip/v1/quantity/list") return json({base:{download:{total_assets:"1000000000",assets:"0"}}});
    if (url.pathname === "/drive/v1/privilege/vip") return json({data:[]});
    if (url.pathname === "/content") {
      assert.equal(request.headers.authorization, undefined);
      assert.equal(request.headers["x-captcha-token"], undefined);
      const range = /^bytes=(\d+)-(\d+)$/.exec(request.headers.range ?? "");
      const start = range ? Number(range[1]) : 0, end = range ? Number(range[2]) : bytes.length - 1;
      rangeRequests++; rangeTimes.push(performance.now()); sockets.add(request.socket);
      await new Promise(resolve => setTimeout(resolve, 40));
      response.writeHead(range ? 206 : 200, {"content-type":"application/octet-stream","content-length":String(end-start+1),"accept-ranges":"bytes","etag":"\"speed-v1\"",...(range?{"content-range":`bytes ${start}-${end}/${bytes.length}`}:{})});
      return response.end(bytes.subarray(start, end + 1));
    }
    response.writeHead(404); response.end('{"error":"file_not_found"}');
  } catch (error) { violations.push(error.message); response.writeHead(500); response.end(); }
});
await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
origin = `http://127.0.0.1:${server.address().port}`;
const reserve = createServer(); await new Promise(resolve => reserve.listen(0, "127.0.0.1", resolve));
const port = reserve.address().port; await new Promise(resolve => reserve.close(resolve));
const child = spawn(join(root,"target/debug/nekopak-webview.exe"), [], {cwd:root,windowsHide:true,stdio:"ignore",env:{...process.env,NEKOPAK_BACKEND:"rust_compat",NEKOPAK_TEST_API_ORIGIN:origin,NEKOPAK_TEST_DATA_DIR:directory,NEKOPAK_TEST_HIDE_WINDOW:"1",WEBVIEW2_USER_DATA_FOLDER:join(directory,"webview"),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
let browser;
async function waitFor(action, check, timeout=40000) {
  const end=Date.now()+timeout;
  let last;
  while(Date.now()<end) {try{const value=await action();last=value;if(check(value))return value;}catch(error){last=String(error);}await new Promise(resolve=>setTimeout(resolve,50));}
  throw new Error(`Throughput fixture timed out: ${JSON.stringify(last)}`);
}
try {
  await waitFor(async()=>await(await fetch(`http://127.0.0.1:${port}/json/list`)).json(), pages=>pages.some(p=>p.url.startsWith("http://tauri.localhost")));
  browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const page=await waitFor(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith("http://tauri.localhost")),Boolean);
  const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  await waitFor(()=>invoke("app_status"), s=>s.storage.status==="ready");
  await invoke("settings_update",{patch:{download_dir:destination,bandwidth_limit_kib_s:0}});
  await invoke("auth_sign_in",{username:"speed@example.test",password:"synthetic-not-an-account"});
  const directStart=performance.now(); const direct=Buffer.from(await(await fetch(`${origin}/content`)).arrayBuffer());
  assert.equal(createHash("sha256").update(direct).digest("hex"),sha256);
  const directMs=performance.now()-directStart;
  metadataRequests=0;rangeRequests=0;sockets.clear();rangeTimes.length=0;
  const start=performance.now();
  const {task_id}=await invoke("transfer_start",{remote_id:"speed-file",destination,idempotency_key:"throughput"});
  await waitFor(()=>invoke("transfers_list"), snapshot=>{
    const task=snapshot.items.find(t=>t.id===task_id);
    if(task?.state==="failed")throw new Error(JSON.stringify(task.error));
    return task?.state==="completed";
  });
  const elapsedMs=performance.now()-start;
  assert.equal(createHash("sha256").update(readFileSync(join(destination,"speed.bin"))).digest("hex"),sha256);
  assert.deepEqual(violations,[]);
  const result={bytes:bytes.length,direct_ms:Math.round(directMs),app_ms:Math.round(elapsedMs),app_mib_per_s:Number((16/(elapsedMs/1000)).toFixed(2)),metadata_requests:metadataRequests,range_requests:rangeRequests,cdn_connections:sockets.size,range_intervals_ms:rangeTimes.slice(1).map((value,index)=>Math.round(value-rangeTimes[index])),sha256,scope:"Synthetic local HTTP40ms latency, actual Rust and Windows WebView; not live-service speed"};
  writeFileSync(join(root,"target/throughput-latest.json"),JSON.stringify(result,null,2)+"\n");
  console.log(JSON.stringify(result,null,2));
  assert(elapsedMs<5000,"The native pipeline adds excessive per-block delay to a fast local download");
} finally { await browser?.close();child.kill();server.closeAllConnections();await new Promise(resolve=>server.close(resolve)); }
