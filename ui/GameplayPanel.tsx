import { errorText } from "./lib/format";
import type { Gameplay } from "./api/models/GameplayPanel";
import { OreToggle } from "./OreToggle";
import { useEffect, useState } from "react";
import { Check, LoaderCircle } from "lucide-react";
import { api, type Server } from "./api";

export function GameplayPanel({ server }: { server: Server }) {
  const [form, setForm] = useState<Gameplay | null>(null);
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const stopped = ["offline", "error"].includes(server.state);
  async function load() {
    setError("");
    try {
      setForm(await api("gameplay", { id: server.id }));
    } catch (e) {
      setError(errorText(e));
    }
  }
  useEffect(() => {
    void load();
  }, [server.id]);
  function change(patch: Partial<Gameplay>) {
    setForm((old) => (old ? { ...old, ...patch } : old));
    setSaved(false);
  }
  return (
    <section className="settings-panel gameplay-panel">
      {!stopped && (
        <div className="form-info">
          Para o servidor antes de alterar o jogo.
        </div>
      )}
      {!form && !error && (
        <p>
          <LoaderCircle size={18} className="spin" /> A ler as definições…
        </p>
      )}
      {form && (
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            setSaving(true);
            setError("");
            setSaved(false);
            try {
              setForm(
                await api("save_gameplay", {
                  id: server.id,
                  settings: form,
                }),
              );
              setSaved(true);
            } catch (e) {
              setError(errorText(e));
            } finally {
              setSaving(false);
            }
          }}
        >
          <fieldset disabled={!stopped || saving}>
            <div className="form-row">
              <label>
                Dificuldade
                <select
                  value={form.difficulty}
                  onChange={(e) => change({ difficulty: e.target.value })}
                >
                  <option value="peaceful">Pacífico</option>
                  <option value="easy">Fácil</option>
                  <option value="normal">Normal</option>
                  <option value="hard">Difícil</option>
                </select>
              </label>
              <label>
                Modo de jogo inicial
                <select
                  value={form.gamemode}
                  onChange={(e) => change({ gamemode: e.target.value })}
                >
                  <option value="survival">Sobrevivência</option>
                  <option value="creative">Criativo</option>
                  <option value="adventure">Aventura</option>
                  <option value="spectator">Espetador</option>
                </select>
              </label>
            </div>
            <p className="gameplay-mode-hint">
              O modo inicial aplica-se apenas a novos jogadores.
            </p>
            <label>
              Máximo de jogadores
              <input
                type="number"
                min={1}
                max={1000}
                required
                value={form.maxPlayers}
                onChange={(e) => change({ maxPlayers: Number(e.target.value) })}
              />
            </label>
            <div className="form-row">
              <label>
                Distância de visão (chunks)
                <input
                  type="number"
                  min={2}
                  max={32}
                  required
                  value={form.viewDistance}
                  onChange={(e) =>
                    change({ viewDistance: Number(e.target.value) })
                  }
                />
              </label>
              <label>
                Distância de simulação (chunks)
                <input
                  type="number"
                  min={2}
                  max={32}
                  required
                  value={form.simulationDistance}
                  onChange={(e) =>
                    change({ simulationDistance: Number(e.target.value) })
                  }
                />
              </label>
            </div>
            <p className="panel-footnote">
              Distâncias menores reduzem o trabalho do servidor. O limite de
              jogadores não reserva RAM adicional.
            </p>
            <label className="eula-check">
              <OreToggle
                checked={form.pvp}
                onChange={(e) => change({ pvp: e.target.checked })}
              />
              <span>Permitir combate entre jogadores (PvP)</span>
            </label>
          </fieldset>
          <button className="button primary" disabled={!stopped || saving}>
            {saving ? (
              <LoaderCircle size={16} className="spin" />
            ) : (
              <Check size={16} />
            )}{" "}
            Guardar jogo
          </button>
          {saved && (
            <p className="gameplay-success" role="status">
              Guardado. As opções serão aplicadas no próximo arranque.
            </p>
          )}
        </form>
      )}
      {error && (
        <div role="alert">
          <p className="inline-error">{error}</p>
          {!form && (
            <button className="button secondary" onClick={() => void load()}>
              Tentar novamente
            </button>
          )}
        </div>
      )}
    </section>
  );
}
