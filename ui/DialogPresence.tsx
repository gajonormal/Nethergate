import {
  createContext,
  useContext,
  useLayoutEffect,
  useRef,
  useState,
  type HTMLAttributes,
  type ReactNode,
} from "react";

const DialogClosing = createContext(false);

/** Retain the actual React tree briefly so portals and dialog state can exit. */
export function DialogPresence({ children }: { children: ReactNode }) {
  const open =
    children !== null && children !== undefined && children !== false;
  const last = useRef(children);
  const [present, setPresent] = useState(open);
  if (open) last.current = children;

  useLayoutEffect(() => {
    if (open) {
      setPresent(true);
      return;
    }
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      setPresent(false);
      return;
    }
    const timer = window.setTimeout(() => setPresent(false), 140);
    return () => window.clearTimeout(timer);
  }, [open]);

  return (
    <DialogClosing.Provider value={!open}>
      {open ? children : present ? last.current : null}
    </DialogClosing.Provider>
  );
}

export function DialogBackdrop(props: HTMLAttributes<HTMLDivElement>) {
  const closing = useContext(DialogClosing);
  return (
    <div
      {...props}
      data-closing={closing || undefined}
      onClickCapture={(event) => {
        if (closing) {
          event.preventDefault();
          event.stopPropagation();
        } else props.onClickCapture?.(event);
      }}
      onKeyDownCapture={(event) => {
        if (closing) {
          event.preventDefault();
          event.stopPropagation();
        } else props.onKeyDownCapture?.(event);
      }}
    />
  );
}
