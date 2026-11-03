import { errorText as message } from "./lib/format";
import type { Status } from "./api/models/BackgroundPanel";
import { DialogBackdrop, DialogPresence } from "./DialogPresence";
import { OreToggle } from "./OreToggle";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Monitor, X, LoaderCircle } from "lucide-react";
import { desktop } from "./api";
import type { Server } from "./api";

const stateLabels: Record<string, string> = {
  online: "Em execução",
  starting: "A iniciar",
  stopping: "A guardar",
  restart_waiting: "A aguardar recuperação",
  installing: "A instalar",
  offline: "Parado",
  error: "Com erro",
  backing_up: "A criar backup",
  restoring: "A restaurar",
  managing_plugins: "A preparar plugins",
  exporting_modpack: "A exportar pacote",
};
const active = (s: Server) =>
  s.pid != null || !["offline", "error"].includes(s.state);
export function BackgroundPanel() {
  const [visible, setVisible] = useState(false);
  const [status, setStatus] = useState<Status | null>(null);
  const [pending, setPending] = useState("");
  const [error, setError] = useState("");
  const [confirm, setConfirm] = useState(false);
  const [closeRequested, setCloseRequested] = useState(false);
  const [refreshError, setRefreshError] = useState("");
  const returnButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!desktop) return;
    let closed = false;
    const unlisten = listen<string>("engine-shutdown-error", (event) => {
      if (!closed) {
        setError(event.payload);
        setVisible(true);
        setPending("");
      }
    });
    const unlistenClose = listen("engine-close-requested", () => {
      if (!closed) {
        setVisible(true);
        setCloseRequested(true);
        setConfirm(false);
      }
    });
    return () => {
      closed = true;
      void unlisten.then((remove) => remove()).catch(() => {});
      void unlistenClose.then((remove) => remove()).catch(() => {});
    };
  }, []);
  useEffect(() => {
    if (!desktop || !visible) return;
    let closed = false;
    let running = false;
    async function refresh() {
      if (running) return;
      running = true;
      try {
        const value = await invoke<Status>("background_status");
        if (!closed) {
          setStatus(value);
          setRefreshError("");
        }
      } catch (e) {
        if (!closed) setRefreshError(message(e));
      } finally {
        running = false;
      }
    }
    void refresh();
    const timer = window.setInterval(() => void refresh(), 3000);
    returnButton.current?.focus();
    return () => {
      closed = true;
      window.clearInterval(timer);
    };
  }, [visible]);
  if (!desktop) return null;
  function show() {
    setVisible(true);
    setCloseRequested(false);
    setConfirm(false);
    setError("");
  }
  async function save(enabled: boolean) {
    setPending("save");
    setError("");
    try {
      const saved = await invoke<Status>("background_settings", { enabled });
      setStatus((previous) => ({ ...previous, ...saved }));
    } catch (e) {
      setError(message(e));
    } finally {
      setPending("");
    }
  }
  async function run(action: "detach_panel" | "shutdown_engine") {
    setPending(action);
    setError("");
    try {
      await invoke(action);
    } catch (e) {
      setError(message(e));
      setPending("");
      setConfirm(false);
    }
  }
  return (
    <>
      <button
        className="nav-item"
        onClick={show}
        title="Motor em segundo plano"
      >
        <Monitor size={18} /> <span>Segundo plano</span>
      </button>
      <DialogPresence>
        {visible &&
          createPortal(
            <DialogBackdrop className="modal-backdrop background-backdrop">
              <section
                className="modal help-modal"
                role="dialog"
                aria-modal="true"
                aria-labelledby="background-title"
                onKeyDown={(event) => {
                  if (event.key === "Escape" && !pending) {
                    event.stopPropagation();
                    if (confirm) setConfirm(false);
                    else setVisible(false);
                  }
                }}
              >
                <button
                  className="modal-close"
                  disabled={!!pending}
                  aria-label="Fechar opções do motor"
                  onClick={() => setVisible(false)}
                >
                  <X size={20} />
                </button>
                <h2 id="background-title">
                  {closeRequested
                    ? "Antes de fechar o Nethergate"
                    : "Motor em background"}
                </h2>
                <div className="form-info" role="status" aria-live="polite">
                  {refreshError ? (
                    <>
                      <strong>
                        Não foi possível atualizar o estado do motor.
                      </strong>
                      <p>
                        {refreshError} A lista anterior pode estar
                        desatualizada. Fechar só o painel não confirma a paragem
                        dos servidores.
                      </p>
                    </>
                  ) : !status?.servers ? (
                    "A consultar os servidores…"
                  ) : (
                    <>
                      <strong>
                        {status.servers.filter(active).length > 0
                          ? `${status.servers.filter(active).length} servidor(es) com atividade`
                          : "Nenhum servidor em execução"}
                      </strong>
                      {status.servers.filter(active).length > 0 && (
                        <ul>
                          {status.servers.filter(active).map((server) => (
                            <li key={server.id}>
                              {server.name} —{" "}
                              {stateLabels[server.state] ?? server.state}
                            </li>
                          ))}
                        </ul>
                      )}
                      {status.busy && (
                        <p>
                          Existem servidores ou operações em curso. Parar tudo
                          aguarda a conclusão segura.
                        </p>
                      )}
                    </>
                  )}
                </div>
                <p>
                  O motor mantém os servidores neste computador. Ao reabrir o
                  Nethergate, o painel volta a ligar aos servidores ativos.
                </p>
                <div className="form-info">
                  Fechar só o painel mantém servidores, túneis, backups e
                  DuckDNS já autorizados. O PC tem de ficar ligado: o motor
                  termina ao sair da sessão ou reiniciar o Windows.
                </div>
                {status && (
                  <>
                    <label className="eula-check">
                      <OreToggle
                        checked={status.keepRunning}
                        disabled={!!pending || status.closing}
                        onChange={(e) => void save(e.target.checked)}
                      />
                      <span>
                        Preferir manter o motor e os servidores ligados ao
                        fechar.
                      </span>
                    </label>
                    <p>
                      {status.keepRunning
                        ? "Fechar só o painel é a opção preferida. O botão X mostra este resumo antes de saíres."
                        : "Parar tudo e sair é a opção preferida. O botão X mostra este resumo para poderes escolher."}
                    </p>
                    {status.error && (
                      <p className="inline-error">{status.error}</p>
                    )}
                  </>
                )}
                <p className="players-hint">
                  Reabre pelo atalho habitual. As notificações Windows exigem o
                  painel aberto.
                </p>
                {confirm ? (
                  <div className="form-info">
                    <h3>Guardar os mundos e parar tudo?</h3>
                    <p>
                      Os jogadores serão desligados. O motor termina apenas
                      depois de parar os servidores e concluir as operações em
                      curso. Outras janelas ligadas a este motor também perdem a
                      ligação.
                    </p>
                    <div className="modal-actions">
                      <button
                        className="button secondary"
                        disabled={!!pending}
                        onClick={() => setConfirm(false)}
                      >
                        Voltar
                      </button>
                      <button
                        className="button primary"
                        disabled={!!pending}
                        onClick={() => void run("shutdown_engine")}
                      >
                        {pending && <LoaderCircle size={16} className="spin" />}{" "}
                        Guardar, parar tudo e sair
                      </button>
                    </div>
                  </div>
                ) : (
                  <div className="modal-actions">
                    <button
                      ref={returnButton}
                      className="button secondary"
                      disabled={!!pending}
                      onClick={() => setVisible(false)}
                    >
                      Voltar ao painel
                    </button>
                    <button
                      className={`button ${status?.keepRunning ? "primary" : "secondary"}`}
                      disabled={!!pending}
                      onClick={() => void run("detach_panel")}
                    >
                      Fechar só o painel
                    </button>
                    <button
                      className={`button ${status?.keepRunning ? "secondary" : "primary"}`}
                      disabled={!!pending}
                      onClick={() => setConfirm(true)}
                    >
                      Parar tudo e sair…
                    </button>
                  </div>
                )}
                {pending === "shutdown_engine" && (
                  <p role="status">
                    A guardar e parar. Aguarda a conclusão; não desligues o
                    computador.
                  </p>
                )}
                {error && (
                  <p className="inline-error" role="alert">
                    {error}
                  </p>
                )}
              </section>
            </DialogBackdrop>,
            document.body,
          )}
      </DialogPresence>
    </>
  );
}
