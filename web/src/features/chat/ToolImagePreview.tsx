import { X } from "lucide-react";
import { useRef, useState } from "react";
import type { ToolImage } from "../../api/types";

export function ToolImagePreview({ image }: { image: ToolImage["image"] }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [viewing, setViewing] = useState(false);
  // Only display the supported raster formats, including for imported snapshots.
  if (!["image/png", "image/jpeg", "image/webp"].includes(image.mimetype)) {
    return <span>Unsupported image: {image.filename}</span>;
  }
  const src = `data:${image.mimetype};base64,${image.data}`;
  return (
    <div className="tool-image">
      <button
        className="tool-image-preview"
        type="button"
        aria-label={`View image: ${image.filename}`}
        onClick={() => {
          setViewing(true);
          dialog.current?.showModal();
        }}
      >
        <img src={src} alt={image.filename} width={image.width} height={image.height} loading="lazy" />
      </button>
      <span>{image.filename} ({image.width} × {image.height})</span>
      <dialog
        className="tool-image-dialog"
        ref={dialog}
        aria-label={image.filename}
        onClose={() => setViewing(false)}
        onClick={(event) => { if (event.target === event.currentTarget) dialog.current?.close(); }}
      >
        <button className="tool-image-close" type="button" aria-label="Close image" title="Close image" onClick={() => dialog.current?.close()}>
          <X size={20} aria-hidden="true" />
        </button>
        {viewing ? <img src={src} alt={image.filename} width={image.width} height={image.height} /> : null}
      </dialog>
    </div>
  );
}
