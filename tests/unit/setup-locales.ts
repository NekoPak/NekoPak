// Unit tests translate synchronously; the app loads catalogs on demand.
import { registerLocale } from "../../src/lib/i18n";
import { EN } from "../../src/lib/locales/en";
import { EN_EXTRA } from "../../src/lib/locales/en-extra";
import { ZH_TW } from "../../src/lib/locales/zh-tw";
import { JA } from "../../src/lib/locales/ja";
import { KO } from "../../src/lib/locales/ko";

registerLocale("en", [EN_EXTRA, EN]);
registerLocale("zh-TW", [ZH_TW]);
registerLocale("ja-JP", [JA]);
registerLocale("ko-KR", [KO]);
