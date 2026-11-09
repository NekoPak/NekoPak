// Isolated debug emulator acceptance: real Android WebView/IPC/Rust/JNI/storage.
// Never launches on a physical device; synthetic account talks only to loopback.
import {execFileSync} from 'node:child_process';
import {createServer} from 'node:http';
import {mkdirSync, writeFileSync, readFileSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';

const root=resolve(import.meta.dirname,'../..');
const adb=process.env.ANDROID_ADB ?? join(process.env.ANDROID_HOME ?? '', 'platform-tools/adb.exe');
const serial=process.env.NEKOPAK_EMULATOR ?? 'emulator-5580';
assert.equal(serial,'emulator-5580','Only the dedicated NekoPak emulator is permitted.');
const pkg='io.nekopak.desktop',out=join(root,'target/android-native');mkdirSync(out,{recursive:true});
function device(...args){return execFileSync(adb,['-s',serial,...args],{encoding:'utf8',windowsHide:true,maxBuffer:8*1024*1024});}
const delay=ms=>new Promise(r=>setTimeout(r,ms));
async function until(action,check,timeout=20000){const end=Date.now()+timeout;let last;while(Date.now()<end){try{last=await action();if(check(last))return last;}catch{}await delay(150);}throw new Error(`Timed out: ${JSON.stringify(last)?.slice(0,2000)}`);}
const filename=`nekopak-android-${Date.now()}.bin`;
const bytes=Buffer.alloc(8*1024*1024+137);for(let i=0;i<bytes.length;i++)bytes[i]=i%251;
let origin, cdnCalls=0;const violations=[];
const server=createServer(async(req,res)=>{
  try{
    const u=new URL(req.url,origin);let text='';for await(const part of req)text+=part;const body=text?JSON.parse(text):{};
    const json=(value,status=200)=>{res.writeHead(status,{'content-type':'application/json'});res.end(JSON.stringify(value));};
    if(u.pathname==='/favicon.ico')return json({},404);
    if(u.pathname==='/v1/shield/captcha/init')return json({captcha_token:'android-fixture-captcha',expires_in:300});
    if(u.pathname==='/v1/auth/signin'){assert.equal(body.username,'fixture@example.test');assert.equal(body.password,'synthetic-not-an-account');return json({access_token:'android-fixture-access',refresh_token:'android-fixture-refresh',token_type:'Bearer',expires_in:3600,sub:'android-fixture-subject'});}
    if(u.pathname==='/cdn/original'){
      assert.equal(req.headers.authorization,undefined);cdnCalls++;
      const range=/^bytes=(\d+)-(\d+)$/.exec(req.headers.range);assert.ok(range);const start=Number(range[1]),end=Number(range[2]);
      res.writeHead(206,{'content-range':`bytes ${start}-${end}/${bytes.length}`,'content-length':String(end-start+1),'etag':'"android-fixture-v1"'});
      for(let at=start;at<=end;at+=65536){if(res.destroyed)return;res.write(bytes.subarray(at,Math.min(at+65536,end+1)));await delay(18);}res.end();return;
    }
    assert.equal(req.headers.authorization,'Bearer android-fixture-access',`Unexpected authorization on ${req.method} ${u.pathname}`);
    const file={id:'file-1',parent_id:'',name:filename,kind:'drive#file',size:String(bytes.length),modified_time:'2026-11-09T00:00:00Z',phase:'PHASE_TYPE_COMPLETE',trashed:false,tags:[]};
    if(u.pathname==='/drive/v1/files')return json({files:[file]});
    if(u.pathname==='/drive/v1/files/file-1')return json({...file,web_content_link:`${origin}/cdn/original`});
    if(u.pathname==='/drive/v1/about')return json({quota:{limit:'1000000000',usage:'12000000'}});
    if(u.pathname==='/vip/v1/quantity/list')return json({base:{download:{total_assets:'100',assets:'100'}}});
    if(u.pathname==='/v1/user/me')return json({sub:'android-fixture-subject',name:'Fixture'});
    if(u.pathname==='/drive/v1/privilege/vip')return json({data:[]});
    return json({error:'file_not_found'},404);
  }catch(error){violations.push(String(error));console.error('SERVER',error,'PATH',req.url);res.writeHead(500);res.end();}
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));const port=server.address().port;origin=`http://127.0.0.1:${port}`;
device('reverse',`tcp:${port}`,`tcp:${port}`);
let socket,sequence=0;const pending=new Map();
async function connect(){
  const pid=await until(()=>device('shell','pidof',pkg).trim(),s=>/^\d+$/.test(s));
  await until(()=>device('shell','cat','/proc/net/unix'),s=>s.includes(`webview_devtools_remote_${pid}`));
  device('forward','tcp:9227',`localabstract:webview_devtools_remote_${pid}`);
  const pages=await until(async()=>await(await fetch('http://127.0.0.1:9227/json/list')).json(),p=>p.some(x=>x.url?.startsWith('http://tauri.localhost')));
  socket=new WebSocket(pages.find(x=>x.url?.startsWith('http://tauri.localhost')).webSocketDebuggerUrl);
  await new Promise(r=>socket.addEventListener('open',r,{once:true}));
  socket.addEventListener('message',e=>{const message=JSON.parse(e.data);const p=pending.get(message.id);if(p){pending.delete(message.id);clearTimeout(p.timer);message.error?p.reject(new Error(JSON.stringify(message.error))):p.resolve(message.result);}});
  await until(()=>evaluate('!!window.__TAURI__?.core'),Boolean);
  await until(()=>invoke('app_status'),s=>s.storage?.status==='ready');
}
async function cdp(method,params={}){return new Promise((resolve,reject)=>{const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(new Error(`CDP timeout ${method}`));},90000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});}
async function evaluate(expression){const result=await cdp('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(result.exceptionDetails)throw new Error(JSON.stringify(result.exceptionDetails));return result.result.value;}
function invoke(command,args={}){return evaluate(`window.__TAURI__.core.invoke(${JSON.stringify(command)},${JSON.stringify(args)})`);}
async function screenshot(name){const image=await cdp('Page.captureScreenshot',{format:'png'});writeFileSync(join(out,name),Buffer.from(image.data,'base64'));}
const checks=[];const record=name=>{checks.push({name,result:'passed'});console.log(`PASS ${name}`);};
function pickerReady(){return device('shell','dumpsys','activity','activities').split('\n').some(l=>l.includes('topResumedActivity=')&&l.includes('com.android.documentsui.picker.PickActivity'));}
function nativeUi(){device('shell','uiautomator','dump','/data/local/tmp/nekopak-window.xml');return device('shell','cat','/data/local/tmp/nekopak-window.xml');}
try{
  device('shell','am','force-stop',pkg);
  writeFileSync(join(out,'marker.json'),JSON.stringify({origin}));
  device('push',join(out,'marker.json'),'/data/local/tmp/nekopak-offline-marker.json');
  device('shell','run-as',pkg,'cp','/data/local/tmp/nekopak-offline-marker.json','android-offline-smoke.json');
  device('shell','am','start','-n',`${pkg}/.MainActivity`);await connect();
  const status=await until(()=>invoke('app_status'),s=>s.storage?.status==='ready');
  assert.equal(status.features.mobile,true);assert.equal(status.features.auth_sign_in,true);assert.equal(status.features.upload,false);assert.equal(status.features.tray,false);
  const vault=JSON.parse(device('shell','run-as',pkg,'cat','android-vault-selftest.json'));assert.equal(vault.rust_jni_keystore,'passed');record('Rust JNI AndroidKeyStore synthetic roundtrip, accounts and auxiliary records');
  await screenshot('login.png');
  const invalid=await evaluate(`window.__TAURI__.core.invoke('auth_sign_in',{username:'',password:'synthetic-not-sent'}).catch(e=>e)`);assert.equal(invalid.code,'CONFLICT');assert.ok(!JSON.stringify(invalid).includes('synthetic-not-sent'));
  await invoke('auth_sign_in',{username:'fixture@example.test',password:'synthetic-not-an-account'});
  assert.equal((await invoke('auth_status')).authenticated,true);
  assert.equal((await invoke('files_list',{parent_id:'root'})).items[0].name,filename);record('Native sign-in, validation redaction and file list via loopback');
  const settings=await invoke('settings_get');assert.ok(settings.download_dir.endsWith('/Downloads'));
  const task=await invoke('transfer_start',{remote_id:'file-1',destination:settings.download_dir,idempotency_key:`android-e2e-${Date.now()}`});
  const snapshot=()=>invoke('transfers_list');
  await until(snapshot,s=>s.items.some(t=>t.id===task.task_id&&Number(t.transferred_bytes)>0));
  device('shell','input','keyevent','KEYCODE_HOME');await delay(700);const atPause=cdnCalls;await delay(1000);assert.equal(cdnCalls,atPause);
  device('shell','am','start','-n',`${pkg}/.MainActivity`);await delay(400);
  const paused=(await snapshot()).items.find(t=>t.id===task.task_id);assert.equal(paused.state,'paused');assert.ok(Number(paused.transferred_bytes)>0);record('Activity background pauses local bytes; no further range requests');
  socket.close();device('shell','am','force-stop',pkg);device('shell','am','start','-n',`${pkg}/.MainActivity`);await connect();
  await invoke('auth_sign_in',{username:'fixture@example.test',password:'synthetic-not-an-account'});
  const restored=(await snapshot()).items.find(t=>t.id===task.task_id);assert.equal(restored.state,'paused');assert.equal(restored.transferred_bytes,paused.transferred_bytes);
  await invoke('transfer_control',{task_id:task.task_id,action:'resume'});await until(snapshot,s=>s.items.some(t=>t.id===task.task_id&&t.state==='completed'),45000);record('Process restart preserves queue/bytes and explicit resume completes');
  const downloaded=execFileSync(adb,['-s',serial,'exec-out','run-as',pkg,'cat',`Downloads/${filename}`],{windowsHide:true,maxBuffer:20*1024*1024});assert.equal(createHash('sha256').update(downloaded).digest('hex'),createHash('sha256').update(bytes).digest('hex'));record('Downloaded bytes match original SHA256');
  const exported=invoke('transfer_export',{task_id:task.task_id});
  await until(pickerReady,Boolean);await until(nativeUi,s=>s.includes('resource-id="android:id/button1"'));
  device('shell','input','keyevent','KEYCODE_BACK');await delay(300);if(pickerReady())device('shell','input','keyevent','KEYCODE_BACK');
  assert.equal(await exported,false);record('Real Android document picker cancellation preserves original');
  const saving=invoke('transfer_export',{task_id:task.task_id});
  await until(pickerReady,Boolean);const ui=await until(nativeUi,s=>s.includes('resource-id="android:id/button1"'));
  const node=ui.match(/<node[^>]*resource-id="android:id\/button1"[^>]*>/)?.[0];assert.ok(node);
  const rect=/bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/.exec(node);assert.ok(rect);
  device('shell','input','tap',String(Math.round((Number(rect[1])+Number(rect[3]))/2)),String(Math.round((Number(rect[2])+Number(rect[4]))/2)));
  assert.equal(await saving,true);
  const external=execFileSync(adb,['-s',serial,'exec-out','cat',`/sdcard/Download/${filename}`],{windowsHide:true,maxBuffer:20*1024*1024});assert.equal(createHash('sha256').update(external).digest('hex'),createHash('sha256').update(bytes).digest('hex'));record('Real SAF save exports matching bytes to shared Downloads');
  await screenshot('signed-in.png');
  const dimensions=await evaluate('({width:innerWidth,scroll:document.documentElement.scrollWidth})');assert.ok(dimensions.scroll<=dimensions.width+1);record('Actual Android WebView has no horizontal page overflow');
  assert.deepEqual(violations,[]);
  writeFileSync(join(out,'results.json'),JSON.stringify({date:'2026-11-09',platform:'Android15/API35 x86_64 isolated emulator',serial,checks,network:'synthetic loopback only; no real account',pending:['physical ARM64','real login/CAPTCHA','arbitrary document providers']},null,2)+'\n');
}finally{
  socket?.close();
  device('shell','am','force-stop',pkg);
  device('shell','run-as',pkg,'rm','-f','android-offline-smoke.json');
  device('reverse','--remove',`tcp:${port}`);
  server.closeAllConnections();await new Promise(r=>server.close(r));
}
