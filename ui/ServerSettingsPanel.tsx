import { errorText } from "./lib/format";
import { useEffect, useState } from "react";
import { Check, LoaderCircle } from "lucide-react";
import { type Server } from "./api";
import { GameplayPanel } from "./GameplayPanel";
import { JavaPanel } from "./JavaPanel";
import { MemoryControl } from "./MemoryControl";
import { Disclosure } from "./Disclosure";
import { active } from "./server-state";

export function ServerSettings({
  server,
  onSave,
}: {
  server: Server;
  onSave: (settings: import("./api/contract").ServerSettings) => Promise<void>;
}) {
  const [form, setForm] = useState({
    name: server.name,
    memoryMb: server.memoryMb,
    port: server.port,
    javaPath: server.javaPath,
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const disabled = active(server) || saving;
  useEffect(() => {
    setForm((previous) => ({ ...previous, javaPath: server.javaPath }));
  }, [server.javaPath]);
  return (
    <div className="server-settings-layout">
      <section className="settings-panel">
        <h2>Definições do servidor</h2>
        <p>As alterações são aplicadas no próximo arranque.</p>
        {active(server) && (
          <div className="form-info">
            Para o servidor antes de alterar as definições.
          </div>
        )}
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            setSaving(true);
            setError("");
            try {
              await onSave(form);
            } catch (e) {
              setError(errorText(e));
            } finally {
              setSaving(false);
            }
          }}
        >
          <fieldset disabled={disabled}>
            <label>
              Nome do servidor
              <input
                maxLength={60}
                required
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
              />
            </label>
            <div className="form-row">
              <MemoryControl
                value={form.memoryMb}
                onChange={(memoryMb) => setForm({ ...form, memoryMb })}
              />
              <label>
                Porta local
                <input
                  type="number"
                  min={1024}
                  max={65535}
                  required
                  value={form.port}
                  onChange={(e) =>
                    setForm({ ...form, port: Number(e.target.value) })
                  }
                />
              </label>
            </div>
            <label>
              Executável Java
              <input
                required
                value={form.javaPath}
                onChange={(e) => setForm({ ...form, javaPath: e.target.value })}
              />
              <small>
                Usa auto para instalar Java, java para o PATH ou o caminho para
                java.exe. Java mínimo: {server.javaMajor}.
              </small>
            </label>
          </fieldset>
          {error && (
            <p className="inline-error" role="alert">
              {error}
            </p>
          )}
          <button className="button primary" disabled={disabled}>
            {saving ? (
              <LoaderCircle size={16} className="spin" />
            ) : (
              <Check size={16} />
            )}
            Guardar alterações
          </button>
        </form>
        <Disclosure
          title="Java e diagnóstico"
          hint={`Java ${server.javaMajor} ou superior`}
        >
          <JavaPanel server={server} />
        </Disclosure>
      </section>
      <Disclosure
        title="Opções de jogo"
        hint="Dificuldade, jogadores e distâncias"
      >
        <GameplayPanel server={server} />
      </Disclosure>
    </div>
  );
}
