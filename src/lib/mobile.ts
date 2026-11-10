import { useEffect, useState } from "preact/hooks";
import { app, isMobileHost } from "../state/app";
import { useStore } from "./store";

/** Presentation only. Host permissions are still supplied exclusively by Rust. */
export function mobileLayout(): boolean {
  return isMobileHost() || (typeof matchMedia !== "undefined" && matchMedia("(max-width: 600px)").matches);
}

export function useMobileLayout(): boolean {
  const native = useStore(app, isMobileHost);
  const [narrow, setNarrow] = useState(() => typeof matchMedia !== "undefined" && matchMedia("(max-width: 600px)").matches);
  useEffect(() => {
    if (typeof matchMedia === "undefined") return;
    const query = matchMedia("(max-width: 600px)");
    const change = () => setNarrow(query.matches);
    query.addEventListener("change", change);
    return () => query.removeEventListener("change", change);
  }, []);
  return native || narrow;
}

declare global { interface Window { __NEKOPAK_KEYBOARD_VISIBLE__?: boolean } }

/** Android supplies real IME visibility; narrow browser previews use viewport changes. */
export function useKeyboardVisible(): boolean {
  const native = useStore(app, isMobileHost);
  const [visible, setVisible] = useState(() => window.__NEKOPAK_KEYBOARD_VISIBLE__ === true);
  useEffect(() => {
    let fullHeight = window.visualViewport?.height ?? innerHeight;
    const fromNative = (event: Event) => {
      const value = (event as CustomEvent<{visible:boolean}>).detail?.visible;
      if (typeof value === "boolean") setVisible(value);
    };
    const fromViewport = () => {
      if (native) return;
      const height = window.visualViewport?.height ?? innerHeight;
      const editing = document.activeElement?.matches("input:not([type=checkbox]):not([type=radio]),textarea,[contenteditable=true]");
      if (!editing) fullHeight = Math.max(fullHeight, height);
      setVisible(!!editing && fullHeight - height > 100);
    };
    window.addEventListener("nekopak:keyboard", fromNative);
    window.visualViewport?.addEventListener("resize", fromViewport);
    window.addEventListener("resize", fromViewport);
    document.addEventListener("focusout", fromViewport);
    setVisible(window.__NEKOPAK_KEYBOARD_VISIBLE__ === true);
    return () => {
      window.removeEventListener("nekopak:keyboard", fromNative);
      window.visualViewport?.removeEventListener("resize", fromViewport);
      window.removeEventListener("resize", fromViewport);
      document.removeEventListener("focusout", fromViewport);
    };
  }, [native]);
  return visible;
}
