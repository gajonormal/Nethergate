import { errorText } from "./lib/format";
import type { Preview } from "./api/models/ServerTransferPanel";
import { DialogBackdrop, DialogPresence } from "./DialogPresence";
import { OreToggle } from "./OreToggle";
import { useState } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { api, desktop, openEula, type Server } from "./api";

export function ServerTransferPanel({
  server,
  onImported,
}: {
  server?: Server;
  onImported?: () => void;
}) {
  const [visible, setVisible] = useState(false);
  const [path, setPath] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [name, setName] = useState("");
  const [port, setPort] = useState(25565);
  const [memory, setMemory] = useState(2048);
  const [contact, setContact] = useState("");
  const [java, setJava] = useState("auto");
  const [accepted, setAccepted] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  async function choose() {
    setError("");
    try {
      const v = await invoke<string | null>("choose_data_folder");
      if (v) {
        setPath(v);
        setPreview(null);
      }
    } catch (e) {
      setError(errorText(e));
    }
  }
  async function review() {
    setPending(true);
    setError("");
    try {
      const p = await api("preview_import", { path });
      setPreview(p);
      setName(p.profile.name);
      setMemory(p.profile.memoryMb);
      setAccepted(false);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(false);
    }
  }
  async function run() {
    setPending(true);
    setError("");
    setNotice("");
    try {
      if (server) {
        const result = await api("export_server", { id: server.id, path });
        setNotice(
          `Pacote criado em ${result.path} · ${result.fileCount} ficheiros verificados. Copia a pasta completa para o outro computador.`,
        );
      } else if (preview) {
        await api("import_server", {
          path: preview.path,
          fingerprint: preview.fingerprint,
          name,
          port,
          memoryMb: memory,
          contact,
          javaPath: java,
          eulaAccepted: accepted,
        });
        setPreview(null);
        setNotice(
          "Servidor importado num perfil novo e parado. Revê Java, whitelist e partilha antes de iniciar.",
        );
        onImported?.();
      }
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(false);
    }
  }
  return (
    <>
      <button
        className="button secondary"
        disabled={
          !!server &&
          (!["offline", "error"].includes(server.state) || !!server.pid)
        }
        onClick={() => {
          setVisible(true);
          setError("");
        }}
      >
        {server ? "Exportar servidor…" : "Importar servidor…"}
      </button>
      <DialogPresence>
        {visible &&
          createPortal(
            <DialogBackdrop className="modal-backdrop">
              <section
                className="modal help-modal"
                role="dialog"
                aria-modal="true"
                aria-labelledby="transfer-title"
              >
                <button
                  className="modal-close"
                  disabled={pending}
                  onClick={() => setVisible(false)}
                  aria-label="Fechar transferência"
                >
                  <X size={20} />
                </button>
                <h2 id="transfer-title">
                  {server
                    ? "Levar o servidor para outro PC"
                    : "Importar um servidor"}
                </h2>
                <p>
                  O pacote inclui mundos, JARs, whitelist e definições de jogo.
                  Importa apenas pacotes de confiança: os JARs são executados ao
                  iniciar o servidor.
                </p>
                <details>
                  <summary>O que configurar no outro PC?</summary>
                  <p>
                    Java, credenciais DuckDNS/playit, permissões de rede,
                    operadores e configurações privadas dos plugins/mods não são
                    incluídos. Volta a configurá-los no destino.
                  </p>
                </details>
                <fieldset disabled={pending}>
                  <label>
                    {server
                      ? "Pasta onde criar o pacote"
                      : "Pasta do pacote Nethergate"}
                    <input
                      value={path}
                      onChange={(e) => {
                        setPath(e.target.value);
                        setPreview(null);
                      }}
                    />
                  </label>
                  {desktop && (
                    <button
                      className="button secondary"
                      onClick={() => void choose()}
                    >
                      Escolher pasta…
                    </button>
                  )}
                  {!server && (
                    <button
                      className="button primary"
                      disabled={!path}
                      onClick={() => void review()}
                    >
                      Verificar e rever pacote
                    </button>
                  )}
                  {preview && (
                    <>
                      <p>
                        <strong>
                          {preview.profile.software} · Minecraft{" "}
                          {preview.profile.minecraft}
                        </strong>
                        <br />
                        {preview.fileCount} ficheiros ·{" "}
                        {(preview.bytes / 1024 / 1024).toFixed(1)} MB · Java{" "}
                        {preview.profile.javaMajor} ou superior
                      </p>
                      <p>
                        A importação cria um perfil novo, apenas neste PC, com
                        autenticação e whitelist ativas. Não substitui
                        servidores existentes.
                      </p>
                      <label>
                        Nome
                        <input
                          required
                          maxLength={60}
                          value={name}
                          onChange={(e) => setName(e.target.value)}
                        />
                      </label>
                      <div className="form-row">
                        <label>
                          Porta
                          <input
                            type="number"
                            min={1}
                            max={65535}
                            value={port}
                            onChange={(e) => setPort(Number(e.target.value))}
                          />
                        </label>
                        <label>
                          RAM (MB)
                          <input
                            type="number"
                            min={512}
                            max={65536}
                            value={memory}
                            onChange={(e) => setMemory(Number(e.target.value))}
                          />
                        </label>
                      </div>
                      <label>
                        Contacto (email ou URL HTTPS)
                        <input
                          value={contact}
                          onChange={(e) => setContact(e.target.value)}
                        />
                      </label>
                      <label>
                        Java (auto ou caminho neste PC)
                        <input
                          value={java}
                          onChange={(e) => setJava(e.target.value)}
                        />
                      </label>
                      <ul>
                        {preview.reconfigure.map((item) => (
                          <li key={item}>{item}</li>
                        ))}
                      </ul>
                      <label className="eula-check">
                        <OreToggle
                          checked={accepted}
                          onChange={(e) => setAccepted(e.target.checked)}
                        />
                        <span>Li e aceito a EULA do Minecraft.</span>
                      </label>
                      <button
                        className="button secondary"
                        onClick={() => void openEula()}
                      >
                        Ler EULA
                      </button>
                    </>
                  )}
                  {(server || preview) && (
                    <button
                      className="button primary"
                      disabled={
                        !path ||
                        (!server &&
                          (!accepted ||
                            !name.trim() ||
                            !contact.trim() ||
                            !java.trim()))
                      }
                      onClick={() => void run()}
                    >
                      {pending
                        ? "A verificar e copiar…"
                        : server
                          ? "Criar pacote de transferência"
                          : "Importar para um novo perfil"}
                    </button>
                  )}
                </fieldset>
                {pending && (
                  <p role="status">
                    A verificar e copiar. Aguarda a conclusão antes de desligar
                    o disco.
                  </p>
                )}
                {error && (
                  <p className="inline-error" role="alert">
                    {error}
                  </p>
                )}
                {notice && (
                  <p className="gameplay-success" role="status">
                    {notice}
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
