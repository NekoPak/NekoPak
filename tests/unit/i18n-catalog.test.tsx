import {afterEach,expect,it} from 'vitest';
import {render} from 'preact-render-to-string';
import catalog from '../../src/lib/locales/catalog.json';
import {EN} from '../../src/lib/locales/en';
import {EN_EXTRA} from '../../src/lib/locales/en-extra';
import {ZH_TW} from '../../src/lib/locales/zh-tw';
import {JA} from '../../src/lib/locales/ja';
import {KO} from '../../src/lib/locales/ko';
import {setLanguage,resolveSystemLanguage,tx,tr,type Language} from '../../src/lib/i18n';
import {SettingsPage} from '../../src/pages/Settings';
import {Sidebar} from '../../src/app/Shell';
afterEach(()=>setLanguage('zh-CN'));
it('includes every literal product translation template in the catalog',()=>{
  const files=import.meta.glob('../../src/**/*.{ts,tsx}',{query:'?raw',import:'default',eager:true}) as Record<string,string>;
  const keys=new Set(catalog);
  for(const [file,source] of Object.entries(files)){
    if(file.includes('/locales/')||file.includes('/mock/'))continue;
    for(const match of source.matchAll(/\b(?:tr|tx)\(\s*("(?:[^"\\]|\\.)*")/g)){
      const key=JSON.parse(match[1]);
      expect(keys.has(key),`${file}: ${key}`).toBe(true);
    }
  }
});
it('has every product template in each language with unchanged positional parameters',()=>{
  for(const [locale,dictionary] of Object.entries({en:{...EN,...EN_EXTRA},'zh-TW':ZH_TW,'ja-JP':JA,'ko-KR':KO})){
    for(const key of catalog){
      expect(dictionary[key],`${locale}: ${key}`).toBeTypeOf('string');
      expect(dictionary[key].match(/\{\d+\}/g)?.sort()??[],`${locale}: parameters ${key}`).toEqual(key.match(/\{\d+\}/g)?.sort()??[]);
    }
  }
});
it('detects supported system locales including traditional Chinese variants',()=>{
  for(const value of ['zh-TW','zh-Hant','zh-HK','zh-MO'])expect(resolveSystemLanguage(value)).toBe('zh-TW');
  expect(resolveSystemLanguage('zh-Hans')).toBe('zh-CN');expect(resolveSystemLanguage('ja')).toBe('ja-JP');expect(resolveSystemLanguage('ko')).toBe('ko-KR');expect(resolveSystemLanguage('fr-FR')).toBe('en');
});
it('changes navigation and settings in all languages without translating inserted file names',()=>{
  for(const locale of ['zh-CN','zh-TW','en','ja-JP','ko-KR'] as Language[]){
    setLanguage(locale);
    expect(render(<Sidebar collapsed={false}/>)).toContain(tr('我的分享'));
    expect(render(<SettingsPage section="general"/>)).toContain(tr('界面语言'));
    const privateName='下载 旅行 사진 動画 😀.mkv';
    expect(tx('打开“{0}”',privateName)).toContain(privateName);
  }
});
