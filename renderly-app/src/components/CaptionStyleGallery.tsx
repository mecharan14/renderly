import { CAPTION_STYLES, CAPTION_STYLE_META } from "../lib/types";

export function CaptionStyleGallery({
  value,
  onChange,
}: {
  value: string;
  onChange: (styleId: string) => void;
}) {
  return (
    <div className="style-gallery" role="listbox" aria-label="Caption styles">
      {CAPTION_STYLES.map((id) => {
        const meta = CAPTION_STYLE_META[id];
        return (
          <button
            key={id}
            type="button"
            role="option"
            aria-selected={value === id}
            className={`style-card style-${id}${value === id ? " selected" : ""}`}
            onClick={() => onChange(id)}
          >
            <span className="style-preview">{meta.preview}</span>
            <span className="style-label">{meta.label}</span>
          </button>
        );
      })}
    </div>
  );
}
