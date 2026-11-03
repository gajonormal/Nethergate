import { errorText } from "./lib/format";
import type { Plan } from "./api/models/MigrationPanel";
import { OreToggle } from "./OreToggle";
import { useEffect, useState } from "react";
import { api, type Server } from "./api";

export function MigrationPanel({
  server,
  onNotice,
}: {
  server: Server;
  onNotice: (text: string) => void;
}) {
  const [versions, setVersions] = useState<string[]>([]);
  const [expanded, setExpanded] = useState(false);
  const [target, setTarget] = useState("");
  const [name, setName] = useState(`${server.name} — atualizado`);
  const [port, setPort] = useState(server.port + 1);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [ack, setAck] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    setPlan(null);
    setTarget("");
    setName(`${server.name} — atualizado`);
    setPort(Math.min(server.port + 1, 65535));
  }, [server.id, server.version]);
  async function load() {
    setExpanded(true);
    setPending(true);
    setError("");
    try {
      setVersions(await api("migration_versions", { id: server.id }));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(false);
    }
  }
  async function preview() {
    setPending(true);
    setError("");
    setPlan(null);
    setAck(false);
    try {
      setPlan(await api("migration_preview", { id: server.id, target }));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(false);
    }
  }
  async function migrate() {
    if (!plan) return;
    setPending(true);
    setError("");
    try {
      await api("migrate_server", {
        id: server.id,
        input: {
          target: plan.target,
          name,
          port,
          fingerprint: plan.fingerprint,
          acknowledged: ack,
        },
      });
      setPlan(null);
      onNotice(
        `Cópia ${name} preparada para Minecraft ${target}. Abre a biblioteca para a iniciar e testar; o servidor original foi preservado.`,
      );
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(false);
    }
  }
  const stopped = ["offline", "error"].includes(server.state) && !server.pid;
  return (
    <article className="connection-card migration-panel">
      <h3>Atualizar a versão do Minecraft</h3>
      <p>
        Cria uma cópia numa versão superior, com backup e verificação de
        mods/plugins. Preserva o original. Não permite downgrade.
      </p>
      {!expanded ? (
        <button
          className="button secondary"
          disabled={!stopped}
          onClick={() => void load()}
        >
          Escolher nova versão…
        </button>
      ) : (
        <fieldset disabled={pending || !stopped}>
          <label>
            Versão de destino
            <select
              value={target}
              onChange={(e) => {
                setTarget(e.target.value);
                setPlan(null);
              }}
            >
              <option value="">Escolher…</option>
              {versions.map((v) => (
                <option key={v} value={v}>
                  {v}
                </option>
              ))}
            </select>
          </label>
          <button
            className="button secondary"
            disabled={!target}
            onClick={() => void preview()}
          >
            Verificar compatibilidade
          </button>
          {plan && (
            <>
              <p>
                {plan.from} → {plan.target} · Java mínimo {plan.javaMajor}
              </p>
              <ul>
                {plan.addons.items.map((item) => (
                  <li key={item.projectId}>
                    {item.title} · {item.versionNumber}
                  </li>
                ))}
              </ul>
              {plan.warnings.map((w, i) => (
                <p key={i}>{w}</p>
              ))}
              <div className="form-row">
                <label>
                  Nome da cópia
                  <input
                    maxLength={60}
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                  />
                </label>
                <label>
                  Porta da cópia
                  <input
                    type="number"
                    min={1}
                    max={65535}
                    value={port}
                    onChange={(e) => setPort(Number(e.target.value))}
                  />
                </label>
              </div>
              <label className="eula-check">
                <OreToggle
                  checked={ack}
                  onChange={(e) => setAck(e.target.checked)}
                />
                <span>
                  Vou iniciar e testar a nova cópia antes de a partilhar. O
                  mundo será convertido pelo Minecraft no primeiro arranque;
                  configurações privadas e whitelist precisam de revisão.
                </span>
              </label>
              <button
                className="button primary"
                disabled={!ack || !name.trim()}
                onClick={() => void migrate()}
              >
                Preparar cópia atualizada
              </button>
            </>
          )}
          {!versions.length && (
            <p>Não foram encontradas versões superiores disponíveis.</p>
          )}
        </fieldset>
      )}
      {pending && (
        <p role="status">
          A verificar ou preparar a cópia. Aguarda a conclusão.
        </p>
      )}
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </article>
  );
}
