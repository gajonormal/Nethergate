import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { Settings2 } from "lucide-react";

export function ApplicationMenu({ children }: { children: ReactNode }) {
  const [expanded, setExpanded] = useState(false);
  const [mounted, setMounted] = useState(false);
  const reveal = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const animation = useRef<Animation | null>(null);

  useLayoutEffect(() => {
    const element = reveal.current;
    if (!element || !content.current) return;
    // Read the interpolated height before cancelling, so reversals never jump.
    const from = element.getBoundingClientRect().height;
    animation.current?.cancel();
    animation.current = null;
    const to = expanded ? content.current.getBoundingClientRect().height : 0;
    element.style.height = `${from}px`;
    if (expanded) setMounted(true);
    const finish = () => {
      element.style.height = expanded ? "auto" : "0px";
      setMounted(expanded);
    };
    if (
      Math.abs(to - from) < 1 ||
      window.matchMedia("(prefers-reduced-motion: reduce)").matches
    ) {
      finish();
      return;
    }
    const running = element.animate(
      [{ height: `${from}px` }, { height: `${to}px` }],
      { duration: 160, easing: "ease-out", fill: "forwards" },
    );
    animation.current = running;
    running.onfinish = () => {
      if (animation.current !== running) return;
      finish();
      running.cancel();
      animation.current = null;
    };
  }, [expanded]);

  useEffect(() => () => animation.current?.cancel(), []);

  return (
    <details
      className="app-options"
      open={expanded || mounted}
      data-closing={(!expanded && mounted) || undefined}
    >
      <summary
        aria-label="Opções da aplicação"
        title="Opções da aplicação"
        onClick={(event) => {
          event.preventDefault();
          setExpanded((value) => !value);
        }}
      >
        <Settings2 size={18} />
        <span>Aplicação</span>
      </summary>
      <div className="app-options-reveal" ref={reveal}>
        <div
          className="app-options-content"
          ref={content}
          inert={!expanded || undefined}
        >
          {children}
        </div>
      </div>
    </details>
  );
}
