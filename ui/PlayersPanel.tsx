import { errorText as message, formatTime } from "./lib/format";
import type { Player, Access } from "./api/models/PlayersPanel";
import { DialogBackdrop, DialogPresence } from "./DialogPresence";
import { useEffect, useState } from "react";
import {
  LoaderCircle,
  Plus,
  RefreshCw,
  ShieldCheck,
  Trash2,
  Users,
  X,
} from "lucide-react";
import { api, type Server } from "./api";
import { PlayerAvatar } from "./PlayerAvatar";

export function PlayersPanel({ server }: { server: Server }) {
  const [access, setAccess] = useState<Access | null>(null);
  const [name, setName] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [pending, setPending] = useState(false);
  const [online, setOnline] = useState<string[] | null>(null);
  const [onlineError, setOnlineError] = useState("");
  const [checking, setChecking] = useState(false);
  const [checkedAt, setCheckedAt] = useState("");
  const [remove, setRemove] = useState<Player | null>(null);
  const live = server.state === "online";
  const editable = live && !!access?.authenticated && !pending && !checking;
  async function load() {
    setError("");
    try {
      setAccess(await api("players", { id: server.id }));
    } catch (e) {
      setError(message(e));
    }
  }
  useEffect(() => {
    void load();
  }, [server.id, server.state]);
  useEffect(() => {
    setOnline(null);
    setCheckedAt("");
    setOnlineError("");
  }, [server.id, server.state]);
  async function mutate(operation: string, target = "") {
    setPending(true);
    setError("");
    setNotice("");
    try {
      setAccess(
        await api("player_operation", {
          id: server.id,
          operation,
          name: target,
        }),
      );
      setName("");
      setRemove(null);
      setNotice(
        operation === "enable"
          ? "Whitelist ativa. Só os jogadores autorizados e operadores podem entrar."
          : operation === "disable"
            ? "Whitelist desativada. Qualquer conta oficial pode tentar entrar."
            : operation === "add"
              ? `${target} foi adicionado à whitelist.`
              : `${target} foi removido da whitelist.`,
      );
    } catch (e) {
      setError(message(e));
    } finally {
      setPending(false);
    }
  }
  async function checkOnline() {
    setChecking(true);
    setOnlineError("");
    setOnline(null);
    try {
      setOnline(await api("online_players", { id: server.id }));
      setCheckedAt(formatTime(Date.now()));
    } catch (e) {
      setOnlineError(message(e));
    } finally {
      setChecking(false);
    }
  }
  return (
    <div className="players-panel">
      <section className="settings-panel">
        <div className="players-heading">
          <div>
            <h2>
              <ShieldCheck size={23} /> Acesso ao servidor
            </h2>
          </div>
          <button
            className="button secondary"
            disabled={pending}
            onClick={() => void load()}
          >
            <RefreshCw size={15} /> Atualizar whitelist
          </button>
        </div>
        {!live && (
          <div className="form-info">
            Inicia o servidor para editar a whitelist.
          </div>
        )}
        {access && !access.authenticated && (
          <p className="inline-error">
            Esta gestão exige autenticação oficial (online-mode=true). A
            whitelist sozinha não confirma a identidade de uma conta.
          </p>
        )}
        {access && (
          <>
            <div className="players-access">
              <div>
                <strong>
                  Whitelist {access.enabled ? "ativa" : "desativada"}
                </strong>
                <p>
                  {access.enabled
                    ? "A lista controla o acesso. Operadores podem contornar a whitelist."
                    : "A lista está guardada, mas ainda não limita quem pode entrar."}
                </p>
              </div>
              <button
                className={`button ${access.enabled ? "secondary" : "primary"}`}
                disabled={
                  !editable || (!access.enabled && !access.players.length)
                }
                onClick={() =>
                  void mutate(access.enabled ? "disable" : "enable")
                }
              >
                {pending ? (
                  <LoaderCircle size={16} className="spin" />
                ) : (
                  <ShieldCheck size={16} />
                )}
                {access.enabled ? "Desativar whitelist" : "Ativar whitelist"}
              </button>
            </div>
            {!access.enabled && !access.players.length && (
              <p className="players-hint">
                Adiciona primeiro o teu nome de Minecraft e os nomes dos teus
                amigos; depois ativa a whitelist.
              </p>
            )}
            <form
              className="players-add"
              onSubmit={(e) => {
                e.preventDefault();
                void mutate("add", name.trim());
              }}
            >
              <label>
                Nome no Minecraft
                <input
                  required
                  minLength={3}
                  maxLength={16}
                  pattern="[A-Za-z0-9_]{3,16}"
                  placeholder="Ex.: PlayerOne"
                  value={name}
                  disabled={!editable}
                  onChange={(e) => setName(e.target.value)}
                />
              </label>
              <button
                className="button primary"
                disabled={
                  !editable || !/^[A-Za-z0-9_]{3,16}$/.test(name.trim())
                }
              >
                <Plus size={16} /> Adicionar
              </button>
              <small className="form-hint">
                Nome do jogo, sem email. A verificação da conta pode precisar de
                internet.
              </small>
            </form>
            <div className="section-heading">
              <h3>
                Jogadores autorizados <span>({access.players.length})</span>
              </h3>
            </div>
            <div className="players-list">
              {access.players.length ? (
                access.players.map((player) => (
                  <div className="player-row" key={player.uuid}>
                    <PlayerAvatar name={player.name} uuid={player.uuid} />
                    <div>
                      <strong title={player.uuid}>{player.name}</strong>
                    </div>
                    <button
                      className="button secondary"
                      aria-label={`Remover ${player.name}`}
                      disabled={!editable}
                      onClick={() => setRemove(player)}
                    >
                      <Trash2 size={15} /> Remover
                    </button>
                  </div>
                ))
              ) : (
                <p className="players-hint">
                  Ainda não há jogadores na whitelist.
                </p>
              )}
            </div>
          </>
        )}
        {pending && (
          <p className="players-hint" role="status">
            A aguardar confirmação do Minecraft…
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
      <section className="settings-panel">
        <div className="players-heading">
          <div>
            <h2>
              <Users size={23} /> Jogadores online
            </h2>
            <p>
              {live
                ? "Lista atualizada ao consultar o servidor."
                : "Disponível com o servidor ligado."}
            </p>
          </div>
          <button
            className="button secondary"
            disabled={!live || pending || checking}
            onClick={() => void checkOnline()}
          >
            {checking ? (
              <LoaderCircle size={16} className="spin" />
            ) : (
              <RefreshCw size={16} />
            )}{" "}
            Atualizar ligados
          </button>
        </div>
        {live && online && (
          <>
            <p className="players-hint">
              {online.length} jogador(es) · Consulta às {checkedAt}
            </p>
            <div className="players-list">
              {online.length ? (
                online.map((n) => (
                  <div className="player-row" key={n}>
                    <PlayerAvatar
                      name={n}
                      uuid={
                        access?.players.find(
                          (player) =>
                            player.name.toLowerCase() === n.toLowerCase(),
                        )?.uuid
                      }
                    />
                    <strong>{n}</strong>
                  </div>
                ))
              ) : (
                <p className="players-hint">Não há jogadores ligados.</p>
              )}
            </div>
          </>
        )}
        {live && onlineError && (
          <p className="inline-error" role="alert">
            {onlineError}
          </p>
        )}
      </section>
      <DialogPresence>
        {remove && (
          <DialogBackdrop className="modal-backdrop">
            <section
              className="modal"
              role="dialog"
              aria-modal="true"
              aria-labelledby="remove-player-title"
            >
              <button
                className="modal-close"
                aria-label="Cancelar remoção"
                disabled={pending}
                onClick={() => setRemove(null)}
              >
                <X size={20} />
              </button>
              <h2 id="remove-player-title">Remover {remove.name}?</h2>
              <p>
                Com a whitelist ativa, este jogador deixa de poder voltar a
                entrar, exceto se for operador. Isto não apaga o inventário nem
                os dados do jogador. O servidor pode desligá-lo se tiver
                enforce-whitelist ativo.
              </p>
              <div className="modal-actions">
                <button
                  className="button secondary"
                  disabled={pending}
                  onClick={() => setRemove(null)}
                >
                  Cancelar
                </button>
                <button
                  className="button primary"
                  disabled={!editable}
                  onClick={() => void mutate("remove", remove.name)}
                >
                  Confirmar remoção
                </button>
              </div>
              {error && (
                <p className="inline-error" role="alert">
                  {error}
                </p>
              )}
            </section>
          </DialogBackdrop>
        )}
      </DialogPresence>
    </div>
  );
}
