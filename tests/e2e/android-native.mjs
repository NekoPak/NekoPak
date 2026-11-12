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
const torrentName=`nekopak-${Date.now()}.torrent`;
const torrentInfo=Buffer.concat([Buffer.from('d6:lengthi3e4:name8:test.bin12:piece lengthi16384e6:pieces20:'),Buffer.alloc(20,9),Buffer.from('e')]);
const torrentBytes=Buffer.concat([Buffer.from('d4:info'),torrentInfo,Buffer.from('e')]);
const torrentHash=createHash('sha1').update(torrentInfo).digest('hex');let cloudCreates=0;
let origin, cdnCalls=0, cdnBytes=0, interruptNextRange=false, injectedFailures=0;const violations=[];
const server=createServer(async(req,res)=>{
  try{
    const u=new URL(req.url,origin);let text='';for await(const part of req)text+=part;const body=text?JSON.parse(text):{};
    const json=(value,status=200)=>{res.writeHead(status,{'content-type':'application/json'});res.end(JSON.stringify(value));};
    if(u.pathname==='/favicon.ico')return json({},404);
    if(u.pathname==='/v1/shield/captcha/init')return json({captcha_token:'android-fixture-captcha',expires_in:300});
    if(u.pathname==='/v1/auth/signin'){assert.equal(body.username,'fixture@example.test');assert.equal(body.password,'synthetic-not-an-account');return json({access_token:'android-fixture-access',refresh_token:'android-fixture-refresh',token_type:'Bearer',expires_in:3600,sub:'android-fixture-subject'});}
    if(u.pathname==='/cdn/original'){
      assert.equal(req.headers.authorization,undefined);cdnCalls++;
      if(interruptNextRange){interruptNextRange=false;injectedFailures++;res.writeHead(503,{'Retry-After':'1'});res.end('synthetic temporary outage');return;}
      const range=/^bytes=(\d+)-(\d+)$/.exec(req.headers.range);assert.ok(range);const start=Number(range[1]),end=Number(range[2]);
      res.writeHead(206,{'content-range':`bytes ${start}-${end}/${bytes.length}`,'content-length':String(end-start+1),'etag':'"android-fixture-v1"'});
      for(let at=start;at<=end;at+=65536){if(res.destroyed)return;const part=bytes.subarray(at,Math.min(at+65536,end+1));res.write(part);cdnBytes+=part.length;await delay(120);}res.end();return;
    }
    assert.equal(req.headers.authorization,'Bearer android-fixture-access',`Unexpected authorization on ${req.method} ${u.pathname}`);
    const file={id:'file-1',parent_id:'',name:filename,kind:'drive#file',size:String(bytes.length),modified_time:'2026-11-09T00:00:00Z',phase:'PHASE_TYPE_COMPLETE',trashed:false,tags:[]};
    if(u.pathname==='/drive/v1/files'&&req.method==='POST'){
      assert.equal(body.upload_type,'UPLOAD_TYPE_URL');assert.equal(new URL(body.url.url).searchParams.get('xt'),`urn:btih:${torrentHash}`);
      cloudCreates++;return json({task:{id:'torrent-cloud',name:'test.bin',phase:'PHASE_TYPE_PENDING',progress:0}});
    }
    if(u.pathname==='/drive/v1/tasks/torrent-cloud')return json({id:'torrent-cloud',name:'test.bin',phase:'PHASE_TYPE_COMPLETE',progress:100});
    if(u.pathname==='/drive/v1/files')return json({files:[file]});
    if(u.pathname==='/drive/v1/files/file-1')return json({...file,web_content_link:`${origin}/cdn/original`});
    if(u.pathname==='/drive/v1/files/file-2')return json({...file,id:'file-2',name:`${filename}.cancelled`,web_content_link:`${origin}/cdn/original`});
    if(u.pathname==='/drive/v1/about')return json({quota:{limit:'1000000000',usage:'12000000'}});
    if(u.pathname==='/vip/v1/quantity/list')return json({base:{download:{total_assets:'1000000000',assets:'12000000'}}});
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
function tapNative(xml, attribute, value){
  const node=[...xml.matchAll(/<node\b[^>]*>/g)].map(match=>match[0]).find(node=>node.includes(`${attribute}="${value}"`));
  assert(node,`Missing native ${attribute}=${value}`);
  const rect=/bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/.exec(node);assert(rect);
  device('shell','input','tap',String(Math.round((+rect[1]+ +rect[3])/2)),String(Math.round((+rect[2]+ +rect[4])/2)));
}
const previousLocales=device('shell','cmd','locale','get-app-locales',pkg).match(/\[([^\]]*)\]/)?.[1]??'';
try{
  assert.match(device('emu','avd','name'),/nekopak-smoke-api35/);
  device('shell','am','force-stop',pkg);
  device('shell','cmd','locale','set-app-locales',pkg,'--locales','zh-CN');
  writeFileSync(join(out,'marker.json'),JSON.stringify({origin}));
  device('push',join(out,'marker.json'),'/data/local/tmp/nekopak-offline-marker.json');
  device('shell','run-as',pkg,'cp','/data/local/tmp/nekopak-offline-marker.json','android-offline-smoke.json');
  device('shell','am','start','-n',`${pkg}/.MainActivity`);await connect();
  const status=await until(()=>invoke('app_status'),s=>s.storage?.status==='ready');
  assert.equal(status.features.mobile,true);assert.equal(status.features.auth_sign_in,true);assert.equal(status.features.upload,false);assert.equal(status.features.tray,false);
  assert.equal(status.system_locale,'zh-CN');
  await invoke('settings_update',{patch:{language:'system'}});
  await until(()=>evaluate('document.documentElement.lang'),language=>language==='zh-CN');
  record('Android application locale is read natively and system language resolves to Chinese');
  const vault=JSON.parse(device('shell','run-as',pkg,'cat','android-vault-selftest.json'));assert.equal(vault.rust_jni_keystore,'passed');record('Rust JNI AndroidKeyStore synthetic roundtrip, accounts and auxiliary records');
  await until(()=>evaluate('!!document.querySelector(".phone-login .signin")'),Boolean);
  await screenshot('login.png');
  await evaluate('[...document.querySelectorAll("button")].find(button=>button.textContent.trim()==="先看看")?.click()');
  await until(()=>evaluate('!document.querySelector(".phone-login")'),Boolean);
  await evaluate('location.hash="#/settings/account"');
  await until(()=>evaluate('!!document.querySelector(".phone-settings input[name=password]")'),Boolean);
  await evaluate('document.querySelector(".phone-settings input[name=password]").scrollIntoView({block:"center",behavior:"instant"})');
  await delay(150);
  const passwordBox=await evaluate('(()=>{const input=document.querySelector(".phone-settings input[name=password]");const box=input.getBoundingClientRect();return {x:box.x+box.width/2,y:box.y+box.height/2};})()');
  assert.equal(await evaluate(`document.elementFromPoint(${passwordBox.x},${passwordBox.y})?.name`),'password');
  const heightBeforeKeyboard=await evaluate('innerHeight');
  await cdp('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[passwordBox]});
  await cdp('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
  await until(()=>evaluate('innerHeight'),height=>height<heightBeforeKeyboard-100);
  await screenshot('password-keyboard.png');
  writeFileSync(join(out,'password-keyboard-device.png'),execFileSync(adb,['-s',serial,'exec-out','screencap','-p'],{windowsHide:true,maxBuffer:12*1024*1024}));
  assert.equal(await evaluate('!!document.querySelector(".mobile-bottom-nav")?.getClientRects().length'),false,'phone navigation must not rise above the software keyboard');
  device('shell','input','keyevent','KEYCODE_BACK');
  await until(()=>evaluate('!!document.querySelector(".mobile-bottom-nav")?.getClientRects().length'),Boolean);
  assert.equal(await evaluate('location.hash'),'#/settings/account','back closes the keyboard before navigating');
  record('Password keyboard hides bottom navigation; Android Back dismisses keyboard and restores navigation');
  await evaluate('location.hash="#/files/root"');
  const invalid=await evaluate(`window.__TAURI__.core.invoke('auth_sign_in',{username:'',password:'synthetic-not-sent'}).catch(e=>e)`);assert.equal(invalid.code,'CONFLICT');assert.ok(!JSON.stringify(invalid).includes('synthetic-not-sent'));
  await invoke('auth_preferences_update',{preferences:{remember_account:true,restore_session:true,username:'fixture@example.test'}});
  assert.deepEqual(await invoke('auth_preferences_get'),{remember_account:true,restore_session:true,username:'fixture@example.test'});
  await invoke('auth_sign_in',{username:'fixture@example.test',password:'synthetic-not-an-account'});
  assert.equal((await invoke('auth_status')).authenticated,true);
  assert.equal((await invoke('files_list',{parent_id:'root'})).items[0].name,filename);record('Native sign-in, validation redaction and file list via loopback');
  writeFileSync(join(out,torrentName),torrentBytes);device('push',join(out,torrentName),`/sdcard/Download/${torrentName}`);
  const cancelledPicker=invoke('cloud_torrent_pick');await until(pickerReady,Boolean);
  device('shell','input','keyevent','KEYCODE_BACK');await delay(300);if(pickerReady())device('shell','input','keyevent','KEYCODE_BACK');assert.equal(await cancelledPicker,null);
  const picking=invoke('cloud_torrent_pick');await until(pickerReady,Boolean);
  let pickerXml=await until(nativeUi,xml=>xml.includes('documentsui'));
  if(!pickerXml.includes(`text="${torrentName}"`)){
    if(pickerXml.includes('content-desc="Show roots"'))tapNative(pickerXml,'content-desc','Show roots');
    pickerXml=await until(nativeUi,xml=>xml.includes('text="Downloads"'));
    tapNative(pickerXml,'text','Downloads');
    pickerXml=await until(nativeUi,xml=>xml.includes(`text="${torrentName}"`));
  }
  tapNative(pickerXml,'text',torrentName);
  const torrent=await picking;assert.equal(torrent.info_hash,torrentHash);assert.equal(torrent.total_bytes,'3');assert.equal(torrent.file_count,1);assert.equal(cloudCreates,0);
  const torrentArgs={import_id:torrent.import_id,parent_id:'root',idempotency_key:`torrent-${Date.now()}`};
  const cloud=await invoke('cloud_torrent_create',torrentArgs);assert.deepEqual(await invoke('cloud_torrent_create',torrentArgs),cloud);assert.equal(cloudCreates,1);
  await invoke('cloud_torrent_release',{import_id:torrent.import_id});
  record('Actual Android torrent picker cancels safely, parses .torrent bytes and submits exactly one cloud task on confirmation');
  await until(()=>evaluate('document.querySelectorAll(".mobile-bottom-nav button").length'),count=>count===3);
  assert.equal(await evaluate('document.querySelectorAll(".sidebar,.workspace-tabs,.pf-commands,.address-drop").length'),0);
  await until(()=>evaluate('document.querySelectorAll(".mobile-file-row").length'),count=>count>0);
  await screenshot('phone-files.png');
  await evaluate('[...document.querySelectorAll(".mobile-bottom-nav button")].find(button=>button.textContent.includes("我的")).click()');
  await until(()=>evaluate('!!document.querySelector(".mobile-account")'),Boolean);
  await screenshot('phone-me.png');
  await evaluate('[...document.querySelectorAll(".mobile-bottom-nav button")].find(button=>button.textContent.includes("传输")).click()');
  record('Installed Android WebView uses phone navigation, touch file rows and account page without desktop controls');
  const settings=await invoke('settings_get');assert.ok(settings.download_dir.endsWith('/Downloads'));
  const task=await invoke('transfer_start',{remote_id:'file-1',destination:settings.download_dir,idempotency_key:`android-e2e-${Date.now()}`});
  const snapshot=()=>invoke('transfers_list');
  const live=await until(snapshot,s=>s.items.some(t=>t.id===task.task_id&&Number(t.received_bytes)>Number(t.transferred_bytes)));
  const liveTask=live.items.find(t=>t.id===task.task_id);
  assert(Number(liveTask.received_bytes)>0);assert(liveTask.speed_bytes_per_sec>0);
  await until(()=>evaluate(`Number(document.querySelector('[data-task="${task.task_id}"] [role="progressbar"]')?.getAttribute('aria-valuenow'))`),percent=>percent>0);
  record('Phone progress moves during an unfinished network chunk while restart bytes stay verified');
  await until(snapshot,s=>s.items.some(t=>t.id===task.task_id&&Number(t.transferred_bytes)>0));
  await until(snapshot,s=>s.items.some(t=>t.id===task.task_id&&t.state==='running'&&t.speed_bytes_per_sec>0));
  await screenshot('phone-transfers.png');record('Actual received bytes produce a nonzero download speed');
  // Default setting: the dataSync foreground service keeps bytes moving while hidden.
  device('shell','input','keyevent','KEYCODE_HOME');await delay(700);const hiddenBytes=cdnBytes;
  await until(async()=>cdnBytes,value=>value>hiddenBytes+262144);
  const services=device('shell','dumpsys','activity','services',pkg);assert.match(services,/io\.nekopak\.desktop\.TransferService/);assert.match(services,/isForeground=true/);
  device('shell','am','start','-n',`${pkg}/.MainActivity`);await delay(400);
  assert.notEqual((await snapshot()).items.find(t=>t.id===task.task_id).state,'paused');
  record('Foreground service keeps local bytes moving while the activity is hidden');
  // Option off: the service stops and hiding the activity pauses durably as before.
  await invoke('settings_update',{patch:{background_transfers:false}});
  await until(async()=>device('shell','dumpsys','activity','services',pkg),text=>!/io\.nekopak\.desktop\.TransferService/.test(text));
  assert.equal((await snapshot()).items.find(t=>t.id===task.task_id).state,'running');
  device('shell','input','keyevent','KEYCODE_HOME');await delay(700);const atPause=cdnCalls;await delay(1000);assert.equal(cdnCalls,atPause);
  device('shell','am','start','-n',`${pkg}/.MainActivity`);await delay(400);
  const paused=(await snapshot()).items.find(t=>t.id===task.task_id);assert.equal(paused.state,'paused');assert.ok(Number(paused.transferred_bytes)>0);record('Activity background pauses local bytes; no further range requests');
  await invoke('network_reconnect');await delay(400);assert.equal((await snapshot()).items.find(t=>t.id===task.task_id).state,'paused');
  record('Refreshing network connections never resumes a manually/background-paused task');
  socket.close();device('shell','am','force-stop',pkg);device('shell','am','start','-n',`${pkg}/.MainActivity`);await connect();
  await invoke('auth_sign_in',{username:'fixture@example.test',password:'synthetic-not-an-account'});
  const restored=(await snapshot()).items.find(t=>t.id===task.task_id);assert.equal(restored.state,'paused');assert.equal(restored.transferred_bytes,paused.transferred_bytes);
  interruptNextRange=true;
  await invoke('transfer_control',{task_id:task.task_id,action:'resume'});
  await until(snapshot,s=>s.items.some(t=>t.id===task.task_id&&t.state==='retrying'));
  await until(snapshot,s=>s.items.some(t=>t.id===task.task_id&&t.state==='completed'),45000);
  assert.equal(injectedFailures,1);record('Transient HTTP failure automatically reconnects and completes without manual retry');
  record('Process restart preserves queue/bytes and explicit resume completes');
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
  const cancelled=await invoke('transfer_start',{remote_id:'file-2',destination:settings.download_dir,idempotency_key:`android-cancel-${Date.now()}`});
  await invoke('transfer_control',{task_id:cancelled.task_id,action:'cancel'});
  await invoke('transfer_forget',{task_id:cancelled.task_id});
  await invoke('transfer_forget',{task_id:task.task_id});
  assert(!(await snapshot()).items.some(t=>[task.task_id,cancelled.task_id].includes(t.id)));
  assert.equal(createHash('sha256').update(execFileSync(adb,['-s',serial,'exec-out','run-as',pkg,'cat',`Downloads/${filename}`],{windowsHide:true,maxBuffer:20*1024*1024})).digest('hex'),createHash('sha256').update(bytes).digest('hex'));
  socket.close();device('shell','am','force-stop',pkg);device('shell','am','start','-n',`${pkg}/.MainActivity`);await connect();
  await invoke('auth_sign_in',{username:'fixture@example.test',password:'synthetic-not-an-account'});
  assert(!(await snapshot()).items.some(t=>[task.task_id,cancelled.task_id].includes(t.id)));
  record('Completed/cancelled history stays removed after process restart while downloaded bytes remain intact');
  assert.deepEqual(violations,[]);
  writeFileSync(join(out,'results.json'),JSON.stringify({date:'2026-11-10',platform:'Android15/API35 x86_64 isolated emulator',serial,checks,network:'synthetic loopback only; in-memory provider account fixture; separate actual JNI Keystore checks',pending:['physical ARM64','real login/CAPTCHA','arbitrary document providers']},null,2)+'\n');
}finally{
  socket?.close();
  device('shell','am','force-stop',pkg);
  device('shell','run-as',pkg,'rm','-f','android-offline-smoke.json');
  device('shell','rm','-f',`/sdcard/Download/${torrentName}`);
  device('shell','cmd','locale','set-app-locales',pkg,...(previousLocales?['--locales',previousLocales]:[]));
  device('reverse','--remove',`tcp:${port}`);
  server.closeAllConnections();await new Promise(r=>server.close(r));
}
