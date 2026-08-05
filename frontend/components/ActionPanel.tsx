import { useMemo, useState } from "react";
import { deposit, withdraw } from "../lib/contract";
import { fromStroops, toStroops, formatUsd } from "../lib/format";

interface Props {
  publicKey: string | null;
  userBalance: bigint;
  usdcBalance: bigint;
  amount: string;
  onAmountChange: (value: string) => void;
  paused: boolean;
  onConnect: () => void;
  onDone: () => void;
}

type Tab = "deposit" | "withdraw";

export default function ActionPanel({
  publicKey,
  userBalance,
  usdcBalance,
  amount,
  onAmountChange,
  paused,
  onConnect,
  onDone,
}: Props) {
  const [tab, setTab] = useState<Tab>("deposit");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ kind: "ok" | "err"; text: string } | null>(
    null,
  );

  const max = tab === "deposit" ? usdcBalance : userBalance;
  const depositBlocked = paused && tab === "deposit";

  const validation = useMemo(() => {
    if (!amount) return null;
    let raw: bigint;
    try {
      raw = toStroops(amount);
    } catch {
      return "Enter a valid number";
    }
    if (raw <= BigInt(0)) return "Amount must be positive";
    if (raw > max) return `Exceeds available ${formatUsd(max)}`;
    return null;
  }, [amount, max]);

  const canSubmit = publicKey && amount && !validation && !busy && !depositBlocked;

  async function handleSubmit() {
    if (!publicKey) {
      onConnect();
      return;
    }
    if (depositBlocked) {
      setMsg({ kind: "err", text: "Deposits are paused — please try again later." });
      return;
    }
    if (!canSubmit) return;
    setBusy(true);
    setMsg(null);
    try {
      const raw = toStroops(amount);
      const res =
        tab === "deposit"
          ? await deposit(publicKey, raw)
          : await withdraw(publicKey, raw);
      setMsg({
        kind: "ok",
        text: `${tab === "deposit" ? "Deposited" : "Withdrew"} ${formatUsd(
          raw,
        )} · tx ${res.hash.slice(0, 8)}…`,
      });
      onAmountChange("");
      onDone();
    } catch (err: any) {
      setMsg({ kind: "err", text: err?.message || "Transaction failed" });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="card">
      <div className="mb-4 flex gap-1 rounded-xl bg-ink-900/70 p-1">
        <button
          className={`tab ${tab === "deposit" ? "tab-active" : "tab-inactive"}`}
          onClick={() => {
            setTab("deposit");
            setMsg(null);
          }}
        >
          Deposit
        </button>
        <button
          className={`tab ${tab === "withdraw" ? "tab-active" : "tab-inactive"}`}
          onClick={() => {
            setTab("withdraw");
            setMsg(null);
          }}
        >
          Withdraw
        </button>
      </div>

      <label className="mb-2 flex items-center justify-between text-sm text-slate-400">
        <span>Amount (USDC)</span>
        <button
          className="text-aqua-300 hover:text-aqua-200"
          onClick={() => onAmountChange(fromStroops(max, 7))}
          type="button"
        >
          Max: {formatUsd(max)}
        </button>
      </label>

      <input
        className="input"
        inputMode="decimal"
        placeholder="0.00"
        value={amount}
        onChange={(e) => onAmountChange(e.target.value.replace(/[^0-9.]/g, ""))}
      />

      {validation && (
        <p className="mt-2 text-sm text-rose-300">{validation}</p>
      )}

      {depositBlocked && (
        <p className="mt-2 text-sm text-amber-300">
          Deposits are paused by the admin. You can still withdraw.
        </p>
      )}

      {/* Fee estimate — Soroban testnet base fee is a fraction of a cent. */}
      <div className="mt-3 flex justify-between text-xs text-slate-500">
        <span>Estimated network fee</span>
        <span>~0.00001 XLM</span>
      </div>

      <button
        className="btn-primary mt-4 w-full"
        onClick={handleSubmit}
        disabled={Boolean(publicKey) && !canSubmit}
      >
        {!publicKey
          ? "Connect Wallet"
          : busy
          ? "Confirming…"
          : tab === "deposit"
          ? "Deposit USDC"
          : "Withdraw USDC"}
      </button>

      {msg && (
        <p
          className={`mt-3 text-sm ${
            msg.kind === "ok" ? "text-emerald-300" : "text-rose-300"
          }`}
        >
          {msg.text}
        </p>
      )}

      <p className="mt-4 text-center text-xs text-slate-500">
        Your principal is never at risk — withdraw 100% anytime.
      </p>
    </div>
  );
}
