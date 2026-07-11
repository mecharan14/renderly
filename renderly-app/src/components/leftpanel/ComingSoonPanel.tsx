import type { LucideIcon } from "lucide-react";

export function ComingSoonPanel({
  icon: Icon,
  title,
  pitch,
}: {
  icon: LucideIcon;
  title: string;
  pitch: string;
}) {
  return (
    <div className="panel-body coming-soon">
      <div className="coming-soon-card">
        <div className="coming-soon-icon" aria-hidden>
          <Icon size={22} strokeWidth={1.5} />
        </div>
        <h3>{title}</h3>
        <p className="coming-soon-pitch">{pitch}</p>
        <p className="coming-soon-badge">Coming in Phase 3</p>
      </div>
    </div>
  );
}
