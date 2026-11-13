#!/usr/bin/env node
// Browser IPC fixtures validate mobile UI only, never Android storage or live accounts.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { chromium } from "playwright";

const root = process.cwd(), output = resolve(root, "target/mobile-ui");
mkdirSync(output, { recursive: true });
const port = 4183, base = process.env.E2E_BASE ?? `http://localhost:${port}`;
let server;
if (!process.env.E2E_BASE) {
  server = spawn(process.execPath, ["node_modules/vite/bin/vite.js", "preview", "--port", String(port), "--strictPort"], { cwd: root, stdio: "pipe", windowsHide: true });
  await new Promise((ok, fail) => {
    const timeout = setTimeout(() => fail(new Error("Vite preview did not start")), 20000);
    server.stdout.on("data", chunk => { if (String(chunk).includes(String(port))) { clearTimeout(timeout); ok(); } });
    server.on("exit", code => { clearTimeout(timeout); fail(new Error(`Vite exited: ${code}`)); });
  });
}
const browser = await chromium.launch(process.env.E2E_BROWSER_PATH ? { executablePath: process.env.E2E_BROWSER_PATH } : { channel: process.env.E2E_BROWSER_CHANNEL ?? (process.platform === "win32" ? "msedge" : undefined) });
const results = [], errors = [];

function fixture({ signedIn, mobile = true, theme = "light" }) {
  localStorage.setItem("nekopak.disclaimer.v1", "1");
  localStorage.setItem("nekopak.language", "zh-CN");
  let signed = signedIn, id = 0;
  let authPrefs={remember_account:true,restore_session:true,username:"remembered@example.test"};
  const callbacks = new Map();
  const now = "2026-11-09T08:00:00Z";
  const folder = { id: "folder-a", parent_id: "root", name: "手机测试文件夹", kind: "folder", size_bytes: "0" };
  const file = { id: "file-a", parent_id: "root", name: "示例文件.txt", kind: "file", size_bytes: "12" };
  let task = { id: "completed-a", remote_id: "file-a", backend: "rust_compat", name: file.name, kind: "download", state: "completed", destination: "/data/user/0/io.nekopak.desktop/files/Downloads/示例文件.txt", transferred_bytes: "12", total_bytes: "12", revision: "1", simulated: false, created_at: now, updated_at: now };
  let tasks=[task,{...task,id:"running-a",name:"旅行视频.mp4",state:"running",transferred_bytes:"4194304",total_bytes:"41943040",speed_bytes_per_sec:2097152},{...task,id:"cancelled-a",name:"已取消的备份.zip",state:"cancelled",transferred_bytes:"0"}];
  const settings = { schema_version: 1, theme, language: "zh-CN", download_dir: "/data/user/0/io.nekopak.desktop/files/Downloads", max_concurrent_transfers: 1, bandwidth_limit_kib_s: 0, ask_download_directory: false, close_to_tray: false, background_transfers: true, cache: { dir: "", max_bytes: "536870912" }, experimental_network: { enabled: false, proxy_mode: "system", user_proxy_configured: false, custom_dns_mode: false, connection_diagnostics: false }, telemetry: false, download: { default_quality: "original", connections_per_task: 1 }, media: { player_path: "" } };
  window.__mobileFixture = { calls: [], exportResult: false, torrentResult: null, receivedBytes:"4194304", updatePhase:"idle" };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
    transformCallback: callback => { callbacks.set(++id, callback); return id; },
    unregisterCallback: key => callbacks.delete(key),
    invoke: async (command, args = {}) => {
      // Never retain even synthetic password values in fixture diagnostics.
      window.__mobileFixture.calls.push({ command, args: command === "auth_sign_in" ? {} : structuredClone(args) });
      if (command === "plugin:event|listen") return ++id;
      if (command === "plugin:event|unlisten") return;
      if (command.startsWith("plugin:window|")) return false;
      if (command === "app_status") return { version: "mobile-fixture", contract_version: "0.2", mode: "rust_compat_preview", storage: { status: "ready", schema_version: 1 }, features: { mobile, app_updates:true, torrent_import: true, desktop_window: !mobile, tray: !mobile, directory_picker: !mobile, player_config: !mobile, media_external: !mobile, transfer_open_directory: !mobile, transfer_export: mobile, background_transfers: true, auth_sign_in: true, auth_logout: true, settings: true, ui_prefs: true, media_session: true, upload: !mobile, upload_directory: !mobile, local_sources: false, sync: false, mount: false, autostart: false } };
      if(command==='update_status'||command==='update_check'){
        if(command==='update_check')window.__mobileFixture.updatePhase='available';
        const phase=window.__mobileFixture.updatePhase;
        return {supported:true,phase,current_version:'0.1.3',channel:'preview',checked_at:now,downloaded_bytes:phase==='ready'?'12345':'0',error:null,update:phase==='idle'?null:{update_id:'update-fixture',version:'0.1.4',channel:'preview',notes:'Synthetic signed update notes',published_at:now,size_bytes:'12345'}};
      }
      if(command==='update_download'){window.__mobileFixture.updatePhase='downloading';setTimeout(()=>{window.__mobileFixture.updatePhase='ready';},400);return;}
      if(command==='update_cancel'){window.__mobileFixture.updatePhase='available';return;}
      if(command==='update_install'){
        if(window.__mobileFixture.updateInstallError)throw {code:'INTEGRITY_FAILED',message:'测试更新包无法安装',retryable:false};
        return {state:'permission_required'};
      }
      if(command === "auth_preferences_get")return authPrefs;
      if(command === "cloud_torrent_pick")return window.__mobileFixture.torrentResult;
      if(command === "cloud_torrent_create")return {task_id:"torrent-cloud-a"};
      if(command === "cloud_torrent_release")return;
      if(command === "auth_preferences_update"){authPrefs=args.preferences;return authPrefs;}
      if(command === "network_reconnect")return;
      if(command === "network_domains_probe"){const selected=settings.experimental_network.server_domain??"auto";return {selected_domain:selected,active_domain:selected==="auto"?"mypikpak.com":selected,checked_at:now,results:["mypikpak.com","mypikpak.net","pikpak.me","pikpakdrive.com"].map((domain,index)=>({domain,usable:true,latency_ms:42+index*10,warm_latency_ms:12+index*5}))};}
      if (command === "auth_status") return { backend: "rust_compat", authenticated: signed, state: signed ? "signed_in" : "signed_out", account_id: signed ? "synthetic-account" : undefined, display_name: "fixture@example.test", credential_owner: "os_vault", account_tier: "unknown", checked_at: now };
      if (command === "auth_sign_in") {
        if (args.password === "synthetic-rejected") throw { code: "PERMISSION_DENIED", message: "测试凭据未通过", retryable: false, details: { reason: "invalid_credentials" } };
        signed = true; return { state: "signed_in" };
      }
      if (command === "backend_discover") return [{ backend: "rust_compat", status: signed ? "ready" : "needs_login", operations: { list: true, stat: true, search: true, mkdir: true, rename: true, upload: true, download: true, cloud_task: true, trash: true, move: true, copy: true, star: true, external_player: true }, limitations: [], checked_at: now }];
      if (command === "settings_get") return settings;
      if (command === "settings_update") return Object.assign(settings, args.patch);
      if (command === "ui_prefs_get") return { account_id: "synthetic-account", prefs: { pins: [] } };
      if (command === "ui_prefs_update") return args.prefs;
      if (command === "quota_get") return { backend: "rust_compat", source: "account", account_tier: "unknown", dimensions: [], limitations: [], checked_at: now };
      if (command === "transfers_list") return { items: signed ? tasks.map(task=>task.id==='running-a'?{...task,received_bytes:window.__mobileFixture.receivedBytes}:task) : [] };
      if (command === "jobs_list" || command === "shares_list") return { items: [] };
      if (command === "files_list") return { items: args.parent_id === "root" ? [folder, {...folder,id:"folder-b",name:"工作文档"}, {...file,id:"video-a",name:"旅行视频.mp4",size_bytes:"41943040",mime_type:"video/mp4"}, {...file,id:"image-a",name:"海边的日落.jpg",size_bytes:"2419401",mime_type:"image/jpeg"}, file] : [{ ...file, id: "nested", parent_id: folder.id }] };
      if(command === "files_search")return {items:[file]};
      if(command === "cloud_task_create")return {task_id:"cloud-a"};
      if(command === "cloud_task_preview")throw {code:"UNSUPPORTED",message:"fixture preview unavailable",retryable:false};
      if(command === "transfer_forget"){tasks=tasks.filter(t=>t.id!==args.task_id);return;}
      if(command === "transfer_clear_finished"){const before=tasks.length;tasks=tasks.filter(t=>!["completed","cancelled","failed"].includes(t.state));return {removed:before-tasks.length};}
      if(command === "transfer_control"){const task=tasks.find(t=>t.id===args.task_id);if(task)task.state=args.action==="pause"?"paused":args.action==="cancel"?"cancelled":"running";return;}
      if (command === "files_path") return args.id === folder.id ? [folder] : [];
      if (command === "files_stat") return args.id === folder.id ? folder : file;
      if (command === "transfer_export") return window.__mobileFixture.exportResult;
      throw { code: "UNSUPPORTED", message: `Mobile fixture: ${command}`, retryable: false };
    },
  };
}

async function assertFits(page, name) {
  const overflow = await page.evaluate(() => {
    const selectors = ["html", "body", ".workspace", ".pf-body", ".canvas", ".settings-layout", ".settings-body", ".ob-island", ".ft-scroll", ".task-scroll"];
    return selectors.flatMap(selector => [...document.querySelectorAll(selector)].filter(el => el.getClientRects().length && el.scrollWidth > el.clientWidth + 2).map(el => ({ selector, width: el.clientWidth, content: el.scrollWidth })));
  });
  assert.deepEqual(overflow, [], `${name} has horizontal content overflow`);
}

async function context(width, signedIn = true, mobile = true, theme = "light") {
  const ctx = await browser.newContext({ viewport: { width, height: 844 }, hasTouch: true, isMobile: true, locale: "zh-CN", reducedMotion: "reduce" });
  await ctx.addInitScript(fixture, { signedIn, mobile, theme });
  const page = await ctx.newPage();
  page.on("pageerror", error => errors.push(error.message));
  await page.route("**/*", route => {
    const url = route.request().url();
    if (url.startsWith(base) || /^(data|blob):/.test(url)) return route.continue();
    errors.push(`Unexpected external request: ${url}`); return route.abort();
  });
  await page.goto(base);
  return { ctx, page };
}

try {
  for (const width of [360,390,412]) for(const theme of ["light","dark"]) {
    const {ctx,page}=await context(width,true,true,theme);
    try {
      await page.locator('[data-id="folder-a"]').waitFor();
      assert.equal(await page.locator('.sidebar,.workspace-tabs,.pf-commands,.address-drop').count(),0,"desktop navigation must not exist in phone markup");
      assert.equal(await page.locator('.mobile-bottom-nav button').count(),3);
      assert.equal(await page.locator('.mobile-file-row .tile').count(),5,"file types retain visible icons outside selection mode");
      await assertFits(page,`${width} ${theme} files`);
      await page.waitForTimeout(220); await page.screenshot({path:resolve(output,`files-${width}-${theme}.png`)});
      await page.getByRole('button',{name:'打开 手机测试文件夹',exact:true}).tap();
      await page.waitForURL('**/#/files/folder-a');
      await page.getByRole('button',{name:'返回上一级',exact:true}).tap();
      await page.getByRole('button',{name:'选择',exact:true}).tap();
      const select=page.getByRole('button',{name:'选择 手机测试文件夹',exact:true});
      await select.focus();await page.keyboard.press('Enter');assert.equal(await select.getAttribute('aria-pressed'),'true');
      await page.keyboard.press('Space');assert.equal(await select.getAttribute('aria-pressed'),'false');
      await page.getByRole('button',{name:'取消',exact:true}).tap();
      const longTarget=page.getByRole('button',{name:'打开 手机测试文件夹',exact:true});
      const box=await longTarget.boundingBox();await page.mouse.move(box.x+30,box.y+30);await page.mouse.down();await page.waitForTimeout(520);await page.mouse.up();
      assert.equal(await page.getByRole('button',{name:'选择 手机测试文件夹',exact:true}).getAttribute('aria-pressed'),'true','long press selects without opening');
      assert.equal(await page.evaluate(()=>!window.dispatchEvent(new CustomEvent('nekopak:back',{cancelable:true}))),true);
      await page.locator('.mobile-selection-actions').waitFor({state:'detached'});
      await page.getByRole('button',{name:'排序与筛选',exact:true}).tap();await page.getByRole('button',{name:'网格视图',exact:true}).tap();await page.locator('.mobile-file-grid').waitFor();
      await page.getByRole('button',{name:'排序与筛选',exact:true}).tap();await page.getByRole('button',{name:'列表视图',exact:true}).tap();
      await page.getByRole('button',{name:'示例文件.txt的更多操作',exact:true}).tap();
      await page.getByRole('dialog',{name:'示例文件.txt',exact:true}).waitFor();
      await page.waitForTimeout(220); await page.screenshot({path:resolve(output,`actions-${width}-${theme}.png`)});
      assert.equal(await page.evaluate(()=>!window.dispatchEvent(new CustomEvent('nekopak:back',{cancelable:true}))),true);
      await page.getByRole('button',{name:'搜索',exact:true}).tap();
      await page.getByRole('searchbox',{name:'搜索云盘文件'}).fill('示例');await page.keyboard.press('Enter');
      await page.locator('.mobile-file-row').waitFor();
      await page.getByRole('button',{name:'文件',exact:true}).tap();
      await page.getByRole('button',{name:'新建',exact:true}).tap();await page.getByRole('button',{name:'添加链接',exact:true}).tap();
      await page.getByRole('dialog',{name:'添加链接',exact:true}).waitFor();
      await page.waitForTimeout(220); await page.screenshot({path:resolve(output,`cloud-dialog-${width}-${theme}.png`)});
      await page.getByRole('tab',{name:'种子文件',exact:true}).tap();
      await page.getByRole('button',{name:'选择种子文件',exact:true}).tap();
      assert.equal(await page.getByRole('button',{name:'提交云下载',exact:true}).isDisabled(),true);
      await page.evaluate(()=>{window.__mobileFixture.torrentResult={import_id:'fixture-torrent',title:'test.bin',info_hash:'a'.repeat(40),total_bytes:'3',file_count:1,files:[{path:'test.bin',size_bytes:'3'}],private:false,expires_at:'2099-01-01T00:00:00Z'};});
      await page.getByRole('button',{name:'选择种子文件',exact:true}).tap();
      await page.getByText('将提交整个种子中的文件。',{exact:true}).waitFor();
      await page.screenshot({path:resolve(output,`torrent-${width}-${theme}.png`)});
      await page.getByRole('button',{name:'提交云下载',exact:true}).tap();
      await page.getByRole('dialog',{name:'添加链接',exact:true}).waitFor({state:'detached'});
      assert(await page.evaluate(()=>window.__mobileFixture.calls.some(call=>call.command==='cloud_torrent_create'&&call.args.import_id==='fixture-torrent')));
      await page.getByRole('button',{name:'传输',exact:true}).tap();
      await page.locator('[data-task="running-a"]').waitFor();
      assert((await page.locator('[data-task="running-a"] .phone-task-stats strong').textContent()).includes('/s'),"measured speed is visible");
      await page.evaluate(()=>{window.__mobileFixture.receivedBytes='6291456';});
      await page.waitForFunction(()=>Number(document.querySelector('[data-task="running-a"] [role="progressbar"]').getAttribute('aria-valuenow'))>=15);
      assert((await page.locator('[data-task="running-a"] .phone-task-stats').textContent()).includes('15.0%'));
      await page.waitForTimeout(220); await page.screenshot({path:resolve(output,`transfers-${width}-${theme}.png`)});
      await assertFits(page,`${width} ${theme} transfers`);
      await page.getByRole('button',{name:'已取消的备份.zip的更多操作',exact:true}).tap();await page.getByRole('button',{name:'移除记录',exact:true}).tap();
      await page.locator('[data-task="cancelled-a"]').waitFor({state:'detached'});
      await page.getByRole('button',{name:'导出文件',exact:true}).tap();assert.equal(await page.getByText('文件已导出',{exact:true}).count(),0);
      await page.evaluate(()=>{window.__mobileFixture.exportResult=true;});await page.getByRole('button',{name:'导出文件',exact:true}).tap();await page.getByText('文件已导出',{exact:true}).waitFor();
      await page.getByRole('button',{name:'我的',exact:true}).tap();await page.locator('.mobile-account').waitFor();
      assert.equal(await page.getByRole('button',{name:'挂载',exact:true}).count(),0);
      await page.waitForTimeout(220); await page.screenshot({path:resolve(output,`me-${width}-${theme}.png`)});
      await page.getByRole('button',{name:'外观',exact:true}).tap();await page.locator('.phone-themes').waitFor();
      assert.equal(await page.locator('.settings-nav').count(),0);assert.equal(await page.getByRole('switch',{name:'关闭窗口时最小化到托盘'}).count(),0);
      await page.waitForTimeout(220); await page.screenshot({path:resolve(output,`appearance-${width}-${theme}.png`)});
      await page.getByRole('button',{name:'返回',exact:true}).tap();await page.getByRole('button',{name:'下载与传输',exact:true}).tap();
      assert.equal(await page.getByRole('button',{name:'选择播放器',exact:true}).count(),0);
      const keepAlive=page.getByRole('switch',{name:'后台继续传输'});assert.equal(await keepAlive.getAttribute('aria-checked'),'true');
      await page.getByText('通过系统通知继续运行',{exact:false}).waitFor();
      await page.waitForTimeout(220); await page.screenshot({path:resolve(output,`settings-${width}-${theme}.png`)});
      await assertFits(page,`${width} ${theme} settings`);
      await keepAlive.tap();await page.getByText('切到后台或锁屏会暂停本地传输',{exact:false}).waitFor();
      await page.getByRole('button',{name:'保存',exact:true}).tap();
      await page.waitForFunction(()=>window.__mobileFixture.calls.some(call=>call.command==='settings_update'&&call.args.patch?.background_transfers===false));
      await keepAlive.tap();await page.getByRole('button',{name:'保存',exact:true}).tap();
      await page.waitForFunction(()=>window.__mobileFixture.calls.filter(call=>call.command==='settings_update').at(-1)?.args.patch?.background_transfers===true);
      await page.getByRole('button',{name:'返回',exact:true}).tap();await page.getByRole('button',{name:'网络',exact:true}).tap();
      await page.locator('#server-domain').selectOption('mypikpak.net');
      await page.getByRole('button',{name:'保存',exact:true}).tap();
      await page.getByRole('button',{name:'测速',exact:true}).tap();await page.getByText('42 ms',{exact:true}).waitFor();
      await page.getByRole('button',{name:'重新连接',exact:true}).tap();await page.getByText('已重新连接',{exact:true}).waitFor();
      await page.waitForTimeout(220);await page.screenshot({path:resolve(output,`network-${width}-${theme}.png`)});
      const calls=await page.evaluate(()=>window.__mobileFixture.calls);
      assert(calls.some(c=>c.command==='settings_update'&&c.args.patch?.experimental_network?.server_domain==='mypikpak.net'));
      assert(calls.some(c=>c.command==='network_domains_probe'));assert(calls.some(c=>c.command==='network_reconnect'));
      await page.getByRole('button',{name:'我的',exact:true}).tap();await page.getByRole('button',{name:'关于',exact:true}).tap();
      await page.getByRole('switch',{name:'启动时检查更新',exact:true}).tap();
      assert(await page.evaluate(()=>window.__mobileFixture.calls.some(call=>call.command==='settings_update'&&call.args.patch?.updates?.auto_check===true)));
      await page.getByRole('button',{name:'检查更新',exact:true}).tap();await page.getByText('Synthetic signed update notes',{exact:true}).waitFor();
      await page.getByRole('button',{name:'下载更新',exact:true}).tap();await page.getByText('更新包已验证，可以安装。',{exact:true}).waitFor();
      if(width===390&&theme==='light'){
        await page.evaluate(()=>{window.__mobileFixture.updateInstallError=true;});
        await page.getByRole('button',{name:'安装更新',exact:true}).tap();
        await page.getByRole('dialog',{name:'安装更新',exact:true}).getByRole('button',{name:'安装更新',exact:true}).tap();
        await page.getByText('测试更新包无法安装',{exact:true}).waitFor();
        await page.waitForTimeout(1200);
        assert(await page.getByText('测试更新包无法安装',{exact:true}).isVisible(),'status polling must not erase an install error');
        await page.evaluate(()=>{window.__mobileFixture.updateInstallError=false;});
      }
      await page.getByRole('button',{name:'安装更新',exact:true}).tap();
      await page.getByRole('dialog',{name:'安装更新',exact:true}).getByRole('button',{name:'安装更新',exact:true}).tap();
      await page.getByText('请允许本应用安装更新，返回后再次点击安装。',{exact:true}).waitFor();
      await page.screenshot({path:resolve(output,`updates-${width}-${theme}.png`)});
      await page.getByRole('button',{name:'传输',exact:true}).tap();await page.locator('[data-task="completed-a"]').waitFor();
      await page.getByRole('button',{name:'管理传输',exact:true}).tap();await page.getByRole('button',{name:'清理已结束',exact:true}).tap();
      const confirmation=page.getByRole('dialog',{name:'清理已结束',exact:true});await confirmation.getByText('保留已下载文件与云端文件',{exact:true}).waitFor();
      await confirmation.getByRole('button',{name:'清理已结束',exact:true}).tap();await page.locator('[data-task="completed-a"]').waitFor({state:'detached'});
      assert.equal(await page.locator('[data-task="running-a"]').count(),1);

      results.push({name:`phone ${width}px ${theme}`,passed:true});
    }catch(error){await page.waitForTimeout(220); await page.screenshot({path:resolve(output,`failure-${width}-${theme}.png`)});throw error;}finally{await ctx.close();}
  }
  for(const width of [360,390,412]) for(const theme of ['light','dark']){
    const {ctx,page}=await context(width,false,true,theme);
    try{await page.getByRole('heading',{name:'登录 PikPak',exact:true}).waitFor();assert.equal(await page.locator('.onboarding').count(),0);await page.waitForTimeout(220); await page.screenshot({path:resolve(output,`login-${width}-${theme}.png`)});assert.equal(await page.locator('input[name="username"]').inputValue(),'remembered@example.test');assert.equal(await page.getByRole('checkbox',{name:'保持登录',exact:true}).isChecked(),true);await page.locator('input[name="username"]').fill('synthetic@example.test');await page.locator('input[name="password"]').fill('synthetic-rejected');await page.getByRole('button',{name:'登录',exact:true}).tap();await page.getByText('测试凭据未通过',{exact:true}).waitFor();assert.equal(await page.locator('input[name="password"]').inputValue(),'');await assertFits(page,'phone login');results.push({name:`phone login ${width}px ${theme}`,passed:true});}finally{await ctx.close();}
  }
  assert.deepEqual(errors,[]);
  writeFileSync(resolve(output,'results.json'),JSON.stringify({scope:'Independent mobile UI/browser IPC fixture; no device or live-service claims',results,errors},null,2));
  console.log(JSON.stringify({passed:results.length,errors,output}));
}finally{await browser.close();server?.kill();}
