import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Server } from "../api";
import { errorText } from "../lib/format";

/** A response from a previous library must never replace the current library. */
export function useServerLibrary(onError: (message: string) => void) {
  const [servers, setServers] = useState<Server[]>([]);
  const [connected, setConnected] = useState(false);
  const [loading, setLoading] = useState(true);
  const epoch = useRef(0);
  const mounted = useRef(true);
  const errors = useRef(onError);
  errors.current = onError;
  const pending = useRef<{ epoch: number; promise: Promise<void> } | null>(
    null,
  );
  const refresh = useCallback(() => {
    const revision = epoch.current;
    if (pending.current?.epoch === revision) return pending.current.promise;
    const promise = (async () => {
      try {
        const next = await api("list");
        if (!mounted.current || epoch.current !== revision) return;
        setServers(next);
        setConnected(true);
      } catch (error) {
        if (!mounted.current || epoch.current !== revision) return;
        setConnected(false);
        errors.current(errorText(error));
      } finally {
        if (mounted.current && epoch.current === revision) setLoading(false);
        if (pending.current?.epoch === revision) pending.current = null;
      }
    })();
    pending.current = { epoch: revision, promise };
    return promise;
  }, []);
  const reset = useCallback(() => {
    epoch.current += 1;
    setServers([]);
    setLoading(true);
    setConnected(false);
  }, []);
  useEffect(() => {
    mounted.current = true;
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1200);
    return () => {
      mounted.current = false;
      epoch.current += 1;
      window.clearInterval(timer);
    };
  }, [refresh]);
  return { servers, connected, loading, refresh, reset };
}
