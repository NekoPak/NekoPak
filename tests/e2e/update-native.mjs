// Actual Rust updater over an isolated HTTP release server; synthetic Ed25519 key.
// Desktop installer is a purpose-built no-op .exe, never a user installation.
// Android candidate is built separately with the emulator debug signing identity.
import assert from "node:assert/strict";
import {createServer} from "node:http";
import {spawn,execFileSync} from "node:child_process";
import {createHash,generateKeyPairSync,sign,randomUUID} from "node:crypto";
import {existsSync,mkdirSync,readFileSync,writeFileSync} from "node:fs";
import {join,resolve} from "node:path";
import {chromium} from "playwright";

const root=resolve(import.meta.dirname,"../.."),android=process.argv.includes("--android");
const output=join(root,"target",`update-native-${android?"android":"windows"}`);mkdirSync(output,{recursive:true});
const data=join(output,randomUUID());mkdirSync(data);
const platform=android?"android-aarch64":"windows-x86_64",version="0.1.4";
const name=`NekoPak-${version}-${android?"android-arm64-v8a.apk":"windows-x64-setup.exe"}`;
let packageBytes;
if(android){assert(process.env.NEKOPAK_TEST_UPDATE_APK);packageBytes=readFileSync(process.env.NEKOPAK_TEST_UPDATE_APK);}
else{
  const source=join(data,"installer.rs"),exe=join(data,"installer.exe");
  writeFileSync(source,`#![windows_subsystem="windows"]\nfn main(){std::fs::write(${JSON.stringify(join(data,"installer-ran.txt"))},b"verified update launched").unwrap();}\n`);
  execFileSync("rustc",[source,"-o",exe,"-C","debuginfo=0"],{windowsHide:true});packageBytes=readFileSync(exe);
}
const {publicKey,privateKey}=generateKeyPairSync("ed25519");
const key=publicKey.export({format:"der",type:"spki"}).subarray(-32).toString("base64");
const hash=createHash("sha256").update(packageBytes).digest("hex");
let origin,mode="normal",downloads=0;const errors=[];
const server=createServer(async(req,res)=>{
  const path=new URL(req.url,origin).pathname;
  assert.equal(req.headers.authorization,undefined);
  if(path==="/releases"){
    if(mode==="offline"){res.writeHead(503);res.end();return;}
    const releases=mode==="none"?[]:[{tag_name:`v${version}`,draft:false,prerelease:true,assets:[{name:"update-manifest.json",browser_download_url:`${origin}/download/v${version}/update-manifest.json`},{name,browser_download_url:`${origin}/download/v${version}/${name}`}]}];
    res.setHeader("content-type","application/json");res.end(JSON.stringify(releases));return;
  }
  if(path.endsWith("update-manifest.json")){
    const raw=Buffer.from(JSON.stringify({schema_version:1,product:"NekoPak",version,channel:"preview",published_at:"2026-11-10T00:00:00Z",notes:"Synthetic update acceptance",platforms:{[platform]:{name,bytes:packageBytes.length,sha256:hash}}}));
    const signature=sign(null,raw,privateKey);if(mode==="bad-signature")signature[0]^=1;
    res.end(JSON.stringify({payload_base64:raw.toString("base64"),signature:signature.toString("base64")}));return;
  }
  downloads++;const bytes=Buffer.from(packageBytes);if(mode==="corrupt")bytes[0]^=1;
  res.writeHead(200,{"content-type":"application/octet-stream","content-length":bytes.length});
  for(let at=0;at<bytes.length;at+=65536){if(res.destroyed)return;res.write(bytes.subarray(at,at+65536));await new Promise(r=>setTimeout(r,mode==="slow"?500:1));}res.end();
});
await new Promise(r=>server.listen(0,"127.0.0.1",r));const port=server.address().port;origin=`http://127.0.0.1:${port}`;
writeFileSync(join(data,"update-offline-smoke.json"),JSON.stringify({origin,public_key:key,current_version:"0.1.3"}));
let browser,child,socket,sequence=0;const pending=new Map();
const adb=join(process.env.ANDROID_HOME??"D:/tmp/luminashelf-sdk","platform-tools/adb.exe"),serial="emulator-5580",pkg="io.nekopak.desktop";
const device=(...args)=>execFileSync(adb,["-s",serial,...args],{encoding:"utf8",windowsHide:true,maxBuffer:12*1024*1024});
const delay=ms=>new Promise(r=>setTimeout(r,ms));
async function until(action,check,timeout=45000){const end=Date.now()+timeout;let last;while(Date.now()<end){try{last=await action();if(check(last))return last;}catch(error){last=String(error);}await delay(100);}throw new Error(`Timed out ${JSON.stringify(last)?.slice(0,500)}`);}
async function cdp(method,params={}){return new Promise((resolve,reject)=>{const id=++sequence;const timer=setTimeout(()=>{pending.delete(id);reject(new Error("CDP timeout"));},60000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});}
let invoke;
const checks=[];const pass=name=>{checks.push({name,passed:true});console.log(`PASS ${name}`);};
try{
  if(android){
    assert(device("emu","avd","name").includes("nekopak-smoke-api35"));
    device("shell","am","force-stop",pkg);device("reverse",`tcp:${port}`,`tcp:${port}`);
    device("push",join(data,"update-offline-smoke.json"),"/data/local/tmp/nekopak-update-marker.json");
    device("shell","run-as",pkg,"cp","/data/local/tmp/nekopak-update-marker.json","update-offline-smoke.json");
    device("shell","am","start","-n",`${pkg}/.MainActivity`);
    const pid=await until(()=>device("shell","pidof",pkg).trim(),p=>/^\d+$/.test(p));
    await until(()=>device("shell","cat","/proc/net/unix"),s=>s.includes(`webview_devtools_remote_${pid}`));
    device("forward","tcp:9227",`localabstract:webview_devtools_remote_${pid}`);
    const pages=await until(async()=>await(await fetch("http://127.0.0.1:9227/json/list")).json(),p=>p.some(v=>v.url?.startsWith("http://tauri.localhost")));
    socket=new WebSocket(pages.find(p=>p.url.startsWith("http://tauri.localhost")).webSocketDebuggerUrl);await new Promise(r=>socket.addEventListener("open",r,{once:true}));
    socket.addEventListener("message",event=>{const value=JSON.parse(event.data),entry=pending.get(value.id);if(entry){pending.delete(value.id);clearTimeout(entry.timer);value.error?entry.reject(new Error(JSON.stringify(value.error))):entry.resolve(value.result);}});
    invoke=async(command,args={})=>{const result=await cdp("Runtime.evaluate",{expression:`window.__TAURI__.core.invoke(${JSON.stringify(command)},${JSON.stringify(args)})`,awaitPromise:true,returnByValue:true});if(result.exceptionDetails)throw new Error(JSON.stringify(result.exceptionDetails));return result.result.value;};
  }else{
    const reserve=createServer();await new Promise(r=>reserve.listen(0,"127.0.0.1",r));const port=reserve.address().port;await new Promise(r=>reserve.close(r));
    child=spawn(join(root,"target/debug/nekopak-webview.exe"),[],{cwd:root,windowsHide:true,stdio:"ignore",env:{...process.env,NEKOPAK_TEST_HIDE_WINDOW:"1",NEKOPAK_TEST_DATA_DIR:data,NEKOPAK_BACKEND:"rust_compat",WEBVIEW2_USER_DATA_FOLDER:join(data,"webview"),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
    await until(async()=>await(await fetch(`http://127.0.0.1:${port}/json/list`)).json(),p=>p.some(v=>v.url.startsWith("http://tauri.localhost")));
    browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`);const page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith("http://tauri.localhost")),Boolean);
    page.on("pageerror",error=>errors.push(error.message));invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  }
  await until(()=>invoke("update_status"),s=>s.supported);
  mode="none";assert.equal((await invoke("update_check")).phase,"up_to_date");pass("Empty channel reports no newer version");
  mode="offline";assert.equal((await invoke("update_check")).phase,"error");pass("Network failure is never reported as current version");
  mode="bad-signature";assert.equal((await invoke("update_check")).error.code,"INTEGRITY_FAILED");assert.equal(downloads,0);pass("Invalid signed metadata prevents package download");
  mode="normal";let status=await invoke("update_check");assert.equal(status.phase,"available");assert.equal(status.update.notes,"Synthetic update acceptance");
  mode="slow";await invoke("update_download",{update_id:status.update.update_id});await until(()=>invoke("update_status"),s=>Number(s.downloaded_bytes)>0);await invoke("update_cancel");await until(()=>invoke("update_status"),s=>s.phase==="available");pass("Partial update download cancels without becoming installable");
  mode="corrupt";await invoke("update_download",{update_id:status.update.update_id});status=await until(()=>invoke("update_status"),s=>s.phase==="error");assert.equal(status.error.code,"INTEGRITY_FAILED");pass("Altered package fails SHA256 verification");
  mode="normal";await invoke("update_download",{update_id:status.update.update_id});status=await until(()=>invoke("update_status"),s=>s.phase==="ready",90000);assert.equal(Number(status.downloaded_bytes),packageBytes.length);pass("Complete signed package downloads and verifies");
  if(android){
    device("shell","appops","set",pkg,"REQUEST_INSTALL_PACKAGES","deny");
    assert.equal((await invoke("update_install",{update_id:status.update.update_id})).state,"permission_required");
    device("shell","input","keyevent","KEYCODE_BACK");device("shell","appops","set",pkg,"REQUEST_INSTALL_PACKAGES","allow");
    device("shell","am","start","-n",`${pkg}/.MainActivity`);await delay(300);
    assert.equal((await invoke("update_install",{update_id:status.update.update_id})).state,"installer_opened");
    await until(()=>device("shell","dumpsys","activity","activities"),s=>s.split('\n').some(line=>line.includes('topResumedActivity=')&&line.toLowerCase().includes('packageinstaller')));
    await until(()=>{
      device("shell","uiautomator","dump","/data/local/tmp/nekopak-update-install.xml");
      return device("shell","cat","/data/local/tmp/nekopak-update-install.xml");
    },xml=>xml.includes('text="NekoPak"')&&/text="(?:Update|Install|更新|安装)"/i.test(xml),90000);
    writeFileSync(join(output,"system-installer.png"),execFileSync(adb,["-s",serial,"exec-out","screencap","-p"],{windowsHide:true,maxBuffer:12*1024*1024}));
    device("shell","input","keyevent","KEYCODE_BACK");device("shell","appops","set",pkg,"REQUEST_INSTALL_PACKAGES","default");pass("Android permission flow reaches the system update confirmation for the matching signed APK");
  }else{
    await invoke("update_install",{update_id:status.update.update_id}).catch(()=>{});
    await until(()=>existsSync(join(data,"installer-ran.txt")),Boolean);await until(()=>child.exitCode,code=>code!==null);pass("Windows launches the verified no-op installer and exits only after handoff");
  }
  assert.deepEqual(errors,[]);writeFileSync(join(output,"results.json"),JSON.stringify({date:new Date().toISOString(),scope:"Actual native update service; synthetic local server and key; no real account",platform,checks,package_sha256:hash},null,2)+'\n');
}finally{
  await browser?.close();if(child?.exitCode===null)child.kill();socket?.close();
  if(android){device("shell","am","force-stop",pkg);device("shell","run-as",pkg,"rm","-f","update-offline-smoke.json");device("shell","rm","-f","/data/local/tmp/nekopak-update-install.xml","/data/local/tmp/nekopak-update-marker.json");device("shell","appops","set",pkg,"REQUEST_INSTALL_PACKAGES","default");device("reverse","--remove",`tcp:${port}`);}
  server.closeAllConnections();await new Promise(r=>server.close(r));
}
