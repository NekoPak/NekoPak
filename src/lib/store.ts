import { useEffect, useRef, useState } from "preact/hooks";

type Listener = () => void;

/** Minimal observable store. Notifications can be batched to one per frame. */
export class Store<T extends object> {
  private state: T;
  private listeners = new Set<Listener>();
  private scheduled = false;

  constructor(initial: T, private readonly batch = false) {
    this.state = initial;
  }

  get(): T {
    return this.state;
  }

  set(patch: Partial<T> | ((s: T) => Partial<T>)): void {
    const p = typeof patch === "function" ? patch(this.state) : patch;
    this.state = { ...this.state, ...p };
    this.notify();
  }

  /** Replace the whole state object (used when T contains Maps mutated in place). */
  replace(next: T): void {
    this.state = next;
    this.notify();
  }

  subscribe(fn: Listener): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  private notify(): void {
    if (!this.batch) {
      for (const l of [...this.listeners]) l();
      return;
    }
    if (this.scheduled) return;
    this.scheduled = true;
    const run = () => {
      this.scheduled = false;
      for (const l of [...this.listeners]) l();
    };
    if (typeof requestAnimationFrame === "function") requestAnimationFrame(run);
    else setTimeout(run, 16);
  }
}

/** Subscribe a component to a selected slice; re-renders only when it changes (Object.is). */
export function useStore<T extends object, S>(store: Store<T>, select: (s: T) => S): S {
  const selRef = useRef(select);
  selRef.current = select;
  const [value, setValue] = useState(() => select(store.get()));
  const valueRef = useRef(value);
  valueRef.current = value;
  useEffect(() => {
    const check = () => {
      const next = selRef.current(store.get());
      if (!Object.is(next, valueRef.current)) {
        valueRef.current = next;
        setValue(() => next);
      }
    };
    check();
    return store.subscribe(check);
  }, [store]);
  return value;
}
