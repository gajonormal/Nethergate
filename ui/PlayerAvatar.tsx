import { useState } from "react";

/** UUIDs keep the skin stable when a player changes their Minecraft name. */
export function PlayerAvatar({ name, uuid }: { name: string; uuid?: string }) {
  const normalizedUuid = uuid?.replaceAll("-", "");
  const identity =
    normalizedUuid && /^[a-f0-9]{32}$/i.test(normalizedUuid)
      ? normalizedUuid
      : /^[A-Za-z0-9_]{3,16}$/.test(name)
        ? name
        : null;
  const source = identity
    ? `https://minotar.net/helm/${identity}/40.png`
    : null;
  const [failedSource, setFailedSource] = useState<string | null>(null);

  return (
    <span className="player-avatar" aria-hidden="true">
      <svg
        className="player-avatar-fallback"
        viewBox="0 0 8 8"
        shapeRendering="crispEdges"
      >
        <path fill="#51443c" d="M0 0h8v8H0z" />
        <path fill="#b88c6c" d="M0 3h8v5H0z" />
        <path fill="#ece4d4" d="M1 4h2v1H1zm4 0h2v1H5z" />
        <path fill="#45415f" d="M2 4h1v1H2zm3 0h1v1H5z" />
        <path fill="#78503d" d="M3 5h2v1H3zM2 7V6h1v1h2V6h1v2H2z" />
      </svg>
      {source && source !== failedSource && (
        <img
          key={source}
          src={source}
          alt=""
          width={40}
          height={40}
          loading="lazy"
          decoding="async"
          referrerPolicy="no-referrer"
          onError={() => setFailedSource(source)}
        />
      )}
    </span>
  );
}
