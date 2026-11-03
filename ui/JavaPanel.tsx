import { errorText } from "./lib/format";
import { useEffect, useState } from "react";
import { Download, LoaderCircle } from "lucide-react";
import { api, type Server } from "./api";

export function JavaPanel({ server }: { server: Server }) {
  const [status, setStatus] = useState<{
    compatible: boolean;
    detail: string | null;
  } | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const busy = pending || !["offline", "error"].includes(server.state);
  useEffect(() => {
    setStatus(null);
  }, [server.javaPath, server.state]);
  async function run(prepare: boolean) {
    setPending(true);
    setError("");
    setNotice("");
    try {
      if (prepare) {
        await api("prepare_java", { id: server.id });
        setStatus(null);
        setNotice(
          "Preparação iniciada. Acompanha os passos na Consola; o servidor fica parado no fim.",
        );
      } else setStatus(await api("java_status", { id: server.id }));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(false);
    }
  }
  return (
    <article className="connection-card java-panel">
      <p>
        Java mínimo: {server.javaMajor}.{" "}
        {server.javaPath === "auto"
          ? "Modo automático: descarrega e guarda Temurin numa pasta do Nethergate."
          : "O motor verifica o executável configurado antes de iniciar."}
      </p>
      <p>
        O modo automático instala Java 21 ou 25 no Windows x64, verifica o
        SHA-256 e partilha o runtime entre servidores. Não altera o Java do
        Windows nem pede administrador.
      </p>
      <div className="modal-actions">
        <button
          type="button"
          className="button secondary"
          disabled={busy}
          onClick={() => void run(false)}
        >
          Verificar Java
        </button>
        <button
          type="button"
          className="button primary"
          disabled={busy}
          onClick={() => void run(true)}
        >
          {pending ? (
            <LoaderCircle size={16} className="spin" />
          ) : (
            <Download size={16} />
          )}{" "}
          Preparar Java
        </button>
      </div>
      {status && (
        <p
          className={status.compatible ? "gameplay-success" : "inline-error"}
          role="status"
        >
          {status.compatible
            ? "Java compatível e de 64 bits confirmado."
            : status.detail}
        </p>
      )}
      {notice && <p role="status">{notice}</p>}
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </article>
  );
}
