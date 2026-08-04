import { shortenAddress } from "../lib/format";

interface Props {
  publicKey: string | null;
  isInstalled: boolean;
  onConnect: () => void;
  onDisconnect: () => void;
}

export default function WalletButton({
  publicKey,
  isInstalled,
  onConnect,
  onDisconnect,
}: Props) {
  if (publicKey) {
    return (
      <button
        onClick={onDisconnect}
        className="btn-ghost group"
        title="Click to disconnect"
      >
        <span className="h-2 w-2 rounded-full bg-emerald-400" />
        <span className="font-mono">{shortenAddress(publicKey)}</span>
        <span className="text-slate-400 group-hover:text-white">·</span>
        <span className="text-slate-400 group-hover:text-white">Disconnect</span>
      </button>
    );
  }

  if (!isInstalled) {
    return (
      <a
        href="https://www.freighter.app/"
        target="_blank"
        rel="noopener noreferrer"
        className="btn-primary"
      >
        Install Freighter
      </a>
    );
  }

  return (
    <button onClick={onConnect} className="btn-primary">
      Connect Wallet
    </button>
  );
}
