import { errorText } from "./lib/format";
import { scopedApi, type ScopedArgs } from "./api";
import type { Jar, Review } from "./api/models/ManualJarsPanel";
import { OreToggle } from "./OreToggle";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api, desktop, type Server } from "./api";

export function ManualJarsPanel({
  server,
  onChange,
}: {
  server: Server;
  onChange: () => void;
}) {
  const [files, setFiles] = useState<Jar[]>([]);
  const [disabled, setDisabled] = useState<Jar[]>([]);
  const [path, setPath] = useState("");
  const [file, setFile] = useState<Jar | null>(null);
  const [review, setReview] = useState<Review | null>(null);
  const [consent, setConsent] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  async function load() {
    const v = await api("manual_jars", {
      id: server.id,
    });
    setFiles(v.files);
    setDisabled(v.disabled);
  }
  useEffect(() => {
    let closed = false;
    api("manual_jars", { id: server.id })
      .then((v) => {
        if (!closed) {
          setFiles(v.files);
          setDisabled(v.disabled);
        }
      })
      .catch((e) => {
        if (!closed) setError(errorText(e));
      });
    return () => {
      closed = true;
    };
  }, [server.id, server.state]);
  const stopped = ["offline", "error"].includes(server.state) && !server.pid;
  async function task(fn: () => Promise<void>) {
    setPending(true);
    setError("");
    setNotice("");
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(false);
    }
  }
  async function choose() {
    await task(async () => {
      const v = await invoke<string | null>("choose_jar_file");
      if (v) {
        setPath(v);
        setFile(await api("manual_jar_file_preview", { path: v }));
      }
    });
  }
  async function change<
    A extends
      | "manual_jar_add"
      | "manual_jar_disable"
      | "manual_jar_enable"
      | "manual_jar_adopt",
  >(action: A, ...args: ScopedArgs<A, { id: string }>) {
    await task(async () => {
      await scopedApi({ id: server.id })(action, ...args);
      setReview(null);
      setFile(null);
      await load();
      onChange();
      setNotice(
        "Alteração concluída. Os JARs só são carregados no próximo arranque.",
      );
    });
  }
  return (
    <article className="connection-card manual-jars-panel">
      <p>
        Importa JARs ou desativa-os sem apagar. São código executável: usa
        apenas fontes em que confias.
      </p>
      <fieldset disabled={pending || !stopped}>
        {desktop && (
          <button className="button secondary" onClick={() => void choose()}>
            Adicionar JAR local…
          </button>
        )}
        {!desktop && (
          <>
            <label>
              Caminho do JAR
              <input
                value={path}
                onChange={(e) => {
                  setPath(e.target.value);
                  setFile(null);
                }}
              />
            </label>
            <button
              className="button secondary"
              onClick={() =>
                void task(async () =>
                  setFile(await api("manual_jar_file_preview", { path })),
                )
              }
            >
              Rever ficheiro
            </button>
          </>
        )}
        {file && (
          <div className="form-info">
            <p>
              {file.filename} · {(file.size / 1048576).toFixed(2)} MB
            </p>
            <p className="firewall-path">SHA-512: {file.sha512}</p>
            <button
              className="button primary"
              onClick={() =>
                void change("manual_jar_add", { path, sha512: file.sha512 })
              }
            >
              Copiar para este servidor
            </button>
          </div>
        )}
        <label className="eula-check">
          <OreToggle
            checked={consent}
            onChange={(e) => setConsent(e.target.checked)}
          />
          <span>
            Autorizar identificação no Modrinth: envia apenas o SHA-512, nunca o
            ficheiro.
          </span>
        </label>
        {files.map((jar) => (
          <div className="plugin-row" key={jar.filename}>
            <div>
              <strong>{jar.filename}</strong>
              <p>{(jar.size / 1048576).toFixed(2)} MB</p>
              {(jar.duplicates?.length ?? 0) > 1 && (
                <p className="inline-error">
                  Conteúdo duplicado: {jar.duplicates!.join(", ")}
                </p>
              )}
            </div>
            <button
              className="button secondary"
              disabled={!consent}
              onClick={() =>
                void task(async () =>
                  setReview(
                    await api("manual_jar_preview", {
                      id: server.id,
                      filename: jar.filename,
                      consent,
                    }),
                  ),
                )
              }
            >
              Identificar
            </button>
            <button
              className="button secondary"
              onClick={() =>
                void change("manual_jar_disable", {
                  filename: jar.filename,
                  sha512: jar.sha512,
                })
              }
            >
              Desativar
            </button>
          </div>
        ))}
        {review && (
          <div className="form-info">
            <p>
              <strong>{review.title}</strong> · {review.versionNumber} ·
              ficheiro {review.sourceFilename}
            </p>
            <p>
              Associar coloca este JAR no registo do gestor e preserva o
              ficheiro original numa cópia de segurança. Revê depois as
              dependências pelo solver.
            </p>
            <button
              className="button primary"
              disabled={!consent}
              onClick={() =>
                void change("manual_jar_adopt", {
                  filename: review.sourceFilename,
                  fingerprint: review.fingerprint,
                  consent,
                })
              }
            >
              Associar ao gestor
            </button>
          </div>
        )}
        {disabled.map((jar) => (
          <div className="plugin-row" key={jar.id}>
            <div>
              <strong>{jar.filename}</strong>
              <p>Desativado, ficheiro preservado</p>
            </div>
            <button
              className="button secondary"
              onClick={() =>
                void change("manual_jar_enable", { disabledId: jar.id })
              }
            >
              Reativar
            </button>
          </div>
        ))}
      </fieldset>
      {!stopped && <p>Para o servidor para gerir os JARs.</p>}
      {pending && <p role="status">A verificar os ficheiros…</p>}
      {notice && <p role="status">{notice}</p>}
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </article>
  );
}
