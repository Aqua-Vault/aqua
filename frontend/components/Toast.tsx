import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import type { TxLifecycleEvent, TxOp, TxState } from "../lib/contract";
import { setTxLifecycleHandler } from "../lib/contract";
import { explorerTxUrl } from "../lib/config";
import { shortenHash } from "../lib/format";

// Central toast stack for the Soroban transaction lifecycle. The provider
// registers itself as the `lib/contract.ts` lifecycle handler, so every write
// op (deposit/withdraw/draw) reports through the exact same pipeline:
// submitting → signing → submitted → confirmed (auto-dismiss after 8s), with
// `failed` persisting until manually dismissed.

interface ToastItem {
  id: number;
  op: TxOp;
  state: TxState;
  txHash?: string;
  message?: string;
}

interface ToastContextValue {
  push: (event: TxLifecycleEvent) => void;
}

const ToastContext = createContext<ToastContextValue | null>(null);

const OP_LABEL: Record<TxOp, string> = {
  deposit: "Deposit",
  withdraw: "Withdraw",
  draw: "Prize draw",
};

const STATE_LABEL: Record<TxState, string> = {
  submitting: "Submitting",
  signing: "Signing",
  submitted: "Broadcast",
  confirmed: "Confirmed",
  failed: "Failed",
};

const STATE_ICON: Record<TxState, string> = {
  submitting: "⏳",
  signing: "✍️",
  submitted: "📡",
  confirmed: "✅",
  failed: "❌",
};

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const toastsRef = useRef<ToastItem[]>([]);
  const idRef = useRef(0);
  const timersRef = useRef<Record<number, ReturnType<typeof setTimeout>>>({});

  const dismiss = useCallback((id: number) => {
    toastsRef.current = toastsRef.current.filter((t) => t.id !== id);
    setToasts(toastsRef.current);
    const timer = timersRef.current[id];
    if (timer) {
      clearTimeout(timer);
      delete timersRef.current[id];
    }
  }, []);

  // Merge lifecycle events: a toast for an op is updated in place (never
  // stacked) across the submit → confirm stages of the same operation.
  const push = useCallback(
    (event: TxLifecycleEvent) => {
      const prev = toastsRef.current;
      const existingIdx = prev.findIndex((t) => t.op === event.op);
      const next: ToastItem = {
        id: existingIdx >= 0 ? prev[existingIdx].id : ++idRef.current,
        op: event.op,
        state: event.state,
        txHash: event.txHash,
        message: event.message,
      };
      const merged =
        existingIdx >= 0
          ? prev.map((t, i) => (i === existingIdx ? next : t))
          : [...prev, next];
      toastsRef.current = merged;
      setToasts(merged);

      // Auto-dismiss confirmed toasts after 8s; failed stays until dismissed.
      const timer = timersRef.current[next.id];
      if (timer) clearTimeout(timer);
      if (event.state === "confirmed") {
        timersRef.current[next.id] = setTimeout(() => dismiss(next.id), 8000);
      }
    },
    [dismiss],
  );

  // Bind the framework-free lifecycle emitter in lib/contract.ts.
  useEffect(() => {
    setTxLifecycleHandler(push);
    return () => {
      setTxLifecycleHandler(null);
      Object.values(timersRef.current).forEach(clearTimeout);
      timersRef.current = {};
    };
  }, [push]);

  return (
    <ToastContext.Provider value={{ push }}>
      {children}
      {/* Fixed top-right stack, announced to screen readers. */}
      <div
        className="pointer-events-none fixed right-4 top-4 z-50 flex w-[min(92vw,360px)] flex-col gap-2"
        role="status"
        aria-live="polite"
        aria-atomic="true"
      >
        {toasts.map((t) => (
          <div
            key={t.id}
            className="pointer-events-auto rounded-xl border border-white/10 bg-ink-900/95 p-4 shadow-xl backdrop-blur"
          >
            <div className="flex items-start justify-between gap-3">
              <div className="flex items-start gap-2.5">
                <span className="text-lg" aria-hidden="true">
                  {STATE_ICON[t.state]}
                </span>
                <div className="min-w-0">
                  <div className="text-sm font-semibold text-white">
                    {STATE_LABEL[t.state]} · {OP_LABEL[t.op]}
                  </div>
                  {t.message && (
                    <div className="mt-0.5 text-xs text-slate-400">
                      {t.message}
                    </div>
                  )}
                  {t.state === "confirmed" && t.txHash && (
                    <a
                      href={explorerTxUrl(t.txHash)}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="mt-1 inline-block text-xs text-aqua-300 hover:text-aqua-200"
                    >
                      View on Stellar Expert · {shortenHash(t.txHash)} ↗
                    </a>
                  )}
                </div>
              </div>
              <button
                onClick={() => dismiss(t.id)}
                className="flex h-11 w-11 shrink-0 items-center justify-center rounded-md text-slate-400 transition-colors hover:bg-white/5 hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-aqua-400"
                aria-label={`Dismiss ${OP_LABEL[t.op]} notification`}
              >
                ✕
              </button>
            </div>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastContextValue {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error("useToast must be used within <ToastProvider>");
  return ctx;
}
