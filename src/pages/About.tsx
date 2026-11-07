import { useStore } from "../lib/store";
import { app } from "../state/app";
import { LogoMark } from "../components/Brand";
import { Btn, openOfficial } from "../components/ui";

/**
 * Mirrors docs/04_SOURCES_LICENSES.md. "关系" reflects what the UI code
 * actually does today: no upstream source code is copied into src/.
 */
const SOURCES: { name: string; url: string; license: string; relation: string }[] = [
  { name: "Piko（NihilDigit）", url: "https://github.com/NihilDigit/piko", license: "AGPL-3.0-or-later", relation: "界面与交互设计参考（圆角岛外框、侧栏、任务页布局），未复制代码或素材" },
  { name: "pikpaktui（Bengerthelorf）", url: "https://github.com/Bengerthelorf/pikpaktui", license: "Apache-2.0", relation: "Rust 核心候选来源，待审计" },
  { name: "pikpakcli（52funny）", url: "https://github.com/52funny/pikpakcli", license: "MIT", relation: "接口与 CLI 行为研究" },
  { name: "PikPak Enhancement Master（digbug82）", url: "https://github.com/digbug82/PikPak_Enhancement_Master", license: "AGPL-3.0", relation: "便利功能参考，未复制代码" },
  { name: "pikpak-kotlin（NihilDigit）", url: "https://github.com/NihilDigit/pikpak-kotlin", license: "MIT", relation: "行为与边界条件参考" },
  { name: "MediaMP（open-ani）", url: "https://github.com/open-ani/mediamp", license: "Apache-2.0", relation: "播放交互设计参考" },
  { name: "Rclone", url: "https://github.com/rclone/rclone", license: "MIT", relation: "可选外部程序，不捆绑" },
  { name: "WinFsp", url: "https://github.com/winfsp/winfsp", license: "GPLv3 + FLOSS 例外", relation: "可选外部驱动，用户自行安装" },
  { name: "Tauri v2", url: "https://github.com/tauri-apps/tauri", license: "Apache-2.0 / MIT", relation: "应用外壳与 IPC" },
  { name: "PikPak 官方 CLI", url: "https://mypikpak.com/en-US/cli", license: "未确认可再分发", relation: "支持的账号连接方式；外部程序不随包分发" },
];

const FRONTEND_DEPS: { name: string; url: string; license: string; why: string }[] = [
  { name: "Preact 10", url: "https://github.com/preactjs/preact", license: "MIT", why: "组件渲染，约 4.5 KB gzip" },
  { name: "@tauri-apps/api 2", url: "https://github.com/tauri-apps/tauri", license: "Apache-2.0 / MIT", why: "invoke、事件与窗口控制，按需加载" },
  { name: "Material Symbols（Rounded）", url: "https://github.com/google/material-design-icons", license: "Apache-2.0", why: "约 80 个图标的 SVG 路径（构建前提取，不打包整套）" },
  { name: "material-color-utilities", url: "https://github.com/material-foundation/material-color-utilities", license: "Apache-2.0", why: "仅构建期：从种子色生成 Material 3 配色" },
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
          <p class="about-sub">A lightweight, Rust-powered PikPak desktop client</p>
          <p class="dim">版本 {version ?? "—"}，Pre-alpha</p>
        </div>
      </div>
      <div class="about-statement" role="note">
        <p><strong>NekoPak 是独立开发的社区客户端，不是 PikPak 官方产品，与 PikPak 服务提供者没有隶属关系，也未获其背书。</strong></p>
        <p>NekoPak 不会要求你输入 PikPak 密码，不修改系统 Hosts，不收集遥测数据。社区兼容层使用的是非官方接口，启用前会明确提示。</p>
      </div>

      <h3>来源与致谢</h3>
      <p class="dim">完整清单与核查记录见仓库中的 docs/04_SOURCES_LICENSES.md、docs/UI_SOURCES_AND_DEPENDENCIES.md 与 THIRD_PARTY_NOTICES.md。各项目代码及素材归各自权利人所有，列出不代表其作者为 NekoPak 背书。</p>
      <div class="table-wrap">
        <table class="data-table">
          <thead><tr><th scope="col">项目</th><th scope="col">许可证</th><th scope="col">与 NekoPak 的关系</th></tr></thead>
          <tbody>
            {SOURCES.map((x) => (
              <tr><td><LinkCell name={x.name} url={x.url} /></td><td>{x.license}</td><td>{x.relation}</td></tr>
            ))}
          </tbody>
        </table>
      </div>

      <h3>界面使用的第三方组件</h3>
      <div class="table-wrap">
        <table class="data-table">
          <thead><tr><th scope="col">组件</th><th scope="col">许可证</th><th scope="col">用途</th></tr></thead>
          <tbody>
            {FRONTEND_DEPS.map((d) => <tr><td><LinkCell name={d.name} url={d.url} /></td><td>{d.license}</td><td>{d.why}</td></tr>)}
          </tbody>
        </table>
      </div>
      <p class="dim">猫箱标志为 NekoPak 原创；界面使用系统字体，未嵌入字体文件。</p>

      <div class="row-gap pad-top">
        <Btn tone="tonal" icon="external" label="源码仓库与反馈" disabled reason="仓库地址在 Gate 0 确定后填写" />
        <Btn tone="ghost" icon="external" label="PikPak 官网" onClick={() => void openOfficial("https://mypikpak.com/")} />
      </div>
    </div>
  );
}
