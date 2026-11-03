import { getCurrentWindow } from "@tauri-apps/api/window";
import { GripHorizontal, Minus, Maximize2, X } from "lucide-react";
import { desktop } from "./api";

const edges = [
  "North",
  "South",
  "East",
  "West",
  "NorthEast",
  "NorthWest",
  "SouthEast",
  "SouthWest",
] as const;

export function WindowChrome({
  onError,
}: {
  onError: (message: string) => void;
}) {
  if (!desktop) return null;
  function run(action: () => Promise<void>) {
    void action().catch((error: unknown) =>
      onError(error instanceof Error ? error.message : String(error)),
    );
  }
  const window = getCurrentWindow();
  return (
    <>
      <div className="skin-window-toolbar" aria-label="Controlos da janela">
        <button
          type="button"
          className="skin-drag-handle"
          aria-label="Arrastar janela"
          title="Arrastar janela · duplo clique para maximizar ou restaurar"
          onMouseDown={(event) => {
            if (event.button !== 0) return;
            if (event.detail === 2) run(() => window.toggleMaximize());
            else run(() => window.startDragging());
          }}
        >
          <GripHorizontal size={18} />
        </button>
        <button
          type="button"
          aria-label="Minimizar"
          title="Minimizar"
          onClick={() => run(() => window.minimize())}
        >
          <Minus size={16} />
        </button>
        <button
          type="button"
          aria-label="Maximizar ou restaurar"
          title="Maximizar ou restaurar"
          onClick={() => run(() => window.toggleMaximize())}
        >
          <Maximize2 size={14} />
        </button>
        <button
          type="button"
          className="skin-close"
          aria-label="Fechar aplicação"
          title="Fechar aplicação"
          onClick={() => run(() => window.close())}
        >
          <X size={17} />
        </button>
      </div>
      {edges.map((edge) => (
        <div
          key={edge}
          className={`skin-resize skin-resize-${edge}`}
          aria-hidden="true"
          onMouseDown={(event) => {
            if (event.button !== 0) return;
            event.preventDefault();
            run(() => window.startResizeDragging(edge));
          }}
        />
      ))}
    </>
  );
}
