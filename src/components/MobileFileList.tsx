import { useEffect, useRef, useState } from "preact/hooks";
import type { RemoteEntry } from "../lib/contract";
import { tr, tx } from "../lib/i18n";
import { formatBytes, formatDateTime } from "../lib/format";
import type { FileTableProps } from "./FileTable";
import { KindTile } from "./FileTable";
import { Icon } from "./Icon";
import { useVirtualRows } from "../lib/virtual";
import { fileGate } from "../state/app";
import { downloadWithOptions } from "../pages/download";
import { dialogs, menu } from "../state/ui";

export function MobileFileList(p: FileTableProps) {
  const scroller = useRef<HTMLDivElement>(null);
  const press = useRef<ReturnType<typeof setTimeout> | null>(null);
  const longPress = useRef(false);
  const pressPoint=useRef({x:0,y:0});
  const [selectMode, setSelectMode] = useState(false);
  const choosing = selectMode || p.selection.ids.size > 0;
  const grid=p.mode==="grid", columns=grid?2:1, rowHeight=grid?152:72;
  const { range } = useVirtualRows(scroller, Math.ceil(p.items.length/columns), rowHeight, 7);
  useEffect(() => { if (p.hasMore && !p.loadingMore && range.end*columns >= p.items.length - 15) p.onLoadMore?.(); }, [range.end,columns,p.hasMore,p.loadingMore,p.items.length]);
  useEffect(() => () => { if (press.current) clearTimeout(press.current); }, []);
  useEffect(()=>{setSelectMode(false);if(scroller.current)scroller.current.scrollTop=0;},[p.scrollKey,p.id]);
  useEffect(()=>{
    if(!choosing)return;
    const back=(event:Event)=>{if(menu.get().open||dialogs.get().stack.length)return;event.preventDefault();event.stopImmediatePropagation();setSelectMode(false);p.setSelection({ids:new Set(),anchor:0,focus:0});};
    window.addEventListener("nekopak:back",back,{capture:true});
    return()=>window.removeEventListener("nekopak:back",back,{capture:true});
  },[choosing,p.setSelection]);
  const toggle = (entry: RemoteEntry, i: number) => p.setSelection(s => {
    const ids = new Set(s.ids); if (ids.has(entry.id)) ids.delete(entry.id); else ids.add(entry.id);
    return { ids, anchor:i, focus:i };
  });
  const stopPress = () => { if (press.current) clearTimeout(press.current); press.current = null; };
  return <div class="mobile-file-browser">
    <div class="mobile-list-summary"><span>{tx("{0} 项", p.items.length)}</span><button type="button" onClick={() => { setSelectMode(!choosing); if (choosing) p.setSelection({ids:new Set(),anchor:0,focus:0}); }}>{tr(choosing ? "取消" : "选择")}</button></div>
    <div ref={scroller} class={`mobile-file-scroll${grid?" mobile-file-grid":""}`} role="list" aria-label={p.label} id={p.id}>
      <div style={{height:`${range.total}px`,position:"relative"}}><div style={{position:"absolute",top:0,left:0,right:0,transform:`translateY(${range.padTop}px)`,...(grid?{display:"grid",gridTemplateColumns:"repeat(2,minmax(0,1fr))"}:{})}}>
        {p.items.slice(range.start*columns,range.end*columns).map((entry,j) => {
          const i=range.start*columns+j, selected=p.selection.ids.has(entry.id);
          return <div key={entry.id} class={`mobile-file-row${selected?" selected":""}`} data-id={entry.id} role="listitem" style={{height:rowHeight}}>
            <button type="button" class="mobile-file-open" aria-label={choosing ? tx("选择 {0}",entry.name) : tx("打开 {0}",entry.name)} aria-pressed={choosing?selected:undefined}
              onPointerDown={event => { longPress.current=false; if (event.button!==0) return; pressPoint.current={x:event.clientX,y:event.clientY};stopPress(); press.current=setTimeout(()=>{longPress.current=true;setSelectMode(true);if(!selected)toggle(entry,i);},450); }}
              onPointerUp={stopPress} onPointerCancel={stopPress} onPointerMove={event=>{if(Math.abs(event.clientX-pressPoint.current.x)+Math.abs(event.clientY-pressPoint.current.y)>12)stopPress();}}
              onClick={()=>{if(longPress.current){longPress.current=false;return;}if(choosing)toggle(entry,i);else p.onOpen(entry);}}
              onContextMenu={event=>{event.preventDefault();setSelectMode(true);if(!selected)toggle(entry,i);}}>
              {choosing ? <span class="mobile-file-checkbox"><Icon name={selected?"checkbox":"checkboxOff"} filled={selected} size={24}/></span> : <KindTile entry={entry}/>}
              <span class="mobile-file-copy"><strong dir="auto">{entry.name}</strong><small>{entry.kind==="folder" ? tr("文件夹") : formatBytes(entry.size_bytes)}{entry.modified_at ? ` · ${formatDateTime(entry.modified_at)}` : ""}</small></span>
            </button>
            {!choosing ? <button type="button" class="mobile-icon-button" aria-label={tx("{0}的更多操作",entry.name)} onClick={()=>p.onMenu([entry],0,0)}><Icon name="more"/></button> : null}
          </div>;
        })}
      </div></div>{p.footer}
    </div>
    {choosing ? <div class="mobile-selection-actions"><span>{tx("已选 {0} 项",p.selection.ids.size)}</span><button type="button" onClick={()=>p.setSelection(s=>({...s,ids:new Set(p.items.map(e=>e.id))}))}>{tr("全选")}</button>{!p.id.startsWith("trash")?<button type="button" disabled={!p.selection.ids.size||!fileGate("download",[...p.selection.ids]).ok} onClick={()=>void downloadWithOptions(p.items.filter(e=>p.selection.ids.has(e.id)))}><Icon name="download"/>{tr("下载")}</button>:null}<button type="button" disabled={!p.selection.ids.size} onClick={()=>p.onMenu(p.items.filter(e=>p.selection.ids.has(e.id)),0,0)}><Icon name="more"/>{tr("操作")}</button></div> : null}
  </div>;
}
