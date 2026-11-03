import { useEffect, useRef, type DependencyList } from "react";

interface PollingOptions<T> {
  interval: number;
  dependencies: DependencyList;
  onData: (value: T, initial: boolean) => void;
  onError: (error: unknown) => void;
  onReset?: () => void;
}
/** One request per scope at a time; cleanup rejects late results, not server work. */
export function usePolling<T>(
  read: () => Promise<T>,
  options: PollingOptions<T>,
) {
  const latest = useRef({ read, ...options });
  latest.current = { read, ...options };
  useEffect(() => {
    let closed = false;
    let pending = false;
    let initial = true;
    latest.current.onReset?.();
    async function poll() {
      if (pending || closed) return;
      pending = true;
      const first = initial;
      initial = false;
      try {
        const value = await latest.current.read();
        if (!closed) latest.current.onData(value, first);
      } catch (error) {
        if (!closed) latest.current.onError(error);
      } finally {
        pending = false;
      }
    }
    void poll();
    const timer = window.setInterval(() => void poll(), options.interval);
    return () => {
      closed = true;
      window.clearInterval(timer);
    };
  }, [options.interval, ...options.dependencies]);
}
