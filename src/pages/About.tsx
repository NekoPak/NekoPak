import { tr } from "../lib/i18n";
import { useStore } from "../lib/store";
import { app } from "../state/app";
import { LogoMark } from "../components/Brand";
import { Btn, openOfficial } from "../components/ui";
import { AppUpdates } from "../components/AppUpdates";

/**
 * Mirrors docs/04_SOURCES_LICENSES.md. "关系" reflects what the UI code
 * actually does today: no upstream source code is copied into src/.
 */
const SOURCES: { name: string; url: string; license: string; relation: string }[] = [
  { name: "Piko（NihilDigit）", url: "https://github.com/NihilDigit/piko", license: "AGPL-3.0-or-later", get relation() { return tr("界面与交互设计参考（圆角岛外框、侧栏、任务页布局），未复制代码或素材"); } },
  { name: "pikpaktui（Bengerthelorf）", url: "https://github.com/Bengerthelorf/pikpaktui", license: "Apache-2.0", get relation() { return tr("PikPak 接口适配参考（Rust）"); } },
  { name: "pikpakcli（52funny）", url: "https://github.com/52funny/pikpakcli", license: "MIT", get relation() { return tr("PikPak 接口行为研究"); } },
  { name: "PikPak Enhancement Master（digbug82）", url: "https://github.com/digbug82/PikPak_Enhancement_Master", license: "AGPL-3.0", get relation() { return tr("便利功能参考，未复制代码"); } },
  { name: "pikpak-kotlin（NihilDigit）", url: "https://github.com/NihilDigit/pikpak-kotlin", license: "MIT", get relation() { return tr("行为与边界条件参考"); } },
  { name: "MediaMP（open-ani）", url: "https://github.com/open-ani/mediamp", license: "Apache-2.0", get relation() { return tr("播放交互设计参考"); } },
  { name: "Rclone", url: "https://github.com/rclone/rclone", license: "MIT", get relation() { return tr("PikPak 接口实现参考（backend/pikpak）；可选外部程序，不捆绑"); } },
  { name: "WinFsp", url: "https://github.com/winfsp/winfsp", get license() { return tr("GPLv3 + FLOSS 例外"); }, get relation() { return tr("可选外部驱动，用户自行安装"); } },
  { name: "Tauri v2", url: "https://github.com/tauri-apps/tauri", license: "Apache-2.0 / MIT", get relation() { return tr("应用外壳与 IPC"); } },
];

const FRONTEND_DEPS: { name: string; url: string; license: string; why: string }[] = [
  { name: "Preact 10", url: "https://github.com/preactjs/preact", license: "MIT", get why() { return tr("组件渲染，约 4.5 KB gzip"); } },
  { name: "@tauri-apps/api 2", url: "https://github.com/tauri-apps/tauri", license: "Apache-2.0 / MIT", get why() { return tr("invoke、事件与窗口控制，按需加载"); } },
  { name: "Material Symbols（Rounded）", url: "https://github.com/google/material-design-icons", license: "Apache-2.0", get why() { return tr("约 120 个图标的 SVG 路径（构建前提取，不打包整套）"); } },
  { name: "material-color-utilities", url: "https://github.com/material-foundation/material-color-utilities", license: "Apache-2.0", get why() { return tr("仅构建期：从种子色生成 Material 3 配色"); } },
];

function LinkCell({ name, url }: { name: string; url: string }) {
  return (
    <a href={url} onClick={(e) => { e.preventDefault(); void openOfficial(url); }}>{name}</a>
  );
}

export function AboutContent() {
  const version = useStore(app, (s) => s.app?.version);
  return (
    <div class="settings-section about">
      <div class="about-hero">
        <LogoMark size={56} />
        <div>
          <h2 class="about-name">NekoPak</h2>
          <p class="about-sub">A lightweight, Rust-powered PikPak desktop and Android client</p>
          <p class="dim">{tr("版本")}{version ?? "—"}，Pre-alpha</p>
        </div>
      </div>
      <div class="about-statement" role="note">
        <p>{tr("NekoPak 是社区开发的 PikPak 客户端，与 PikPak 官方无关。")}</p>
      </div>

      <AppUpdates />
      <h3>{tr("来源与致谢")}</h3>
      <p class="dim">{tr("完整清单与核查记录见仓库中的 docs/04_SOURCES_LICENSES.md、docs/UI_SOURCES_AND_DEPENDENCIES.md 与 THIRD_PARTY_NOTICES.md。各项目代码及素材归各自权利人所有，列出不代表其作者为 NekoPak 背书。")}</p>
      <div class="table-wrap">
        <table class="data-table">
          <thead><tr><th scope="col">{tr("项目")}</th><th scope="col">{tr("许可证")}</th><th scope="col">{tr("与 NekoPak 的关系")}</th></tr></thead>
          <tbody>
            {SOURCES.map((x) => (
              <tr><td><LinkCell name={x.name} url={x.url} /></td><td>{x.license}</td><td>{x.relation}</td></tr>
            ))}
          </tbody>
        </table>
      </div>

      <h3>{tr("界面使用的第三方组件")}</h3>
      <div class="table-wrap">
        <table class="data-table">
          <thead><tr><th scope="col">{tr("组件")}</th><th scope="col">{tr("许可证")}</th><th scope="col">{tr("用途")}</th></tr></thead>
          <tbody>
            {FRONTEND_DEPS.map((d) => <tr><td><LinkCell name={d.name} url={d.url} /></td><td>{d.license}</td><td>{d.why}</td></tr>)}
          </tbody>
        </table>
      </div>
      <p class="dim">{tr("猫箱标志为 NekoPak 原创；界面使用系统字体，未嵌入字体文件。")}</p>

      <div class="row-gap pad-top">
        <Btn tone="tonal" icon="external" label={tr("源码仓库与反馈")} onClick={() => void openOfficial("https://github.com/NekoPak/NekoPak")} />
        <Btn tone="ghost" icon="external" label={tr("PikPak 官网")} onClick={() => void openOfficial("https://mypikpak.com/")} />
      </div>
    </div>
  );
}
