import { usePolling } from "./hooks/usePolling";
import type { Energy } from "./api/models/EnergyStatus";
import { useState } from "react";
import {
  Battery,
  Moon,
  PlugZap,
  ShieldCheck,
  TriangleAlert,
} from "lucide-react";
import { api } from "./api";

export function EnergyStatus() {
  const [energy, setEnergy] = useState<Energy | null>(null);
  const [error, setError] = useState("");
  usePolling(() => api("energy"), {
    interval: 3000,
    dependencies: [],
    onData: (value) => {
      setEnergy(value);
      setError("");
    },
    onError: () => {
      setEnergy(null);
      setError("Não foi possível confirmar o estado de energia.");
    },
  });
  if (!energy)
    return error ? (
      <aside className="energy-status warning" role="status">
        <TriangleAlert size={20} />
        <span>{error}</span>
      </aside>
    ) : null;
  const active = energy.activeServers > 0;
  const onBattery = energy.supply?.source === "battery";
  const unprotected = active && energy.protectedServers < energy.activeServers;
  const warning = active && (onBattery || unprotected || !!energy.error);
  if (!active && !energy.error) return null;
  const Icon = warning ? TriangleAlert : active ? ShieldCheck : Moon;
  const supply =
    energy.supply?.source === "ac"
      ? "Ligado à corrente"
      : onBattery
        ? "A usar bateria"
        : energy.supply?.hasBattery === false
          ? "Sem bateria detetada"
          : "Alimentação desconhecida";
  return (
    <aside
      className={`energy-status ${warning ? "warning" : active ? "protected" : "idle"}`}
      aria-live="polite"
    >
      <Icon size={21} />
      <div className="energy-description">
        <strong>
          {unprotected
            ? "Proteção contra suspensão indisponível"
            : active
              ? `Suspensão automática impedida · ${energy.protectedServers} servidor(es)`
              : energy.supported
                ? "Energia · comportamento normal do Windows"
                : "Proteção de energia não disponível nesta plataforma"}
        </strong>
        <p>
          {active
            ? unprotected
              ? "O Windows pode suspender automaticamente. Mantém o PC acordado enquanto o servidor corre."
              : onBattery
                ? "O servidor está a consumir bateria. Liga o carregador para continuar a jogar."
                : "A proteção mantém-se até o servidor terminar de guardar. O ecrã pode apagar-se."
            : energy.supported
              ? "A proteção ativa-se automaticamente quando iniciares um servidor."
              : "A proteção contra suspensão está implementada apenas para Windows."}
        </p>
        {active && (
          <small>
            {unprotected
              ? "Consulta a consola para saber por que motivo o pedido falhou. "
              : ""}
            Fechar a tampa ou suspender manualmente pode interromper o servidor.
          </small>
        )}
        {energy.error && <small>{energy.error}</small>}
      </div>
      <span className="energy-supply">
        {onBattery ? <Battery size={17} /> : <PlugZap size={17} />}
        {supply}
        {energy.supply?.batteryPercent != null &&
          ` · ${energy.supply.batteryPercent}%`}
      </span>
    </aside>
  );
}
