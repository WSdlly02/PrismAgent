import type { ReactNode } from "react";
import { useEffect, useRef } from "react";

export type ConfirmDialogProps = {
  open: boolean;
  title: string;
  children: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  onConfirm: () => void | Promise<void>;
  onCancel: () => void;
};

export function ConfirmDialog({
  open,
  title,
  children,
  confirmLabel = "Delete",
  cancelLabel = "Cancel",
  danger = true,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const overlayRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        onCancel();
      }
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [open, onCancel]);

  if (!open) return null;

  async function handleConfirm() {
    await onConfirm();
  }

  function handleOverlayClick(e: React.MouseEvent) {
    if (e.target === overlayRef.current) {
      onCancel();
    }
  }

  return (
    <div
      className="confirm-dialog-overlay"
      ref={overlayRef}
      onClick={handleOverlayClick}
    >
      <div className={`confirm-dialog${danger ? " confirm-dialog-danger" : ""}`}>
        <h3 className="confirm-dialog-title">{title}</h3>
        <div className="confirm-dialog-body">{children}</div>
        <div className="confirm-dialog-actions">
          <button
            className="secondary-button"
            onClick={onCancel}
            type="button"
          >
            {cancelLabel}
          </button>
          <button
            className={`confirm-button${danger ? " confirm-button-danger" : " confirm-button-primary"}`}
            onClick={handleConfirm}
            type="button"
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
