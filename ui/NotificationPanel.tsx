import { errorText } from "./lib/format";
import type { Settings } from "./api/models/NotificationPanel";
import { DialogBackdrop, DialogPresence } from "./DialogPresence";
import { OreToggle } from "./OreToggle";
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Bell, X, LoaderCircle } from "lucide-react";
import { desktop } from "./api";

export function NotificationPanel() {
  const [visible, setVisible] = useState(false);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  if (!desktop) return null;
  async function show() {
    setVisible(true);
    setError("");
    try {
      setSettings(await invoke<Settings>("notification_settings"));
    } catch (e) {
      setError(errorText(e));
    }
  }
  async function change(enabled: boolean) {
    setPending(true);
    setError("");
    try {
      setSettings(await invoke<Settings>("notification_settings", { enabled }));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(false);
    }
  }
  return (
    <>
      <button className="nav-item" onClick={() => void show()}>
        <Bell size={18} /> <span>Notificações</span>
      </button>
      <DialogPresence>
        {visible && (
          <DialogBackdrop
            className="modal-backdrop"
            onClick={() => {
              if (!pending) setVisible(false);
            }}
          >
            <section
              className="modal help-modal"
              role="dialog"
              aria-modal="true"
              aria-labelledby="notification-title"
              onClick={(e) => e.stopPropagation()}
            >
              <button
                className="modal-close"
                aria-label="Fechar notificações"
                disabled={pending}
                onClick={() => setVisible(false)}
              >
                <X size={20} />
              </button>
              <h2 id="notification-title">Notificações</h2>
              <p>
                Recebe avisos sobre servidores, backups, entrada de jogadores e
                utilização da bateria.
              </p>
              {settings && (
                <label className="checkbox-row">
                  <OreToggle
                    checked={settings.enabled}
                    disabled={pending}
                    onChange={(e) => void change(e.target.checked)}
                  />{" "}
                  Ativar neste computador{" "}
                  {pending && <LoaderCircle size={16} className="spin" />}
                </label>
              )}
              <p>
                Requer a aplicação instalada e o painel aberto. As definições do
                Windows e o modo Não incomodar podem silenciar os avisos.
              </p>
              {(error || settings?.error) && (
                <p className="inline-error" role="alert">
                  {error || settings?.error}
                </p>
              )}
              {!!settings?.submitted && (
                <p>
                  {settings.submitted} aviso(s) aceites pelo Windows nesta
                  sessão.
                </p>
              )}
            </section>
          </DialogBackdrop>
        )}
      </DialogPresence>
    </>
  );
}
