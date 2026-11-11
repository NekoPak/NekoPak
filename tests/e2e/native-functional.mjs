// Real WebView2 + real Rust HTTP/provider/SQLite/downloader, against isolated synthetic HTTP.
// No user account, external service, OS credentials, or user's app data are accessed.
import {createServer} from 'node:http';
import {spawn,spawnSync} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {mkdirSync,readFileSync,writeFileSync,existsSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {chromium} from 'playwright';
import assert from 'node:assert/strict';

const root=resolve(import.meta.dirname,'../..');
const data=join(root,'target',`native-functional-${randomUUID()}`);
const destination=join(data,'downloads');mkdirSync(destination,{recursive:true});
const bytes=Buffer.alloc(2*1024*1024+137);for(let i=0;i<bytes.length;i++)bytes[i]=i%251;
const file={id:'file-1',parent_id:'',name:'fixture.bin',kind:'drive#file',size:String(bytes.length),hash:'synthetic-content-v1',modified_time:'2026-11-08T00:00:00Z',phase:'PHASE_TYPE_COMPLETE',trashed:false,tags:[]};
const files=new Map([[file.id,file]]);let sequence=0;let verifyRequired=true;let origin;
const cloudTasks=new Map();const shares=new Map();
const mediaPath=join(data,'source.mp4');
const ffmpeg='C:/Users/elysia/AppData/Local/Programs/Python/Python312/Lib/site-packages/imageio_ffmpeg/binaries/ffmpeg-win-x86_64-v7.1.exe';
const mediaGenerated=spawnSync(ffmpeg,['-hide_banner','-loglevel','error','-f','lavfi','-i','testsrc2=size=320x180:rate=25:duration=12','-f','lavfi','-i','sine=frequency=440:sample_rate=48000:duration=12','-c:v','libx264','-g','50','-keyint_min','50','-sc_threshold','0','-bf','2','-c:a','aac','-y',mediaPath],{windowsHide:true});
assert.equal(mediaGenerated.status,0,String(mediaGenerated.stderr));const mediaBytes=readFileSync(mediaPath);
const mediaFile={...file,id:'video-1',name:'source.mp4',size:String(mediaBytes.length),mime_type:'video/mp4',hash:'a'.repeat(40)};files.set(mediaFile.id,mediaFile);
files.set('archive-1',{...file,id:'archive-1',name:'archive.zip',hash:'b'.repeat(40)});
const requests=[];const violations=[];
const server=createServer(async(req,res)=>{
  try{
    const u=new URL(req.url,origin);let input='';for await(const chunk of req)input+=chunk;
    const body=input?JSON.parse(input):{};
    requests.push({method:req.method,path:u.pathname});
    const json=(status,value)=>{res.writeHead(status,{'content-type':'application/json'});res.end(JSON.stringify(value));};
    if(u.pathname==='/v1/shield/captcha/init'){
      if(body.action==='POST:/v1/auth/signin')return json(200,{captcha_token:'fixture-login',expires_in:300});
      assert.equal(body.meta.user_id,'synthetic-user');assert.equal(body.meta.client_version,'2.0.0');
      assert.match(body.action,/^(GET|POST|PATCH|DELETE):\//);assert.ok(!body.action.includes('?'));
      return json(200,verifyRequired?{captcha_token:'pending-fixture',url:'https://user.mypikpak.com/challenge?fixture=true',expires_in:300}:{captcha_token:'fixture-drive',expires_in:300});
    }
    if(u.pathname==='/v1/auth/signin'){
      assert.equal(body.username,'fixture@example.test');assert.equal(body.password,'synthetic-not-an-account');
      return json(200,{access_token:'fixture-access',refresh_token:'fixture-refresh',token_type:'Bearer',expires_in:3600,sub:'synthetic-user'});
    }
    if(u.pathname==='/cdn/original'||u.pathname==='/cdn/video'){
      assert.equal(req.headers.authorization,undefined);assert.equal(req.headers['x-captcha-token'],undefined);assert.equal(req.headers['x-device-id'],undefined);
      const match=/^bytes=(\d+)-(\d+)$/.exec(req.headers.range);assert.ok(match);
      const start=Number(match[1]),end=Number(match[2]);const content=u.pathname==='/cdn/video'?mediaBytes:bytes;
      res.writeHead(206,{'content-range':`bytes ${start}-${end}/${content.length}`,'content-length':String(end-start+1),'content-type':'application/octet-stream','etag':'"fixture-v1"'});
      for(let at=start;at<=end;at+=65536){if(res.destroyed)return;res.write(content.subarray(at,Math.min(at+65536,end+1)));await new Promise(ok=>setTimeout(ok,u.pathname==='/cdn/video'?0:80));}res.end();return;
    }
    assert.equal(req.headers.authorization,'Bearer fixture-access');
    if(verifyRequired)return json(403,{error:'captcha_required'});
    assert.ok(['fixture-drive','human-fixture-token'].includes(req.headers['x-captcha-token']));
    if(u.pathname==='/drive/v1/about')return json(200,{quota:{limit:'1000000000',usage:'12000000'}});
    if(u.pathname==='/vip/v1/quantity/list')return json(200,{base:{download:{total_assets:'100',assets:'120'},download_daily:{total_assets:'0',assets:'25'}}});
    if(u.pathname==='/drive/v1/files'&&req.method==='GET'){
      const filters=JSON.parse(u.searchParams.get('filters'));const parent=u.searchParams.get('parent_id');
      return json(200,{files:[...files.values()].filter(f=>(parent==='*'||f.parent_id===parent)&&f.trashed===filters.trashed.eq&&(!filters.system_tag||f.tags.some(t=>t.name==='STAR')))});
    }
    if(u.pathname==='/drive/v1/files'&&req.method==='POST'){
      if(body.upload_type==='UPLOAD_TYPE_URL'){const task={id:`cloud-${++sequence}`,phase:'PHASE_TYPE_PENDING',progress:0,name:'Cloud fixture'};cloudTasks.set(task.id,task);return json(200,{task});}
      const folder={id:`folder-${++sequence}`,name:body.name,parent_id:body.parent_id,kind:'drive#folder',phase:'PHASE_TYPE_COMPLETE',trashed:false,tags:[]};files.set(folder.id,folder);return json(200,{file:folder});
    }
    if(u.pathname.startsWith('/drive/v1/files:')){
      const action=u.pathname.split(':')[1];
      for(const id of body.ids){const f=files.get(id);assert.ok(f);if(action==='star')f.tags=[{name:'STAR'}];else if(action==='unstar')f.tags=[];else if(action==='batchTrash')f.trashed=true;else if(action==='batchUntrash')f.trashed=false;else if(action==='batchMove')f.parent_id=body.to.parent_id;else if(action==='batchCopy'){const copy={...f,id:`copy-${++sequence}`,parent_id:body.to.parent_id};files.set(copy.id,copy);}else throw new Error('unexpected action');}
      return json(200,{task:{id:'job-1',phase:'PHASE_TYPE_COMPLETE'}});
    }
    if(u.pathname.startsWith('/drive/v1/files/')){
      const f=files.get(u.pathname.split('/').at(-1));if(!f)return json(404,{error:'file_not_found'});
      if(req.method==='PATCH')f.name=body.name;
      return json(200,{...f,web_content_link:`${origin}/cdn/${f.id==='video-1'?'video':'original'}`});
    }
    if(u.pathname==='/drive/v1/resource/list')return json(200,{list:{resources:[{name:'Cloud fixture',file_size:'100'}]}});
    if(u.pathname.startsWith('/drive/v1/tasks/')){const t=cloudTasks.get(u.pathname.split('/').at(-1));return t?json(200,{...t,phase:'PHASE_TYPE_COMPLETE',progress:100}):json(404,{error:'file_not_found'});}
    if(u.pathname==='/drive/v1/share'&&req.method==='POST'){const s={share_id:`share-${++sequence}`,share_url:'https://mypikpak.com/s/fixture-created',title:'Created share',file_num:String(body.file_ids.length),create_time:'2026-11-08T00:00:00Z',share_status:'OK',expiration_at:'-1',pass_code:'1234'};shares.set(s.share_id,s);return json(200,s);}
    if(u.pathname==='/drive/v1/share:batchDelete'){for(const id of body.ids)shares.delete(id);return json(200,{});}
    if(u.pathname==='/decompress/v1/list')return json(200,{status:'OK',files:[{index:'1',filename:'inside.txt',filesize:'12',kind:'drive#file',path:''}]});
    if(u.pathname==='/decompress/v1/decompress')return json(200,{status:'OK',task_id:'extract-1'});
    if(u.pathname==='/decompress/v1/progress')return json(200,{phase:'PHASE_TYPE_COMPLETE',progress:100,file_id:'folder-new'});
    if(u.pathname==='/drive/v1/events')return json(200,{events:[{id:'event-1',file_id:'file-1',reference_resource:files.get('file-1'),created_time:'2026-11-08T01:00:00Z'}]});
    if(u.pathname==='/drive/v1/share/list')return json(200,{data:[{share_id:'initial-share',share_url:'https://mypikpak.com/s/fixture',title:'测试分享',file_num:'1',create_time:'2026-11-08T00:00:00Z',share_status:'OK',expiration_at:'-1',view_count:'2',restore_count:'1'},...shares.values()]});
    return json(404,{error:'file_not_found'});
  }catch(error){violations.push(error.message);res.writeHead(500,{'content-type':'application/json'});res.end('{"error":"fixture_failure"}');}
});
await new Promise(ok=>server.listen(0,'127.0.0.1',ok));origin=`http://127.0.0.1:${server.address().port}`;
const portServer=createServer();await new Promise(ok=>portServer.listen(0,'127.0.0.1',ok));const cdpPort=portServer.address().port;await new Promise(ok=>portServer.close(ok));
const env={...process.env,NEKOPAK_BACKEND:'rust_compat',NEKOPAK_TEST_API_ORIGIN:origin,NEKOPAK_TEST_DATA_DIR:data,NEKOPAK_TEST_HIDE_WINDOW:'1',WEBVIEW2_USER_DATA_FOLDER:join(data,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${cdpPort} --remote-debugging-address=127.0.0.1`};
delete env.NEKOPAK_API_CLIENT_ID;delete env.NEKOPAK_API_CLIENT_SECRET;
const child=spawn(join(root,'target/debug/nekopak-webview.exe'),[],{cwd:root,env,windowsHide:true,stdio:['ignore','ignore','pipe']});let stderr='';child.stderr.on('data',b=>{stderr=(stderr+b).slice(-4096);});
let browser;let page;const results=[];
const wait=async(fn,ms=20000)=>{const end=Date.now()+ms;while(Date.now()<end){if(await fn())return;await new Promise(ok=>setTimeout(ok,100));}throw new Error('Timed out');};
const test=async(name,run)=>{await run();results.push({name,passed:true});console.log(`PASS ${name}`);};
try{
  await wait(async()=>{if(child.exitCode!==null)throw new Error(`App exited: ${stderr}`);try{return (await(await fetch(`http://127.0.0.1:${cdpPort}/json/list`)).json()).some(p=>p.url?.startsWith('http://tauri.localhost'));}catch{return false;}},40000);
  browser=await chromium.connectOverCDP(`http://127.0.0.1:${cdpPort}`);
  await wait(()=>{page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith('http://tauri.localhost'));return !!page;});page.setDefaultTimeout(20000);
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  const wire=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  const route=hash=>page.evaluate(hash=>{location.hash=hash;},hash);
  await page.evaluate(()=>localStorage.setItem('nekopak.language','zh-CN'));
  await wire('settings_update',{patch:{language:'zh-CN',download_dir:destination}});await page.reload();
  await test('native form signs in through Rust HTTP and DRIVE verification keeps the session',async()=>{
    await page.locator('input[name=username]').fill('fixture@example.test');await page.locator('input[name=password]').fill('synthetic-not-an-account');await page.locator('.signin').getByRole('button',{name:'登录',exact:true}).click();
    await page.getByRole('button',{name:'完成安全验证',exact:true}).first().waitFor();assert.equal((await wire('auth_status')).authenticated,true);
    await page.screenshot({path:join(root,'docs/screenshots/ui-v3/desktop-drive-verification.png')});
    await page.getByRole('button',{name:'完成安全验证',exact:true}).first().click();await wait(()=>existsSync(join(data,'fixture-verification.json')));
    const challenge=new URL(JSON.parse(readFileSync(join(data,'fixture-verification.json'),'utf8')).url);const callback=new URL(challenge.searchParams.get('redirect_uri'));assert.equal(callback.hostname,'127.0.0.1');
    callback.searchParams.set('state',challenge.searchParams.get('state'));callback.searchParams.set('captcha_token','human-fixture-token');callback.searchParams.set('expires_in','300');
    verifyRequired=false;assert.equal((await fetch(callback)).status,200);
    await page.getByRole('button',{name:/验证已收到，继续读取/}).first().click();await page.locator('.ft-row[data-id="file-1"]').waitFor();
    assert.equal((await wire('auth_status')).authenticated,true);
  });
  const mutate=(op,ids,target)=>wire('files_mutate',{op,ids,target,idempotency_key:randomUUID()});
  await test('native mkdir rename move star trash restore and recent are real service operations',async()=>{
    const created=await mutate('mkdir',[],{name:'新建文件夹',parent_id:'root'});const id=created.created[0].id;
    await mutate('rename',['file-1'],{name:'renamed.bin'});assert.equal((await wire('files_stat',{id:'file-1'})).name,'renamed.bin');
    await mutate('move',['file-1'],{parent_id:id});assert.equal((await wire('files_list',{parent_id:id})).items[0].id,'file-1');
    await mutate('star',['file-1']);await route('#/starred');await page.locator('.ft-row[data-id="file-1"]').waitFor();
    await mutate('trash',['file-1']);await route('#/trash');await page.locator('.ft-row[data-id="file-1"]').waitFor();
    await mutate('restore',['file-1']);await route('#/recent');await page.locator('.ft-row[data-id="file-1"]').waitFor();
    await route('#/shares');await page.getByText('测试分享',{exact:true}).waitFor();
    const quota=await wire('quota_get');assert.equal(quota.dimensions.find(d=>d.dimension==='download').used_bytes,'120');
  });
  await test('native tasks, shares, archives, MP4 export, loopback playback, mappings and settings complete through Rust',async()=>{
    await route('#/settings/transfers');await page.getByRole('button',{name:'选择文件夹',exact:true}).waitFor();await page.getByRole('button',{name:'保存',exact:true}).click();await wait(async()=> (await wire('settings_get')).download_dir===destination);
    await route('#/settings/network');await page.getByRole('switch',{name:'启用网络增强',exact:true}).click();await page.getByRole('radio',{name:'不使用',exact:true}).click();await page.getByRole('button',{name:'保存',exact:true}).click();await wait(async()=> (await wire('settings_get')).experimental_network.proxy_mode==='none');
    await page.getByRole('switch',{name:'屏蔽 access 域名',exact:true}).click();await page.getByRole('button',{name:'保存',exact:true}).click();await wait(async()=> (await wire('settings_get')).experimental_network.block_access_domains===true);
    const task=await wire('transfer_start',{remote_id:'file-1',destination,idempotency_key:randomUUID()});await route('#/transfers');
    await wait(async()=> (await wire('transfers_list')).items.find(t=>t.id===task.task_id)?.speed_bytes_per_sec>0,15000);
    const taskRow=page.locator(`.task[data-task="${task.task_id}"]`);
    await wait(async()=>/\d.*\/s/.test(await taskRow.locator('.task-speed').innerText()));
    await page.screenshot({path:join(root,'docs/screenshots/ui-v3/desktop-native-download-speed.png')});
    await wait(async()=> (await wire('transfers_list')).items.find(t=>t.id===task.task_id)?.state==='completed',45000);
    assert.deepEqual(readFileSync(join(destination,'renamed.bin')),bytes);
    const preview=await wire('cloud_task_preview',{source_url:'https://example.test/cloud'});assert.equal(preview.selection_supported,false);
    const cloud=await wire('cloud_task_create',{source_url:'https://example.test/cloud',parent_id:'root',preview_id:preview.preview_id,idempotency_key:randomUUID()});
    await wait(async()=>(await wire('transfers_list')).items.some(t=>t.id===cloud.task_id&&t.state==='completed'));
    const share=await wire('share_create',{ids:['file-1'],expires_in_days:7,with_code:true,idempotency_key:randomUUID()});assert.equal(share.code,'1234');
    assert.deepEqual((await wire('shares_cancel',{share_ids:[share.share_id],idempotency_key:randomUUID()})).succeeded,[share.share_id]);
    const listing=await wire('archive_list',{file_id:'archive-1'});assert.equal(listing.entries[0].name,'inside.txt');
    const extract=await wire('archive_extract',{file_id:'archive-1',paths:'all',target_parent_id:'root',idempotency_key:randomUUID()});assert.equal((await wire('jobs_list')).items.find(j=>j.job_id===extract.job_id).state,'completed');
    const media=await wire('media_probe',{remote_id:'video-1'});assert.equal(media.duration_ms,12000);
    const align=await wire('media_align',{remote_id:'video-1',quality_id:'original',start_ms:5000,end_ms:8500});assert.equal(align.start_ms,4000);
    const clip=await wire('transfer_start',{remote_id:'video-1',destination,idempotency_key:randomUUID(),options:{quality_id:'original',clip:{start_ms:5000,end_ms:8500,align:'previous_keyframe'}}});
    await wait(async()=>(await wire('transfers_list')).items.some(t=>t.id===clip.task_id&&t.state==='completed'),45000);
    const clipTask=(await wire('transfers_list')).items.find(t=>t.id===clip.task_id);assert.ok(existsSync(clipTask.destination));
    const decoded=spawnSync(ffmpeg,['-hide_banner','-loglevel','error','-xerror','-i',clipTask.destination,'-map','0:v','-map','0:a','-f','null','-'],{windowsHide:true});assert.equal(decoded.status,0,String(decoded.stderr));
    const stream=await wire('media_session_open',{remote_id:'video-1'});const streamed=await fetch(stream.url,{headers:{Range:'bytes=20-39',Origin:'http://tauri.localhost'}});assert.equal(streamed.status,206);assert.deepEqual(Buffer.from(await streamed.arrayBuffer()),mediaBytes.subarray(20,40));await wire('media_session_close',{session_id:stream.session_id});
    const mapped=await wire('local_sources_add',{path:data});assert.ok((await wire('files_list',{parent_id:mapped.root_id})).items.some(f=>f.name==='source.mp4'));await wire('local_sources_remove',{source_id:mapped.source_id});assert.ok(existsSync(mediaPath));
    await page.screenshot({path:join(root,'docs/screenshots/ui-v3/desktop-native-download-completed.png')});
    await page.reload();await wait(async()=> (await wire('settings_get')).download_dir===destination);
    await route('#/recent');await page.locator('.nav-item[aria-current=page]').waitFor();
    const colors=await page.evaluate(()=>({download:getComputedStyle(document.querySelector('.dl-btn')).backgroundColor,selected:getComputedStyle(document.querySelector('.nav-item[aria-current=page]')).backgroundColor}));
    assert.notEqual(colors.download,colors.selected);
    await page.screenshot({path:join(root,'docs/screenshots/ui-v3/desktop-navigation-colors.png')});
    await route('#/settings/general');
    await page.evaluate(()=>{window.__languageDocumentMarker='same-document';});
    await page.getByLabel('界面语言',{exact:true}).selectOption('en');
    await page.getByRole('heading',{name:'Appearance',exact:true}).waitFor();
    await page.getByRole('link',{name:'Recent',exact:true}).waitFor();
    assert.equal(await page.evaluate(()=>window.__languageDocumentMarker),'same-document');
    await route('#/settings/transfers');await page.getByRole('button',{name:'Choose folder',exact:true}).waitFor();
    await page.screenshot({path:join(root,'docs/screenshots/ui-v3/desktop-settings-english.png')});
    // Native persisted language is authoritative even if the WebView cache is
    // unavailable. This used to enter a reload loop during startup.
    await page.addInitScript(()=>{
      Object.defineProperty(window,'localStorage',{get(){throw new DOMException('Storage blocked','SecurityError');}});
    });
    await page.reload();
    // An unavailable preference store also loses the welcome acknowledgement.
    await page.getByRole('button',{name:'Explore first',exact:true}).click();
    await route('#/settings/transfers');
    await page.getByRole('button',{name:'Choose folder',exact:true}).waitFor();
    await page.getByRole('link',{name:'Recent',exact:true}).waitFor();
    await page.evaluate(()=>{window.__languageDocumentMarker='blocked-storage';});
    await route('#/settings/general');await page.getByLabel('Language',{exact:true}).selectOption('zh-CN');
    await page.getByRole('heading',{name:'外观',exact:true}).waitFor();
    await page.getByRole('link',{name:'最近',exact:true}).waitFor();
    assert.equal(await page.evaluate(()=>window.__languageDocumentMarker),'blocked-storage');
    assert.equal((await wire('desktop_status')).tray_available,true);
    await wire('settings_update',{patch:{close_to_tray:true}});
    await wire('plugin:window|close',{label:'main'});
    assert.equal((await wire('desktop_status')).window_visible,false);
    assert.equal((await wire('auth_status')).authenticated,true);
    await wire('auth_logout',{backend:'rust_compat'});assert.equal((await wire('transfers_list')).items.length,0);
  });
  assert.deepEqual(violations,[]);assert.deepEqual(errors,[]);
  writeFileSync(join(root,'docs/evidence/native-functional-e2e.json'),JSON.stringify({date:new Date().toISOString(),scope:'Real Windows WebView2 + native Rust HTTP, SQLite and file IO; synthetic local server, no account/service traffic',results,fixture_requests:requests.length,external_requests:0,download_bytes:bytes.length,errors},null,2));
}catch(error){if(page)await page.screenshot({path:join(root,'docs/screenshots/ui-v3/native-functional-failure.png')});throw error;}
finally{if(browser)await browser.close();if(child.exitCode===null)child.kill();server.closeAllConnections();await new Promise(ok=>server.close(ok));}
