import { useEditorStore } from "../store/editorStore";

export function Toasts() {
  const toasts = useEditorStore((s) => s.toasts);
  const dismissToast = useEditorStore((s) => s.dismissToast);

  return (
    <div className="toasts">
      {toasts.map((t) => (
        <div
          key={t.id}
          className={`toast toast-${t.kind} show`}
          onClick={() => dismissToast(t.id)}
        >
          {t.message}
        </div>
      ))}
    </div>
  );
}
