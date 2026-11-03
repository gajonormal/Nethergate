import { useId, type CSSProperties } from "react";

const MIN = 512;
const MAX = 65536;
const marks = [512, 1024, 2048, 4096, 8192, 16384, 32768, 65536];
const gb = (mb: number) =>
  (mb / 1024).toLocaleString("pt-PT", { maximumFractionDigits: 2 });

export const memoryPosition = (mb: number) =>
  Math.log2(Math.min(MAX, Math.max(MIN, mb)) / MIN);
export const memoryAtPosition = (position: number) =>
  Math.min(MAX, Math.max(MIN, Math.round((MIN * 2 ** position) / 256) * 256));

export function MemoryControl({
  value,
  onChange,
}: {
  value: number;
  onChange: (mb: number) => void;
}) {
  const id = useId();
  return (
    <div className="memory-control">
      <div className="memory-heading">
        <label htmlFor={`${id}-range`}>Memória máxima</label>
        <output htmlFor={`${id}-range ${id}-exact`}>{gb(value)} GB</output>
      </div>
      <input
        id={`${id}-range`}
        className="memory-slider"
        type="range"
        min={0}
        max={7}
        step="any"
        value={memoryPosition(value)}
        style={
          {
            "--memory-fill": `${(memoryPosition(value) / 7) * 100}%`,
          } as CSSProperties
        }
        aria-valuetext={`${gb(value)} GB`}
        aria-describedby={`${id}-hint`}
        onChange={(e) => onChange(memoryAtPosition(Number(e.target.value)))}
        onKeyDown={(e) => {
          const steps: Record<string, number> = {
            ArrowLeft: -256,
            ArrowDown: -256,
            ArrowRight: 256,
            ArrowUp: 256,
            PageDown: -1024,
            PageUp: 1024,
          };
          if (e.key === "Home" || e.key === "End" || e.key in steps) {
            e.preventDefault();
            onChange(
              e.key === "Home"
                ? MIN
                : e.key === "End"
                  ? MAX
                  : Math.min(MAX, Math.max(MIN, value + steps[e.key])),
            );
          }
        }}
      />
      <div className="memory-marks" aria-hidden="true">
        {marks.map((mb, index) => (
          <span key={mb} style={{ left: `${(index / 7) * 100}%` }}>
            {gb(mb)}
          </span>
        ))}
      </div>
      <label className="memory-exact" htmlFor={`${id}-exact`}>
        Valor exato (MB)
        <input
          id={`${id}-exact`}
          type="number"
          required
          min={MIN}
          max={MAX}
          step={256}
          value={value || ""}
          onChange={(e) => onChange(e.currentTarget.valueAsNumber || 0)}
        />
      </label>
      <small id={`${id}-hint`}>
        Marcações em GB. Deixa memória livre para o jogo e o sistema.
      </small>
    </div>
  );
}
