import {expect,it,vi} from 'vitest';

it('loads saved network settings before the first authentication and discovery calls',async()=>{
  const calls:string[]=[];
  vi.stubGlobal('window',new EventTarget());
  vi.stubGlobal('document',{addEventListener:vi.fn(),documentElement:{lang:'',setAttribute:vi.fn(),removeAttribute:vi.fn()}});
  vi.stubGlobal('location',{reload:()=>{throw new Error('boot must not reload');}});
  vi.doMock('../../src/lib/api',()=>({
    api:{
      appStatus:async()=>({mode:'rust_compat_preview',feature_flags:{settings:true}}),
      settingsGet:async()=>{calls.push('settings');return {theme:'system',language:'en'};},
      backendDiscover:async()=>{calls.push('discover');return [];},
      authStatus:async()=>{calls.push('auth');return {state:'signed_out'};},
    },
    transportKind:async()=>'tauri',on:async()=>()=>{},writeLocal:()=>{},errorOf:(e:unknown)=>e,
  }));
  vi.doMock('../../src/state/transfers',()=>({scheduleReload:()=>{},startTransfers:async()=>{}}));
  vi.doMock('../../src/state/session',()=>({resetSessionUi:()=>{},onSessionReset:()=>()=>{}}));
  try{
    const {app,boot}=await import('../../src/state/app');
    await boot();
    expect(calls[0]).toBe('settings');
    expect(calls).toEqual(['settings','discover','auth']);
    expect(app.get().boot).toBe('ready');
  }finally{vi.unstubAllGlobals();}
});
