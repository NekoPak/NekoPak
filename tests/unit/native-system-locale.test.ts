import {afterEach,expect,it,vi} from "vitest";
import {resolvedLanguage,setLanguage,setSystemLanguageOverride} from "../../src/lib/i18n";

afterEach(()=>{setSystemLanguageOverride(undefined);setLanguage("zh-CN");vi.unstubAllGlobals();});

it("uses the Android configured locale when WebView reports unrelated English",()=>{
  vi.stubGlobal("navigator",{language:"en-US"});
  vi.stubGlobal("document",{documentElement:{lang:""}});
  setLanguage("system");
  expect(resolvedLanguage()).toBe("en");
  setSystemLanguageOverride("zh-Hans-CN");
  expect(resolvedLanguage()).toBe("zh-CN");
  setLanguage("ja-JP");
  expect(resolvedLanguage()).toBe("ja-JP");
  setLanguage("system");setSystemLanguageOverride("zh_Hant_TW");
  expect(resolvedLanguage()).toBe("zh-TW");
});
