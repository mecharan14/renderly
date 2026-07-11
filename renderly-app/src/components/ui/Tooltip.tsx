import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

type Side = "top" | "bottom" | "left" | "right";

type Props = {
  content: ReactNode;
  children: ReactNode;
  side?: Side;
  /** Hover/focus delay before showing (ms). */
  delay?: number;
  disabled?: boolean;
  className?: string;
};

/**
 * Custom tooltip — never uses the native HTML `title` attribute (slow, unstyled).
 */
export function Tooltip({
  content,
  children,
  side = "bottom",
  delay = 350,
  disabled = false,
  className = "",
}: Props) {
  const [visible, setVisible] = useState(false);
  const [coords, setCoords] = useState<{ top: number; left: number } | null>(null);
  const wrapRef = useRef<HTMLSpanElement | null>(null);
  const timerRef = useRef<number | null>(null);
  const tipId = useId();

  function clearTimer() {
    if (timerRef.current != null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }

  function hide() {
    clearTimer();
    setVisible(false);
  }

  function show() {
    if (disabled || content == null || content === "") return;
    clearTimer();
    timerRef.current = window.setTimeout(() => {
      const el = wrapRef.current;
      if (!el) return;
      const r = el.getBoundingClientRect();
      const gap = 6;
      let top = r.bottom + gap;
      let left = r.left + r.width / 2;
      if (side === "top") top = r.top - gap;
      if (side === "left") {
        top = r.top + r.height / 2;
        left = r.left - gap;
      }
      if (side === "right") {
        top = r.top + r.height / 2;
        left = r.right + gap;
      }
      setCoords({ top, left });
      setVisible(true);
    }, delay);
  }

  useEffect(() => () => clearTimer(), []);

  useEffect(() => {
    if (!visible) return;
    const onScroll = () => hide();
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onScroll);
    return () => {
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onScroll);
    };
  }, [visible]);

  return (
    <span
      ref={wrapRef}
      className={`uc-tooltip-wrap${className ? ` ${className}` : ""}`}
      onMouseEnter={show}
      onMouseLeave={hide}
      onFocus={show}
      onBlur={hide}
    >
      {children}
      {visible &&
        coords &&
        createPortal(
          <div
            id={tipId}
            role="tooltip"
            className={`uc-tooltip uc-tooltip-${side}`}
            style={{ top: coords.top, left: coords.left }}
          >
            {content}
          </div>,
          document.body,
        )}
    </span>
  );
}
