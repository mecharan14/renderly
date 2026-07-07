/** Small UI helpers — toasts, timecode, buttons. */

export function formatTimecode(secs: number): string {
  const m = Math.floor(secs / 60);
  const s = Math.floor(secs % 60);
  const f = Math.floor((secs % 1) * 100);
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${String(f).padStart(2, "0")}`;
}

export function fileName(path: string): string {
  return path.split(/[/\\]/).pop() ?? path;
}

let toastRoot: HTMLElement | null = null;

function ensureToastRoot(): HTMLElement {
  if (!toastRoot) {
    toastRoot = document.createElement("div");
    toastRoot.id = "toasts";
    toastRoot.className = "toasts";
    document.body.appendChild(toastRoot);
  }
  return toastRoot;
}

export function toast(message: string, kind: "info" | "success" | "error" = "info") {
  const root = ensureToastRoot();
  const node = document.createElement("div");
  node.className = `toast toast-${kind}`;
  node.textContent = message;
  root.appendChild(node);
  requestAnimationFrame(() => node.classList.add("show"));
  setTimeout(() => {
    node.classList.remove("show");
    setTimeout(() => node.remove(), 300);
  }, 3200);
}

export function btn(
  label: string,
  opts?: { className?: string; title?: string; icon?: string; primary?: boolean },
): HTMLButtonElement {
  const b = document.createElement("button");
  b.type = "button";
  b.className = [
    opts?.primary ? "btn-primary" : "btn",
    opts?.className ?? "",
  ]
    .filter(Boolean)
    .join(" ");
  if (opts?.title) b.title = opts.title;
  if (opts?.icon) {
    const ic = document.createElement("span");
    ic.className = "btn-icon";
    ic.textContent = opts.icon;
    b.appendChild(ic);
  }
  const text = document.createElement("span");
  text.textContent = label;
  b.appendChild(text);
  return b;
}
