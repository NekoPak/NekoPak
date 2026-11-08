/** Local and shared handles are not IDs in the active account's drive. */
export function entrySource(id: string): "local" | "shared" | "drive" {
  if (id.startsWith("local:")) return "local";
  if (id.startsWith("shared:") || id.startsWith("share:")) return "shared";
  return "drive";
}
