import { api } from "./api";

/** Never allow an upstream credential URL to become an image/video src. */
export function safeMediaSessionUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === "http:" && url.hostname === "127.0.0.1" && !!url.port && !url.username && !url.password && !url.hash && url.pathname.length > 10;
  } catch { return false; }
}

let active = 0;
const waiting: (() => void)[] = [];
const MAX_IMAGE_REQUESTS = 3;

function acquire(signal: AbortSignal): Promise<() => void> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) { reject(signal.reason); return; }
    const abort = () => {
      const index = waiting.indexOf(start);
      if (index >= 0) waiting.splice(index, 1);
      reject(signal.reason);
    };
    const start = () => {
      signal.removeEventListener("abort", abort);
      if (signal.aborted) { reject(signal.reason); return; }
      active++;
      resolve(() => { active--; waiting.shift()?.(); });
    };
    if (active < MAX_IMAGE_REQUESTS) start();
    else { waiting.push(start); signal.addEventListener("abort", abort, { once: true }); }
  });
}

/** Keep only three short-lived loopback sessions, then display revocable blobs. */
export async function loadGalleryImageUrl(id: string, signal: AbortSignal): Promise<string> {
  const release = await acquire(signal);
  let sessionId: string | undefined;
  let objectUrl: string | undefined;
  try {
    const session = await api.mediaSessionOpen(id);
    sessionId = session.session_id;
    signal.throwIfAborted();
    if (!safeMediaSessionUrl(session.url)) throw new Error("Invalid local media session");
    const response = await fetch(session.url, { signal, credentials: "omit", redirect: "error" });
    if (!response.ok) throw new Error(`Image stream failed (${response.status})`);
    const image = await response.blob();
    signal.throwIfAborted();
    objectUrl = URL.createObjectURL(image);
    return objectUrl;
  } finally {
    try { if (sessionId) await api.mediaSessionClose(sessionId); }
    catch (error) { if (objectUrl) URL.revokeObjectURL(objectUrl); throw error; }
    finally { release(); }
  }
}
