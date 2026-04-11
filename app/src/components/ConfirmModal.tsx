import { Show, createEffect, onCleanup } from "solid-js";
import { AlertTriangle } from "lucide-solid";

interface ConfirmModalProps {
  open: boolean;
  title: string;
  description: string;
  confirmText?: string;
  cancelText?: string;
  destructive?: boolean;
  busy?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export default function ConfirmModal(props: ConfirmModalProps) {
  const handleEsc = (event: KeyboardEvent) => {
    if (event.key === "Escape" && props.open && !props.busy) {
      props.onCancel();
    }
  };

  createEffect(() => {
    if (!props.open) return;
    document.addEventListener("keydown", handleEsc);
    onCleanup(() => document.removeEventListener("keydown", handleEsc));
  });

  return (
    <Show when={props.open}>
      <div
        class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/45"
        role="presentation"
        onMouseDown={(event) => {
          if (event.target === event.currentTarget && !props.busy) props.onCancel();
        }}
      >
        <div
          class="w-full max-w-md rounded-xl border border-mk-separator bg-mk-grouped-bg p-4 sm:p-5 shadow-2xl"
          role="dialog"
          aria-modal="true"
          aria-label={props.title}
        >
          <div class="flex items-start gap-3">
            <div class={`mt-0.5 rounded-lg p-2 ${props.destructive ? "bg-mk-pink/15 text-mk-pink" : "bg-mk-cyan/15 text-mk-cyan"}`}>
              <AlertTriangle class="w-4 h-4" />
            </div>
            <div class="min-w-0">
              <h3 class="text-[15px] font-semibold text-mk-text">{props.title}</h3>
              <p class="mt-1 text-[12px] text-mk-secondary leading-relaxed">{props.description}</p>
            </div>
          </div>

          <div class="mt-4 flex items-center justify-end gap-2">
            <button
              class="px-3 py-1.5 rounded-md text-[12px] border border-mk-separator text-mk-secondary hover:text-mk-text hover:bg-mk-fill transition-colors"
              onClick={props.onCancel}
              disabled={props.busy}
            >
              {props.cancelText ?? "Cancel"}
            </button>
            <button
              class={`px-3 py-1.5 rounded-md text-[12px] font-semibold transition-colors ${
                props.destructive
                  ? "bg-mk-pink text-white hover:opacity-90"
                  : "bg-mk-green text-mk-sidebar hover:bg-mk-green-hover"
              }`}
              onClick={props.onConfirm}
              disabled={props.busy}
            >
              {props.busy ? "Working..." : (props.confirmText ?? "Confirm")}
            </button>
          </div>
        </div>
      </div>
    </Show>
  );
}
