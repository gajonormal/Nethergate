import { useRef, useState } from "react";
import { api } from "./api";
import { usePolling } from "./hooks/usePolling";

/** The original Minecraft world thumbnail is read without blocking the library. */
export function WorldPreview({ id }: { id: string }) {
  const [image, setImage] = useState<string | null>(null);
  const [generating, setGenerating] = useState(false);
  const revision = useRef<string | undefined>(undefined);
  usePolling(() => api("world_preview", { id, revision: revision.current }), {
    interval: generating ? 1500 : 30_000,
    dependencies: [id],
    onData: (preview) => {
      setGenerating(preview.generating);
      if (preview.image) setImage(preview.image);
      else if (!preview.revision) setImage(null);
      revision.current = preview.revision ?? undefined;
    },
    onError: () => setGenerating(false),
  });
  return image ? (
    <img
      className="world-preview"
      src={image}
      alt="Miniatura do mundo capturada no Minecraft"
      draggable={false}
      onError={() => {
        setImage(null);
        revision.current = undefined;
      }}
    />
  ) : null;
}
