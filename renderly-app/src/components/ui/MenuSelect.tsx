import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { ChevronDown } from "lucide-react";
import { Tooltip } from "./Tooltip";

export type MenuSelectOption = {
  value: string;
  label: string;
  disabled?: boolean;
};

type Props = {
  value: string;
  options: MenuSelectOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  tooltip?: string;
  className?: string;
  /** Compact trigger for dense toolbars (e.g. transport). */
  compact?: boolean;
  /** Custom trigger label; defaults to the selected option's label. */
  displayLabel?: ReactNode;
};

/** Styled popover select — replaces native `<select>` so chrome never looks OS-native. */
export function MenuSelect({
  value,
  options,
  onChange,
  disabled = false,
  tooltip,
  className = "",
  compact = false,
  displayLabel,
}: Props) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const listId = useId();
  const selected = options.find((o) => o.value === value);
  const label = displayLabel ?? selected?.label ?? value;

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const control = (
    <div
      className={`menu-select${compact ? " compact" : ""}${open ? " open" : ""}${className ? ` ${className}` : ""}`}
      ref={rootRef}
    >
      <button
        type="button"
        className="menu-select-trigger"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listId}
        onClick={() => setOpen((v) => !v)}
      >
        <span className="menu-select-label">{label}</span>
        <ChevronDown size={12} strokeWidth={2} className="menu-select-chevron" />
      </button>
      {open && (
        <ul id={listId} className="menu-select-list" role="listbox">
          {options.map((opt) => (
            <li key={opt.value} role="presentation">
              <button
                type="button"
                role="option"
                aria-selected={opt.value === value}
                disabled={opt.disabled}
                className={`menu-select-option${opt.value === value ? " selected" : ""}`}
                onClick={() => {
                  if (opt.disabled) return;
                  setOpen(false);
                  if (opt.value !== value) onChange(opt.value);
                }}
              >
                {opt.label}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );

  if (!tooltip || open) return control;
  return (
    <Tooltip content={tooltip} disabled={disabled}>
      {control}
    </Tooltip>
  );
}
