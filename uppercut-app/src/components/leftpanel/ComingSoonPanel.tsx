export function ComingSoonPanel({
  icon,
  title,
  pitch,
}: {
  icon: string;
  title: string;
  pitch: string;
}) {
  return (
    <div className="panel-body coming-soon">
      <div className="coming-soon-card">
        <div className="coming-soon-icon" aria-hidden>
          {icon}
        </div>
        <h3>{title}</h3>
        <p className="coming-soon-pitch">{pitch}</p>
        <p className="coming-soon-badge">Phase 3 — plugins &amp; asset packs</p>
        <p className="coming-soon-note">
          These tools stay stubbed until the WASM plugin SDK and declarative asset packs land.
          Media, Audio, and Text are fully usable today.
        </p>
      </div>
    </div>
  );
}
