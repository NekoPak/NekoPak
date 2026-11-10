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
