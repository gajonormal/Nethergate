import { errorText as message } from "./lib/format";
import type { Info, Preview } from "./api/models/LibraryPanel";
import { DialogBackdrop, DialogPresence } from "./DialogPresence";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FolderOpen, LoaderCircle, X } from "lucide-react";
import { desktop } from "./api";

export function LibraryPanel({
  busy,
  onChanged,
}: {
  busy: boolean;
  onChanged: () => Promise<void>;
}) {
  const [visible, setVisible] = useState(false);
  const [info, setInfo] = useState<Info | null>(null);
  const [preview, setPreview] = useState<Preview | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (desktop)
      void invoke<Info>("library_info")
        .then(setInfo)
        .catch((e) => setError(message(e)));
  }, []);
  if (!desktop) return null;
  async function show() {
    setVisible(true);
    setPreview(null);
    setError("");
    try {
      setInfo(await invoke<Info>("library_info"));
    } catch (e) {
      setError(message(e));
    }
  }
  async function choose() {
    setPending(true);
    setError("");
    try {
      setPreview(await invoke<Preview | null>("pick_library"));
    } catch (e) {
      setError(message(e));
    } finally {
      setPending(false);
    }
  }
  async function open() {
    if (!preview) return;
    setPending(true);
    setError("");
    try {
      await invoke("open_library", {
        path: preview.root,
        fingerprint: preview.fingerprint,
      });
      setInfo(await invoke<Info>("library_info"));
      await onChanged();
      setVisible(false);
      setPreview(null);
    } catch (e) {
      setError(message(e));
    } finally {
      setPending(false);
    }
  }
  return (
    <>
      <button className="nav-item" onClick={() => void show()}>
        <FolderOpen size={18} /> <span>Biblioteca</span>
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
              className="modal help-modal library-modal"
              role="dialog"
              aria-modal="true"
              aria-labelledby="library-title"
              onClick={(e) => e.stopPropagation()}
            >
              <button
                className="modal-close"
                aria-label="Fechar biblioteca"
                disabled={pending}
                onClick={() => setVisible(false)}
              >
                <X size={20} />
              </button>
              <h2 id="library-title">Biblioteca</h2>
              <p>
                Abre uma biblioteca Nethergate existente. Os servidores
                mantêm-se na pasta original.
              </p>
              {info && (
                <div className="internet-step">
                  <strong>
                    Biblioteca aberta · {info.serverCount} servidor(es)
                  </strong>
                  <p className="library-path">{info.root}</p>
                </div>
              )}
              {info?.warning && (
                <p className="inline-error" role="status">
                  {info.warning}
                </p>
              )}
              {busy && (
                <p role="status">
                  Para os servidores e aguarda os backups e outras operações
                  antes de mudar de biblioteca.
                </p>
              )}
              <button
                className="button secondary"
                disabled={pending || busy}
                onClick={() => void choose()}
              >
                {pending ? (
                  <LoaderCircle size={16} className="spin" />
                ) : (
                  <FolderOpen size={16} />
                )}{" "}
                Escolher pasta existente
              </button>
              {preview && (
                <div className="internet-step">
                  <h3>
                    Biblioteca encontrada · {preview.servers.length}{" "}
                    servidor(es)
                  </h3>
                  <p className="library-path">{preview.root}</p>
                  <ul>
                    {preview.servers.map((s, i) => (
                      <li key={i}>
                        {s.name} · {s.software} {s.version} · porta {s.port}
                      </li>
                    ))}
                  </ul>
                  {preview.externalDirectories && (
                    <p>
                      Alguns servidores estão guardados noutras pastas
                      referenciadas por esta biblioteca. Essas pastas têm de
                      continuar disponíveis.
                    </p>
                  )}
                  <p>
                    A escolha fica guardada para os próximos arranques. Os
                    ficheiros dos mundos permanecem nas pastas atuais.
                  </p>
                  <button
                    className="button primary"
                    disabled={pending || busy}
                    onClick={() => void open()}
                  >
                    {pending && <LoaderCircle size={16} className="spin" />}{" "}
                    Abrir esta biblioteca
                  </button>
                </div>
              )}
              {error && (
                <p className="inline-error" role="alert">
                  {error}
                </p>
              )}
            </section>
          </DialogBackdrop>
        )}
      </DialogPresence>
    </>
  );
}
