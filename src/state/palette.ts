import { Store } from "../lib/store";
import { onSessionReset } from "./session";

/** Command palette visibility; the palette UI itself loads on first use. */
export const palette = new Store<{ open: boolean; seed: string }>({ open: false, seed: "" });
onSessionReset(() => palette.set({ open: false, seed: "" }));

export function openPalette(seed = ""): void {
  palette.set({ open: true, seed });
}
