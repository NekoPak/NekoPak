import {afterEach,expect,it,vi} from 'vitest';
import {render} from 'preact-render-to-string';
import {tr,setLanguage,resolvedLanguage} from '../../src/lib/i18n';
import {SettingsPage} from '../../src/pages/Settings';
import {app,updateSettings} from '../../src/state/app';
import {api} from '../../src/lib/api';
import {Sidebar} from '../../src/app/Shell';
import {MockTransport} from '../../src/lib/mock/backend';
import {formatDuration} from '../../src/lib/format';
const original=app.get();
afterEach(()=>{vi.restoreAllMocks();vi.unstubAllGlobals();setLanguage('zh-CN');app.replace(original);});
it('changes real settings labels and time formatting while leaving file names intact',()=>{
  setLanguage('en');app.set({runtime:'tauri'});
  expect(resolvedLanguage()).toBe('en');expect(tr('选择文件夹')).toBe('Choose folder');
  expect(render(<SettingsPage section="transfers"/>)).toContain('Choose folder');
  expect(formatDuration(65)).toBe('1 min 5 s');
  expect(tr('用户文档.zip')).toBe('用户文档.zip');
  setLanguage('zh-CN');expect(tr('选择文件夹')).toBe('选择文件夹');
});
it('updates navigation labels after modules have loaded',()=>{
  setLanguage('en');
  expect(render(<Sidebar collapsed={false}/>)).toContain('aria-label="Recent"');
  expect(render(<SettingsPage section="general"/>)).toContain('<span>Appearance</span>');
  setLanguage('zh-CN');
  expect(render(<Sidebar collapsed={false}/>)).toContain('aria-label="最近"');
});
it('applies a saved language even when browser storage is unavailable, without reloading',async()=>{
  const demo=new MockTransport('free',{latency_ms:0});
  const settings=await demo.invoke('settings_get',undefined);demo.dispose();
  vi.spyOn(api,'settingsUpdate').mockResolvedValue({...settings,language:'en'});
  const reload=vi.fn();vi.stubGlobal('location',{reload});
  vi.stubGlobal('document',{documentElement:{lang:'',setAttribute:vi.fn(),removeAttribute:vi.fn()}});
  vi.stubGlobal('localStorage',{setItem:()=>{throw new Error('storage blocked');}});
  setLanguage('zh-CN');
  await updateSettings({language:'en'});
  expect(resolvedLanguage()).toBe('en');
  expect(reload).not.toHaveBeenCalled();
});
it('retains the selected demo language when recreating the browser backend',async()=>{
  setLanguage('en');
  const demo=new MockTransport('free',{latency_ms:0});
  try{expect((await demo.invoke('settings_get',undefined)).language).toBe('en');}
  finally{demo.dispose();}
});
