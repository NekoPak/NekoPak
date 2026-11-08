import { tr } from "../lib/i18n";
import type { JSX } from "preact";
import { useEffect, useState } from "preact/hooks";
import { Spinner } from "../components/ui";

/** Load a page component the first time it is shown (keeps start-up small). */
type Comp<P> = (p: P) => JSX.Element | null;

export function lazyPage<P extends object = Record<string, never>>(load: () => Promise<unknown>) {
  let cached: Comp<P> | null = null;
  return function LazyPage(props: P) {
    const [C, setC] = useState(() => cached);
    useEffect(() => {
      if (C) return;
      let alive = true;
      void load().then((mod) => {
        const c = mod as Comp<P>;
        cached = c;
        if (alive) setC(() => c);
      });
      return () => {
        alive = false;
      };
    }, []);
    return C ? <C {...props} /> : <div class="page-loading"><Spinner label={tr("正在打开")} /></div>;
  };
}
