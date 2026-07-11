export function ComingSoonPanel({ icon, title, pitch }: { icon: string; title: string; pitch: string }) {
  return (
    <div className="panel-body">
      <div className="inspector-empty">
        <div className="icon">{icon}</div>
        <p>
          <strong>{title}</strong>
          <br />
          {pitch}
          <br />
          <span style={{ opacity: 0.7 }}>Planned for Phase 3 — plugins &amp; asset packs.</span>
        </p>
      </div>
    </div>
  );
}
