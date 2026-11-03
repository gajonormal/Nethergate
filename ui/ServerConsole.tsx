import { errorText } from "./lib/format";
import React, { useEffect, useRef, useState } from "react";
import { ArrowRight, ChevronRight, Folder, Terminal } from "lucide-react";
import { api, type Server, type LogLine } from "./api";
import { OreToggle } from "./OreToggle";

export function Console({
  server,
  onError,
}: {
  server: Server;
  onError: (message: string) => void;
}) {
  const [logs, setLogs] = useState<LogLine[]>([]);
  const [command, setCommand] = useState("");
  const [sending, setSending] = useState(false);
  const [follow, setFollow] = useState(true);
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    setLogs([]);
    let after = 0;
    let closed = false;
    let pending = false;
    async function poll() {
      if (pending) return;
      pending = true;
      try {
        const rows = await api("logs", { id: server.id, after });
        if (!closed && rows.length) {
          after = rows[rows.length - 1].seq;
          setLogs((old) => [...old, ...rows].slice(-600));
        }
      } catch (e) {
        if (!closed) onError(errorText(e));
      } finally {
        pending = false;
      }
    }
    void poll();
    const timer = setInterval(() => void poll(), 800);
    return () => {
      closed = true;
      clearInterval(timer);
    };
  }, [server.id]);
  useEffect(() => {
    const output = end.current?.parentElement;
    if (follow && output) output.scrollTop = output.scrollHeight;
  }, [logs, follow]);
  async function send(e: React.FormEvent) {
    e.preventDefault();
    if (!command.trim()) return;
    setSending(true);
    try {
      await api("command", { id: server.id, command });
      setCommand("");
    } catch (e) {
      onError(errorText(e));
    } finally {
      setSending(false);
    }
  }
  return (
    <section className="console-panel">
      <div className="console-toolbar">
        <span>
          <span
            className={`console-dot ${server.state === "online" ? "live" : ""}`}
          />
          Saída do servidor
        </span>
        <label>
          <OreToggle
            checked={follow}
            onChange={(e) => setFollow(e.target.checked)}
          />
          Seguir consola
        </label>
      </div>
      <div className="console-output" aria-label="Registos do servidor">
        {!logs.length ? (
          <div className="console-empty">
            <Terminal size={30} />
            <strong>A tua aventura começa aqui.</strong>
            <p>
              {server.installed
                ? "Inicia o servidor para acompanhar a consola em tempo real."
                : "Prepara o servidor para descarregar o software selecionado."}
            </p>
          </div>
        ) : (
          logs.map((line) => (
            <div
              key={line.seq}
              className={
                /ERROR|Exception|Falha/.test(line.text)
                  ? "log-error"
                  : /^\[Nethergate\]/i.test(line.text)
                    ? "log-system"
                    : line.text.startsWith(">")
                      ? "log-command"
                      : ""
              }
            >
              {line.text.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "")}
            </div>
          ))
        )}
        <div ref={end} />
      </div>
      <form className="console-input" onSubmit={(e) => void send(e)}>
        <ChevronRight size={16} />
        <input
          aria-label="Comando do servidor"
          placeholder={
            server.state === "online"
              ? "Escreve um comando, por exemplo: list"
              : "A consola fica disponível quando o servidor estiver online"
          }
          value={command}
          disabled={server.state !== "online" || sending}
          onChange={(e) => setCommand(e.target.value)}
        />
        <button
          type="submit"
          disabled={server.state !== "online" || sending || !command.trim()}
          aria-label="Enviar comando"
        >
          <ArrowRight size={19} />
        </button>
      </form>
      <div className="panel-footnote">
        <Folder size={13} />
        <span>{server.directory}</span>
      </div>
    </section>
  );
}
