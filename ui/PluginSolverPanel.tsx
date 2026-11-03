import { errorText } from "./lib/format";
import type { Bundle } from "./api/models/PluginSolverPanel";
import { DialogBackdrop } from "./DialogPresence";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { api, type Server } from "./api";

export function PluginSolverPanel({
  server,
  project,
  onClose,
  onInstalled,
}: {
  server: Server;
  project: string;
  onClose: () => void;
  onInstalled: (text: string) => void;
}) {
  const [plan, setPlan] = useState<Bundle | null>(null);
  const [error, setError] = useState("");
  const [pending, setPending] = useState(true);
  useEffect(() => {
    let closed = false;
    api("addon_set_preview", {
      id: server.id,
      projects: project ? [project] : [],
    })
      .then((v) => {
        if (!closed) setPlan(v);
      })
      .catch((e) => {
        if (!closed) setError(errorText(e));
      })
      .finally(() => {
        if (!closed) setPending(false);
      });
    return () => {
      closed = true;
    };
  }, [server.id, project]);
  async function install() {
    if (!plan) return;
    setPending(true);
    setError("");
    try {
      const v = await api("addon_set_install", {
        id: server.id,
        projects: plan.projects,
        fingerprint: plan.fingerprint,
      });
      onInstalled(
        `${v.changed} mods/plugins instalados ou atualizados. Cópias anteriores: ${v.snapshot}. Inicia o servidor para os carregar.`,
      );
      onClose();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(false);
    }
  }
  const stopped = ["offline", "error"].includes(server.state) && !server.pid;
  return createPortal(
    <DialogBackdrop className="modal-backdrop">
      <section
        className="modal help-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="solver-title"
      >
        <button
          className="modal-close"
          disabled={pending}
          onClick={onClose}
          aria-label="Fechar resolução"
        >
          <X size={20} />
        </button>
        <h2 id="solver-title">
          Rever o conjunto de{" "}
          {server.software === "fabric" ? "mods" : "plugins"}
        </h2>
        <p>
          Procura versões e dependências compatíveis com Minecraft{" "}
          {server.version}. Pode alterar projetos já instalados: revê a lista
          antes de aplicar.
        </p>
        {pending && (
          <p role="status">
            A consultar o catálogo ou a preparar os ficheiros verificados.
            Aguarda a conclusão.
          </p>
        )}
        {plan && (
          <>
            {!plan.items.length && (
              <p>
                O conjunto já está nas versões compatíveis selecionadas. Não há
                alterações a aplicar.
              </p>
            )}
            <ul>
              {plan.items.map((item) => (
                <li key={item.projectId}>
                  <strong>{item.title}</strong> ·{" "}
                  {item.previous
                    ? `${item.previous.versionNumber} → ${item.versionNumber}`
                    : `Instalar ${item.versionNumber}`}{" "}
                  · {(item.size / 1048576).toFixed(1)} MB
                  {item.distribution.requiredProjects.length > 0 && (
                    <p>
                      Dependências:{" "}
                      {item.distribution.requiredProjects.join(", ")}
                    </p>
                  )}
                </li>
              ))}
            </ul>
            {plan.warnings.map((text, i) => (
              <p className="form-info" key={i}>
                {text}
              </p>
            ))}
            <p>
              Todos os downloads são verificados antes de trocar a pasta ativa.
              Preserva uma cópia da pasta anterior e cria um backup dos mundos
              existentes. Uma alteração interrompida bloqueia o arranque até
              recuperação. Esta operação atualiza as versões dos mods/plugins;
              não altera a versão do Minecraft.
            </p>
          </>
        )}
        {!stopped && (
          <p className="inline-error">Para o servidor antes de aplicar.</p>
        )}
        {error && (
          <p className="inline-error" role="alert">
            {error}
          </p>
        )}
        <div className="modal-actions">
          <button
            className="button secondary"
            disabled={pending}
            onClick={onClose}
          >
            Voltar
          </button>
          <button
            className="button primary"
            disabled={pending || !stopped || !plan?.items.length || !!error}
            onClick={() => void install()}
          >
            Aplicar todas as alterações
          </button>
        </div>
      </section>
    </DialogBackdrop>,
    document.body,
  );
}
