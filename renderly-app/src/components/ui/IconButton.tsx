import type { ButtonHTMLAttributes, ReactNode } from "react";
import type { LucideIcon } from "lucide-react";
import { Tooltip } from "./Tooltip";

type Props = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "title"> & {
  icon?: LucideIcon;
  label?: ReactNode;
  /** Icon-only square control (toolbars / titlebar). */
  iconOnly?: boolean;
  size?: "sm" | "md";
  variant?: "default" | "primary" | "ghost" | "danger";
  /** Custom tooltip (preferred over native `title`). */
  tooltip?: ReactNode;
};

export function IconButton({
  icon: Icon,
  label,
  iconOnly = false,
  size = "md",
  variant = "default",
  tooltip,
  className = "",
  children,
  ...rest
}: Props) {
  const classes = [
    iconOnly ? "btn-icon-only" : "btn",
    variant === "primary" ? "btn-primary" : "",
    variant === "ghost" ? "btn-ghost" : "",
    variant === "danger" ? "btn-danger" : "",
    size === "sm" ? "btn-sm" : "",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  const button = (
    <button type="button" className={classes} {...rest}>
      {Icon ? <Icon className="btn-lucide" size={size === "sm" ? 14 : 16} strokeWidth={1.75} /> : null}
      {!iconOnly && label != null ? <span>{label}</span> : null}
      {children}
    </button>
  );

  if (tooltip == null || tooltip === "") return button;
  return <Tooltip content={tooltip}>{button}</Tooltip>;
}
