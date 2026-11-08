import { tr } from "../lib/i18n";
import { useStore } from "../lib/store";
import { closeTab, cycleTab, MAX_TABS, openTab, selectTab, tabTitle, workspaceTabs } from "../state/route";
import { Icon } from "./Icon";

export function WorkspaceTabs() {
  const s = useStore(workspaceTabs, x => x);
  return <div class="workspace-tabs" role="tablist" aria-label={tr("工作区标签页")}>
    <div class="workspace-tab-scroll">
      {s.items.map(tab => <div class={`workspace-tab${tab.id === s.activeId ? " active" : ""}`} key={tab.id}>
        <button type="button" role="tab" id={`workspace-tab-${tab.id}`} aria-selected={tab.id === s.activeId}
          tabIndex={tab.id === s.activeId ? 0 : -1} aria-controls="workspace-panel" title={tabTitle(tab)}
          onClick={() => selectTab(tab.id)} onKeyDown={e => {
            if (e.key === "ArrowRight" || e.key === "ArrowLeft") { e.preventDefault(); cycleTab(e.key === "ArrowRight" ? 1 : -1); requestAnimationFrame(() => document.getElementById(`workspace-tab-${workspaceTabs.get().activeId}`)?.focus()); }
            if (e.key === "Delete") { e.preventDefault(); closeTab(tab.id); }
          }}><span dir="auto">{tabTitle(tab)}</span></button>
        <button type="button" class="workspace-tab-close" aria-label={`${tr("关闭标签页")}: ${tabTitle(tab)}`} title={tr("关闭标签页（Ctrl+W）")}
          onClick={() => closeTab(tab.id)}><Icon name="x" size={14}/></button>
      </div>)}
    </div>
    <button type="button" class="workspace-tab-add" aria-label={tr("新建标签页")} title={tr("新建标签页（Ctrl+T）")}
      disabled={s.items.length >= MAX_TABS} onClick={() => openTab()}><Icon name="plus" size={18}/></button>
  </div>;
}
