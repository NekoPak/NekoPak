import { afterEach, expect, it, vi } from "vitest";
import { api } from "../../src/lib/api";
import { loadGalleryImageUrl } from "../../src/lib/gallery-image";

afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it("loads twelve visible images while keeping at most three media sessions open", async () => {
  let live = 0, highest = 0;
  const open = vi.spyOn(api, "mediaSessionOpen").mockImplementation(async id => {
    live++; highest = Math.max(highest, live);
    return { session_id: id, url: `http://127.0.0.1:49200/media/${id}/image`, mime_type: "image/png" };
  });
  const close = vi.spyOn(api, "mediaSessionClose").mockImplementation(async () => { live--; });
  vi.stubGlobal("fetch", vi.fn(async () => {
    await new Promise(resolve => setTimeout(resolve, 5));
    return new Response(new Blob(["image"], { type: "image/png" }));
  }));
  const images = await Promise.all(Array.from({ length: 12 }, (_, n) => loadGalleryImageUrl(`image-${n}`, new AbortController().signal)));
  expect(images).toHaveLength(12);
  expect(images.every(url => url.startsWith("blob:"))).toBe(true);
  expect(highest).toBe(3); expect(live).toBe(0);
  expect(open).toHaveBeenCalledTimes(12); expect(close).toHaveBeenCalledTimes(12);
  images.forEach(url => URL.revokeObjectURL(url));
});

it("closes failed streams and removes images scrolled out before their queued request starts", async () => {
  let release!: () => void;
  const barrier = new Promise<void>(resolve => { release = resolve; });
  const open = vi.spyOn(api, "mediaSessionOpen").mockImplementation(async id => ({ session_id: id, url: `http://127.0.0.1:49200/media/${id}/image`, mime_type: "image/png" }));
  const close = vi.spyOn(api, "mediaSessionClose").mockResolvedValue();
  vi.stubGlobal("fetch", vi.fn(async () => { await barrier; return new Response("failed", { status: 500 }); }));
  const first = Array.from({ length: 3 }, (_, n) => loadGalleryImageUrl(`first-${n}`, new AbortController().signal).catch(() => null));
  const queued = new AbortController();
  const pending = loadGalleryImageUrl("out-of-view", queued.signal);
  queued.abort();
  await expect(pending).rejects.toBeTruthy();
  release(); await Promise.all(first);
  expect(open).toHaveBeenCalledTimes(3);
  expect(close).toHaveBeenCalledTimes(3);
});
