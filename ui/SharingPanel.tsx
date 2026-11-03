import { errorText as message, formatDateTime } from "./lib/format";
import type {
  Share,
  Advice,
  ExternalTest,
  Setup,
} from "./api/models/SharingPanel";
import { OreToggle } from "./OreToggle";
import { useEffect, useRef, useState } from "react";
import { Copy, Globe2, LoaderCircle, Square, ExternalLink } from "lucide-react";
import { api, openPlayit, type Server } from "./api";

export function SharingPanel({
  server,
  authenticated,
  whitelist,
  networkRevision,
  smart = false,
  onOpenWan,
}: {
  server: Server;
  authenticated: boolean;
  whitelist: boolean;
  networkRevision: number;
  smart?: boolean;
  onOpenWan?: () => void;
}) {
  const [share, setShare] = useState<Share>({ state: "idle", address: null });
  const [setup, setSetup] = useState<Setup | null>(null);
  const [consent, setConsent] = useState(false);
  const [recover, setRecover] = useState(true);
  const [externalConsent, setExternalConsent] = useState(false);
  const [allowFallback, setAllowFallback] = useState(false);
  const [externalPort, setExternalPort] = useState(server.port);
  const [wan, setWan] = useState("");
  const [advice, setAdvice] = useState<Advice | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [testConsent, setTestConsent] = useState(false);
  const [result, setResult] = useState<ExternalTest | null>(null);
  const [testing, setTesting] = useState(false);
  const mounted = useRef(true);
  const busy = useRef(false);
  const generation = useRef(0);
  const testRevision = useRef(0);
  const adviceRevision = useRef(0);
  const starting = useRef(false);
  const canConfigure = authenticated && whitelist;
  useEffect(() => {
    let closed = false;
    let timer: number;
    mounted.current = true;
    setShare({ state: "idle", address: null });
    setSetup(null);
    setConsent(false);
    setError("");
    setNotice("");
    const refresh = async () => {
      const request = generation.current;
      try {
        if (busy.current && !starting.current) return;
        const [next, config] = await Promise.all([
          api("share_status", { id: server.id }),
          api("playit_status", { id: server.id }),
        ]);
        if (!closed && request === generation.current) {
          setShare(next);
          setSetup(config);
        }
      } catch (e) {
        if (!closed && request === generation.current) setError(message(e));
      } finally {
        if (!closed) timer = window.setTimeout(() => void refresh(), 2000);
      }
    };
    void refresh();
    return () => {
      closed = true;
      mounted.current = false;
      ++generation.current;
      window.clearTimeout(timer);
    };
  }, [server.id, server.port]);
  useEffect(() => {
    ++adviceRevision.current;
    setAdvice(null);
  }, [
    server.state,
    server.port,
    server.localAddress,
    networkRevision,
    externalPort,
    wan,
  ]);
  useEffect(() => {
    ++testRevision.current;
    setResult(null);
    setTestConsent(false);
    setNotice("");
  }, [
    share.state,
    share.address,
    share.startedAt,
    server.state,
    server.port,
    server.localAddress,
    networkRevision,
  ]);
  const active = [
    "checking",
    "direct",
    "tunnel",
    "connecting",
    "recovering",
  ].includes(share.state);
  async function perform(
    action:
      | "share_start"
      | "share_stop"
      | "share_diagnose"
      | "playit_prepare"
      | "playit_confirm",
  ) {
    if (busy.current) return;
    busy.current = true;
    const request = ++generation.current;
    const diagnosticRevision = adviceRevision.current;
    starting.current = action === "share_start";
    setPending(true);
    setError("");
    setNotice("");
    try {
      const payload = {
        id: server.id,
        port: smart ? externalPort : server.port,
        consent:
          action === "share_diagnose" || (smart && action === "share_start")
            ? externalConsent
            : consent || !!setup?.ready,
        temporary: true,
        recover,
        wan,
        allowFallback,
        expectedNetworkRevision: advice?.networkRevision,
        expectedTarget: advice?.target,
      };
      const result =
        action === "share_diagnose"
          ? { kind: "advice" as const, value: await api(action, payload) }
          : action === "playit_prepare" || action === "playit_confirm"
            ? { kind: "setup" as const, value: await api(action, payload) }
            : {
                kind: "share" as const,
                value: await api(
                  smart && action === "share_start"
                    ? "share_assisted_start"
                    : action,
                  payload,
                ),
              };
      if (!mounted.current || request !== generation.current) return;
      ++generation.current;
      if (result.kind === "advice") {
        if (diagnosticRevision === adviceRevision.current)
          setAdvice(result.value);
      } else if (result.kind === "setup") setSetup(result.value);
      else setShare(result.value);
    } catch (e) {
      if (mounted.current && request === generation.current)
        setError(message(e));
    } finally {
      busy.current = false;
      starting.current = false;
      if (mounted.current) setPending(false);
    }
  }
  async function cancelCheck() {
    if (share.state !== "checking") return;
    ++generation.current;
    try {
      const next = await api("share_cancel_check", {
        id: server.id,
        expectedOperation: share.operation,
      });
      if (mounted.current) {
        setShare(next);
        setNotice(
          next.cancelled
            ? "Preparação cancelada. Uma resposta tardia não inicia um túnel."
            : "A preparação já terminou. Revê o estado da partilha abaixo.",
        );
      }
    } catch (e) {
      if (mounted.current) setError(message(e));
    }
  }
  async function test() {
    if (busy.current || !testConsent) return;
    busy.current = true;
    const request = ++generation.current;
    const sessionRevision = testRevision.current;
    setTesting(true);
    setError("");
    setResult(null);
    try {
      const next = await api("share_test", {
        id: server.id,
        consent: testConsent,
      });
      if (
        mounted.current &&
        request === generation.current &&
        sessionRevision === testRevision.current
      )
        setResult(next);
    } catch (e) {
      if (
        mounted.current &&
        request === generation.current &&
        sessionRevision === testRevision.current
      )
        setError(message(e));
    } finally {
      busy.current = false;
      if (mounted.current) setTesting(false);
    }
  }
  async function open(url: string) {
    try {
      await openPlayit(url);
    } catch (e) {
      setError(message(e));
    }
  }
  async function copy() {
    if (!share.address) return;
    try {
      await navigator.clipboard.writeText(share.address);
      setNotice("Endereço copiado.");
    } catch {
      setNotice(`Usa este endereço: ${share.address}`);
    }
  }
  return (
    <article className="connection-card internet-panel sharing-panel">
      <div className="players-heading">
        <h3>
          <Globe2 size={18} />{" "}
          {smart
            ? "Diagnosticar e escolher a ligação"
            : "Ligação para os teus amigos"}
        </h3>
        <span className="connection-tag">
          {share.state === "direct"
            ? "Acesso direto"
            : smart && !active
              ? "Assistido"
              : "playit.gg"}
        </span>
      </div>
      <p>
        {smart
          ? "O modo assistido testa o acesso direto existente e pode usar o playit como alternativa autorizada. Não configura o router nem muda o DNS."
          : "Liga os amigos a este PC sem configurar o router. Associa o playit uma vez; o endereço é reutilizado nas próximas sessões."}
      </p>
      {!active && (
        <>
          {smart && (
            <div className="internet-step">
              <h4>Diagnóstico antes de partilhar</h4>
              <p>
                Testa o acesso direto que já preparaste. A recomendação explica
                o resultado; a alternativa playit só é iniciada quando
                autorizada abaixo.
              </p>
              <label>
                Porta externa TCP
                <input
                  type="number"
                  min={1}
                  max={65535}
                  value={externalPort || ""}
                  disabled={pending}
                  onChange={(e) => setExternalPort(Number(e.target.value))}
                />
              </label>
              <label>
                IPv4 WAN mostrado no router (opcional)
                <input
                  value={wan}
                  disabled={pending}
                  placeholder="Serve apenas para comparar com o IP público"
                  onChange={(e) => setWan(e.target.value)}
                />
              </label>
              <label className="checkbox-row">
                <OreToggle
                  checked={externalConsent}
                  disabled={pending}
                  onChange={(e) => {
                    setExternalConsent(e.target.checked);
                    setAdvice(null);
                  }}
                />
                Autorizo consultar o IPv4 público no ipify e enviá-lo com esta
                porta ao mcsrvstat.us para testar o acesso direto.
              </label>
              <button
                type="button"
                className="button secondary sharing-diagnostic-action"
                disabled={
                  pending ||
                  !Number.isInteger(externalPort) ||
                  externalPort < 1 ||
                  externalPort > 65535
                }
                onClick={() => void perform("share_diagnose")}
              >
                {pending && <LoaderCircle size={16} className="spin" />}{" "}
                Diagnosticar ligação
              </button>
              {advice && (
                <div
                  className={`connection-result ${advice.recommendation === "direct" ? "success" : ""}`}
                  role="status"
                >
                  <strong>
                    {advice.recommendation === "direct"
                      ? "Recomendado: acesso direto"
                      : advice.recommendation === "playit"
                        ? "Recomendado: túnel playit"
                        : "Rever os requisitos"}
                  </strong>
                  <p>{advice.reason}</p>
                  {advice.directTest?.error && <p>{advice.directTest.error}</p>}
                  <small>
                    Diagnóstico: {formatDateTime(advice.checkedAt)}. O teste
                    externo pode estar em cache até cinco minutos.
                  </small>
                </div>
              )}
              <label className="checkbox-row">
                <OreToggle
                  checked={allowFallback}
                  disabled={pending}
                  onChange={(e) => setAllowFallback(e.target.checked)}
                />
                Se o acesso direto não for confirmado, autorizo iniciar o túnel
                playit já associado. Os amigos terão de usar o endereço playit;
                o nome DuckDNS mantém-se inalterado.
              </label>
              {onOpenWan && (
                <button
                  type="button"
                  className="button secondary sharing-diagnostic-action"
                  disabled={pending}
                  onClick={onOpenWan}
                >
                  Rever router e firewall WAN
                </button>
              )}
              {!externalConsent && (
                <p>
                  Sem autorização, o diagnóstico utiliza apenas informação
                  local. Para a partilha assistida, autoriza o teste ou escolhe
                  Túnel playit.
                </p>
              )}
              {allowFallback && !setup?.ready && (
                <p>
                  Prepara e associa o playit abaixo antes de usá-lo como
                  alternativa.
                </p>
              )}
            </div>
          )}
          {!setup?.ready && (!smart || allowFallback) && (
            <label className="checkbox-row">
              <OreToggle
                checked={consent}
                disabled={pending || !canConfigure}
                onChange={(e) => setConsent(e.target.checked)}
              />
              Aceito descarregar o agente oficial playit e associá-lo à minha
              conta. Ao partilhar, o tráfego de jogo passa pelo playit.
            </label>
          )}
          {(!smart || allowFallback) && (
            <div className="internet-step">
              <h4>1. Associar o playit</h4>
              {!setup?.associated && (
                <button
                  className="button secondary"
                  disabled={pending || !consent || !canConfigure}
                  onClick={() => void perform("playit_prepare")}
                >
                  {pending ? (
                    <LoaderCircle size={16} className="spin" />
                  ) : (
                    <Globe2 size={16} />
                  )}{" "}
                  Preparar playit
                </button>
              )}
              {setup?.claimUrl && (
                <>
                  <p>
                    Abre o site oficial, entra na tua conta e aprova o agente
                    Nethergate. Não envies credenciais pelo chat.
                  </p>
                  <button
                    className="button secondary"
                    disabled={pending}
                    onClick={() => void open(setup.claimUrl!)}
                  >
                    <ExternalLink size={16} /> Associar no site playit
                  </button>
                </>
              )}
              {(setup?.claimUrl || setup?.associated) && (
                <button
                  className="button secondary"
                  disabled={
                    pending || (!consent && !setup?.ready) || !canConfigure
                  }
                  onClick={() => void perform("playit_confirm")}
                >
                  {pending && <LoaderCircle size={16} className="spin" />}{" "}
                  {setup.associated
                    ? "Verificar configuração playit"
                    : "Já associei — verificar"}
                </button>
              )}
              {setup?.ready && (
                <p>
                  Endereço guardado: <strong>{setup.address}</strong>. Destino
                  local: {setup.target}.
                </p>
              )}
              <p>
                A conta está sujeita aos limites do fornecedor. Se mudar o IP
                local ou a porta, verifica novamente a configuração; a aplicação
                tenta preservar o mesmo túnel.
              </p>
            </div>
          )}
          <div className="internet-step">
            <h4>2. Iniciar a partilha</h4>
            <label className="checkbox-row">
              <OreToggle
                checked={recover}
                disabled={pending}
                onChange={(e) => setRecover(e.target.checked)}
              />
              Recuperar automaticamente o túnel se o agente falhar ou a rede
              mudar. Até três tentativas, com espera de 5, 15 e 45 segundos.
            </label>
            <button
              className="button primary"
              disabled={
                pending ||
                (smart
                  ? !externalConsent ||
                    !advice ||
                    advice.blocked ||
                    (advice.recommendation !== "direct" && !allowFallback) ||
                    (allowFallback && !setup?.ready)
                  : !setup?.ready) ||
                server.state !== "online" ||
                !authenticated ||
                !whitelist
              }
              onClick={() => void perform("share_start")}
            >
              {pending ? (
                <LoaderCircle size={16} className="spin" />
              ) : (
                <Globe2 size={16} />
              )}{" "}
              {smart
                ? "Testar e partilhar com esta autorização"
                : "Partilhar servidor"}
            </button>
            {server.state !== "online" && <p>Inicia primeiro o servidor.</p>}
          </div>
        </>
      )}
      {share.state === "checking" && (
        <div className="form-info" role="status">
          <LoaderCircle size={16} className="spin" /> A verificar a ligação
          antes de partilhar…
          <button
            type="button"
            className="button secondary"
            onClick={() => void cancelCheck()}
          >
            Cancelar preparação
          </button>
        </div>
      )}
      {share.fallbackUsed && (
        <p className="form-info">
          O acesso direto não foi confirmado. Foi escolhido o playit pela
          autorização desta partilha. Usa o endereço playit apresentado; o
          DuckDNS não foi alterado.
        </p>
      )}
      {share.state === "connecting" && (
        <p>
          <LoaderCircle size={16} className="spin" /> A ligar o agente playit…
        </p>
      )}
      {share.state === "recovering" && (
        <div className="form-info" role="status">
          <LoaderCircle size={16} className="spin" /> A recuperar a partilha ·
          tentativa {share.attempt}/{share.maximumAttempts || 3}.
          {share.remainingSeconds != null
            ? ` Nova tentativa em ${share.remainingSeconds} s.`
            : " A verificar a configuração…"}
          <p>
            O endereço atribuído é preservado. Podes cancelar a recuperação em
            Terminar túnel.
          </p>
        </div>
      )}
      {["ended", "error"].includes(share.state) && (
        <p className="inline-error" role="alert">
          A partilha foi interrompida. Verifica a ligação à internet e volta a
          partilhar; o endereço playit guardado será reutilizado.
        </p>
      )}
      {share.address && (
        <div className="connection-address">
          <code>{share.address}</code>
          <button
            className="button secondary button-copy"
            aria-label="Copiar endereço"
            title="Copiar endereço"
            onClick={() => void copy()}
          >
            <Copy size={16} />
          </button>
        </div>
      )}
      {share.detail && <p role="status">{share.detail}</p>}
      {share.state === "tunnel" && (
        <p>
          O túnel termina ao parar este servidor ou terminar o motor. Fechar só
          o painel mantém a partilha. O endereço atribuído fica guardado para a
          próxima sessão. A latência depende da rede playit e da tua ligação.
          {share.recoveryEnabled
            ? " Recuperação automática ativa nesta partilha."
            : " Recuperação automática desativada nesta partilha."}
        </p>
      )}
      {["tunnel", "direct"].includes(share.state) && share.address && (
        <div className="internet-step">
          <h4>
            {testing
              ? "A verificar pela internet…"
              : result?.state === "reported_online"
                ? "O Minecraft respondeu pela internet"
                : result
                  ? "Não foi possível confirmar a ligação"
                  : "Ligação ativa · falta confirmar a resposta Minecraft"}
          </h4>
          <label className="checkbox-row">
            <OreToggle
              checked={testConsent}
              disabled={testing || pending}
              onChange={(e) => setTestConsent(e.target.checked)}
            />
            Autorizo enviar {share.address} ao mcsrvstat.us para verificar a
            resposta Minecraft.
          </label>
          <button
            className="button primary"
            disabled={
              testing || pending || !testConsent || server.state !== "online"
            }
            onClick={() => void test()}
          >
            {testing ? (
              <LoaderCircle size={16} className="spin" />
            ) : (
              <Globe2 size={16} />
            )}
            {result ? "Verificar novamente" : "Verificar pela internet"}
          </button>
          {result && (
            <div
              className={`connection-result ${result.state === "reported_online" ? "success" : ""}`}
              role="status"
            >
              <strong>
                {result.state === "reported_online"
                  ? "Resposta Minecraft confirmada pelo serviço externo"
                  : result.state === "reported_offline"
                    ? "O serviço externo não recebeu resposta Minecraft"
                    : "A resposta externa foi inconclusiva"}
              </strong>
              {result.version && <p>{result.version}</p>}
              <p>
                {result.state !== "reported_online"
                  ? "Confirma que o servidor e a partilha continuam ligados. O túnel pode demorar a ficar disponível; tenta entrar pelo Minecraft de outra rede."
                  : "Já podes enviar o endereço aos teus amigos. A entrada no mundo ainda precisa de ser testada com uma conta autorizada."}
              </p>
              <small>
                {result.observedAt
                  ? `Observação: ${formatDateTime(result.observedAt * 1000)}. `
                  : `Consulta: ${formatDateTime(result.checkedAt)}. `}
                O resultado pode estar em cache até cinco minutos. Repetir nesse
                intervalo reutiliza a resposta.
              </small>
            </div>
          )}
          <p>
            No Minecraft Java {server.version}: Multijogador → Ligação direta →
            cola o endereço acima. Usa um nome incluído na whitelist e testa a
            partir de outra rede.
          </p>
          {server.software === "fabric" && (
            <p>
              Os amigos precisam do Fabric e dos mods correspondentes. Exporta o
              pacote para jogadores no separador Mods.
            </p>
          )}
        </div>
      )}
      {share.state === "direct" && (
        <p>
          A regra do router e a firewall mantêm-se configuradas. Terminar esta
          partilha guarda o mundo e para o servidor.
        </p>
      )}
      {active && (
        <button
          className="button secondary"
          disabled={pending || testing}
          onClick={() => void perform("share_stop")}
        >
          <Square size={16} />
          {share.state === "direct"
            ? "Parar servidor e terminar partilha"
            : "Terminar túnel"}
        </button>
      )}
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
      {setup?.error && !error && (
        <p className="inline-error" role="alert">
          {setup.error}
        </p>
      )}
      {notice && <p role="status">{notice}</p>}
    </article>
  );
}
