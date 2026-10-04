/**
 * Replaces properties of `target` (a jsdom gap such as `URL.createObjectURL`
 * or `window.matchMedia`) and returns a function that puts the originals
 * back, or removes the properties that did not exist before.
 */
export function stubProperties<T extends object>(target: T, values: Partial<T>): () => void {
  const saved = Object.keys(values).map(
    (key) => [key, Object.getOwnPropertyDescriptor(target, key)] as const,
  );
  for (const [key, value] of Object.entries(values)) {
    Object.defineProperty(target, key, { value, configurable: true, writable: true });
  }
  return () => {
    for (const [key, descriptor] of saved) {
      if (descriptor) Object.defineProperty(target, key, descriptor);
      else Reflect.deleteProperty(target, key);
    }
  };
}
