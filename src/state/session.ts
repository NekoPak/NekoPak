/** Account-bound UI lifetime. Appearance preferences are deliberately outside it. */
let generation = 0;
const resets = new Set<() => void>();

export function sessionGeneration(): number { return generation; }
export function onSessionReset(reset: () => void): () => void {
  resets.add(reset);
  return () => resets.delete(reset);
}
export function resetSessionUi(): void {
  generation++;
  for (const reset of resets) reset();
}
