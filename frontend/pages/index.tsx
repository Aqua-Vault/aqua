import { useEffect, useMemo, useState } from "react";
import Head from "next/head";
import { useWallet } from "../hooks/useWallet";
import { useVault } from "../hooks/useVault";
import { DrawRecord, loadDraws } from "../lib/history";
import { IS_CONFIGURED, VAULT_ID, explorerContractUrl } from "../lib/config";
import { shortenAddress, toStroops } from "../lib/format";
import { getAdmin } from "../lib/contract";
import { resetLedgerTimeCache } from "../lib/ledger";

import WalletButton from "../components/WalletButton";
import StatsBar from "../components/StatsBar";
import DepositCard from "../components/DepositCard";
import ActionPanel from "../components/ActionPanel";
import AdminPanel from "../components/AdminPanel";
import LiveDrawFeed from "../components/LiveDrawFeed";
import ProbabilityCalculator from "../components/ProbabilityCalculator";

export default function Home() {
  const wallet = useWallet();
  const vault = useVault(wallet.publicKey);
  const [depositAmount, setDepositAmount] = useState("");
  const [draws, setDraws] = useState<DrawRecord[]>([]);
  const [admin, setAdmin] = useState<string | null>(null);
  const [connectErr, setConnectErr] = useState<string | null>(null);

  useEffect(() => setDraws(loadDraws()), []);

  useEffect(() => {
    if (IS_CONFIGURED) getAdmin().then(setAdmin).catch(() => setAdmin(null));
  }, []);

  // Mid-session account switch in Freighter: refresh vault state and re-anchor
  // the ledger clock so the new account's numbers render immediately.
  useEffect(() => {
    if (wallet.accountChanged) {
      resetLedgerTimeCache();
      vault.refresh();
      wallet.acknowledgeAccountChange();
    }
  }, [wallet.accountChanged, wallet, vault]);

  // Ledger close time captured alongside stats on the same poll, so the
  // countdown stays anchored to chain time. Fallback: null → wall-clock.
  const ledgerCloseMs = vault.stats?.ledgerCloseMs ?? null;

  // Live preview deposit for the probability calculator (raw stroops).
  const projectedDeposit = useMemo(() => {
    try {
      return toStroops(depositAmount);
    } catch {
      return BigInt(0);
    }
  }, [depositAmount]);

  const canDraw = (vault.stats?.secondsUntilNextDraw ?? 1) <= 0;
  const isAdmin = useMemo(
    () => Boolean(wallet.publicKey && admin && wallet.publicKey === admin),
    [wallet.publicKey, admin],
  );

  async function handleConnect() {
    setConnectErr(null);
    try {
      await wallet.connect();
      vault.refresh();
    } catch (err: any) {
      setConnectErr(err?.message || "Failed to connect");
    }
  }

  function afterAction() {
    vault.refresh();
    setDraws(loadDraws());
  }

  return (
    <>
      <Head>
        <title>Aqua — No-Loss Prize Savings on Stellar</title>
      </Head>

      <div className="mx-auto max-w-6xl px-4 py-8 sm:py-12">
        {/* Header */}
        <header className="flex flex-wrap items-center justify-between gap-4">
          <div className="flex items-center gap-3">
            <div className="flex h-11 w-11 items-center justify-center rounded-2xl bg-aqua-500/20 text-2xl animate-floaty">
              🌊
            </div>
            <div>
              <h1 className="text-2xl font-extrabold tracking-tight text-white">
                Aqua
              </h1>
              <p className="text-xs text-slate-400">
                No-loss prize-linked savings
              </p>
            </div>
          </div>
          <WalletButton
            publicKey={wallet.publicKey}
            isInstalled={wallet.isInstalled}
            onConnect={handleConnect}
            onDisconnect={wallet.disconnect}
          />
        </header>

        {connectErr && (
          <div className="mt-4 rounded-xl border border-rose-500/30 bg-rose-500/10 px-4 py-3 text-sm text-rose-200">
            {connectErr}
          </div>
        )}

        {wallet.networkMismatch && (
          <div className="mt-4 rounded-xl border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-sm text-amber-200">
            {wallet.networkMismatch}
          </div>
        )}

        {!IS_CONFIGURED && (
          <div className="mt-4 rounded-xl border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-sm text-amber-200">
            Contracts not configured. Run{" "}
            <code className="font-mono">make deploy</code> and set the
            <code className="font-mono"> NEXT_PUBLIC_*</code> variables in{" "}
            <code className="font-mono">frontend/.env.local</code>.
          </div>
        )}

        {vault.error && IS_CONFIGURED && (
          <div className="mt-4 rounded-xl border border-rose-500/30 bg-rose-500/10 px-4 py-3 text-sm text-rose-200">
            {vault.error}
          </div>
        )}

        {/* Hero copy */}
        <section className="mt-8 text-center sm:mt-10">
          <h2 className="mx-auto max-w-3xl text-3xl font-extrabold leading-tight text-white sm:text-5xl">
            Save without losing.{" "}
            <span className="text-aqua-300">Win the yield.</span>
          </h2>
          <p className="mx-auto mt-4 max-w-2xl text-slate-400">
            Deposit USDC into a shared vault. Your principal earns yield through
            Blend and is fully withdrawable at any time. Every draw, one saver
            wins 100% of the pooled yield — chosen by verifiable on-chain
            randomness.
          </p>
        </section>

        {/* Stats */}
        <section className="mt-8">
          <StatsBar
            stats={vault.stats}
            ledgerCloseMs={ledgerCloseMs}
            loading={vault.loading}
          />
        </section>

        {/* Main grid */}
        <section className="mt-6 grid grid-cols-1 gap-6 lg:grid-cols-3">
          <div className="space-y-6 lg:col-span-2">
            <DepositCard
              publicKey={wallet.publicKey}
              userBalance={vault.userBalance}
              stats={vault.stats}
            />
            <LiveDrawFeed draws={draws} />
          </div>

          <div className="space-y-6">
            <ActionPanel
              publicKey={wallet.publicKey}
              userBalance={vault.userBalance}
              usdcBalance={vault.usdcBalance}
              amount={depositAmount}
              onAmountChange={setDepositAmount}
              onConnect={handleConnect}
              onDone={afterAction}
            />
            <ProbabilityCalculator
              userBalance={vault.userBalance}
              totalDeposits={vault.stats?.totalDeposits ?? BigInt(0)}
              projectedDeposit={projectedDeposit}
            />
            <AdminPanel
              publicKey={wallet.publicKey}
              canDraw={canDraw}
              onDrawComplete={afterAction}
            />
          </div>
        </section>

        {/* Footer */}
        <footer className="mt-12 flex flex-wrap items-center justify-between gap-3 border-t border-white/5 pt-6 text-xs text-slate-500">
          <span>
            Built on Stellar · Soroban · CAP-0074 randomness · Blend yield
          </span>
          {IS_CONFIGURED && (
            <a
              href={explorerContractUrl(VAULT_ID)}
              target="_blank"
              rel="noopener noreferrer"
              className="text-aqua-300 hover:text-aqua-200"
            >
              Vault: {shortenAddress(VAULT_ID)} ↗
            </a>
          )}
          {isAdmin && <span className="text-amber-300">You are the admin</span>}
        </footer>
      </div>
    </>
  );
}
