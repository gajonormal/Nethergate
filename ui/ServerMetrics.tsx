import { usePolling } from "./hooks/usePolling";
import type { Metrics } from "./api/models/ServerMetrics";
import { useState } from "react";
import { Activity, MemoryStick } from "lucide-react";
import { api, type Server } from "./api";

export function ServerMetrics({ server }: { server: Server }) {
  const [metrics, setMetrics] = useState<Metrics | null>(null);
  const [error, setError] = useState("");
  usePolling(() => api("metrics", { id: server.id }), {
    interval: 3000,
    dependencies: [server.id],
    onReset: () => {
      setMetrics(null);
      setError("");
    },
    onData: (value) => {
      setMetrics(value);
      setError("");
    },
    onError: () => setError("Métricas temporariamente indisponíveis."),
  });
  if (!server.pid) return null;
  const memory =
    metrics?.memoryBytes != null
      ? (metrics.memoryBytes / 1024 / 1024).toLocaleString("pt-PT", {
          maximumFractionDigits: 0,
        }) + " MB"
      : "—";
  return (
    <section className="server-metrics" aria-label="Recursos do processo Java">
      <div>
        <Activity size={18} />
        <span>
          CPU do Java
          <strong>
            {metrics?.cpuPercent != null
              ? metrics.cpuPercent.toLocaleString("pt-PT", {
                  maximumFractionDigits: 1,
                }) + "%"
              : "A medir…"}
          </strong>
        </span>
      </div>
      <div>
        <MemoryStick size={18} />
        <span>
          RAM em uso<strong>{memory}</strong>
        </span>
      </div>
      <p>
        {error ||
          metrics?.error ||
          (metrics && !metrics.supported
            ? "Métricas disponíveis apenas no Windows."
            : "CPU relativa à capacidade total do processador. RAM residente do Java; o jogo e a aplicação usam memória adicional.")}
      </p>
    </section>
  );
}
