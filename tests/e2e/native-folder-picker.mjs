import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {mkdirSync,writeFileSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {randomUUID} from 'node:crypto';
import {chromium} from 'playwright';
import assert from 'node:assert/strict';
const root=resolve(import.meta.dirname,'../..');const data=join(root,'target',`folder-picker-${randomUUID()}`);const destination=join(data,'选择目录 sample');mkdirSync(destination,{recursive:true});
const reserved=createServer();await new Promise(ok=>reserved.listen(0,'127.0.0.1',ok));const port=reserved.address().port;await new Promise(ok=>reserved.close(ok));
const env={...process.env,NEKOPAK_BACKEND:'rust_compat',NEKOPAK_TEST_HIDE_WINDOW:'1',NEKOPAK_TEST_DATA_DIR:data,WEBVIEW2_USER_DATA_FOLDER:join(data,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`};
delete env.NEKOPAK_API_CLIENT_ID;delete env.NEKOPAK_API_CLIENT_SECRET;
const child=spawn(join(root,'target/debug/nekopak-webview.exe'),[],{cwd:root,env,windowsHide:true,stdio:'ignore'});let browser;
const poll=async(fn)=>{const until=Date.now()+20000;while(Date.now()<until){if(await fn())return;await new Promise(ok=>setTimeout(ok,100));}throw new Error('timeout');};
const control=(action)=>new Promise((ok,fail)=>{const p=spawn('powershell.exe',['-NoProfile','-File',join(root,'tests/e2e/folder-dialog.ps1'),'-TargetProcessId',String(child.pid),'-Action',action,'-Directory',destination],{windowsHide:true});let out='';p.stdout.on('data',b=>out+=b);p.stderr.on('data',b=>out+=b);p.on('exit',code=>code===0?ok(out):fail(new Error(out)));});
try{
  await poll(async()=>{try{return (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).some(p=>p.url?.startsWith('http://tauri.localhost'));}catch{return false;}});
  browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`);let page;await poll(()=>{page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith('http://tauri.localhost'));return !!page;});
  const wire=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  await wire('settings_update',{patch:{language:'zh-CN'}});await page.evaluate(()=>localStorage.setItem('nekopak.language','zh-CN'));await page.reload();
  await page.getByRole('button',{name:'先看看',exact:true}).click();await page.evaluate(()=>{location.hash='#/settings/transfers';});
  const previous=await page.locator('#native-download-dir').inputValue();
  await page.getByRole('button',{name:'选择文件夹',exact:true}).click();const cancelled=await control('cancel');
  await poll(async()=>await page.getByRole('button',{name:'选择文件夹',exact:true}).getAttribute('aria-busy')!=='true');assert.equal(await page.locator('#native-download-dir').inputValue(),previous);
  await page.getByRole('button',{name:'选择文件夹',exact:true}).click();const selected=await control('choose');
  await poll(async()=> (await page.locator('#native-download-dir').inputValue()).replaceAll('\\','/').toLowerCase()===destination.replaceAll('\\','/').toLowerCase());
  await page.getByRole('button',{name:'保存',exact:true}).click();await poll(async()=> (await wire('settings_get')).download_dir.replaceAll('\\','/').toLowerCase()===destination.replaceAll('\\','/').toLowerCase());
  await page.screenshot({path:join(root,'docs/screenshots/ui-v3/desktop-native-folder-selected.png')});
  writeFileSync(join(root,'docs/evidence/native-folder-picker.json'),JSON.stringify({date:new Date().toISOString(),scope:'Actual Windows IFileOpenDialog in isolated debug process; native UIAutomation used because Computer Use kernel could not initialize',cancelled:JSON.parse(cancelled),selected:JSON.parse(selected),persisted:true},null,2));console.log('PASS native Windows directory dialog selection, cancel and persistence');
}finally{if(browser)await browser.close();if(child.exitCode===null)child.kill();}
