import { useEffect, useState } from "preact/hooks";
import { Store, useStore } from "../lib/store";

/** F5 / refresh button broadcast: pages re-fetch when the key changes. */
export const refreshBus = new Store<{ key: number }>({ key: 0 });

export function requestRefresh(): void {
  refreshBus.set((s) => ({ key: s.key + 1 }));
}

export function useRefreshKey(): number {
  return useStore(refreshBus, (s) => s.key);
}

/** Re-render periodically (relative timestamps). */
export function useTicker(ms = 30_000): number {
  const [n, setN] = useState(0);
  useEffect(() => {
    const t = setInterval(() => setN((x) => x + 1), ms);
    return () => clearInterval(t);
  }, [ms]);
  return n;
}
