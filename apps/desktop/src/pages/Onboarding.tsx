import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { ipc } from "../lib/ipc";
import { cn } from "../lib/cn";

type WizardMode = "create" | "import";

function normalizeMnemonicInput(s: string): string {
  return s.trim().replace(/\s+/g, " ");
}

export function OnboardingPage() {
  const navigate = useNavigate();

  const [vaultExists, setVaultExists] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  /** Unlock-only flow when vault is present */
  const [unlockPass, setUnlockPass] = useState("");

  /** New identity flow */
  const [mode, setMode] = useState<WizardMode>("create");
  const [mnemonic, setMnemonic] = useState("");
  const [mnemonicConfirm, setMnemonicConfirm] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [passphrase2, setPassphrase2] = useState("");
  const [preview, setPreview] = useState<{
    ethereumAddress: string;
    peerIdHex: string;
    pubkeyPem: string;
  } | null>(null);

  /** Registration */
  const [username, setUsername] = useState("");
  const [availability, setAvailability] = useState<boolean | null>(null);
  const [registeredEns, setRegisteredEns] = useState<string | null>(null);
  const [txHash, setTxHash] = useState<string | null>(null);
  /** After unlock when settings have no `last_username` yet */
  const [postUnlockRegister, setPostUnlockRegister] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const exists = await ipc("vault_exists");
        if (!cancelled) setVaultExists(exists);
      } catch (e) {
        if (!cancelled) {
          setVaultExists(false);
          setError(e instanceof Error ? e.message : String(e));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const mnemonicWords = useMemo(() => normalizeMnemonicInput(mnemonic).split(" ").filter(Boolean), [mnemonic]);

  const clearError = useCallback(() => setError(null), []);

  const handleUnlock = async () => {
    clearError();
    setBusy(true);
    try {
      const res = await ipc("unlock_vault", { passphrase: unlockPass });
      if (res.ens) {
        navigate("/chat");
      } else {
        setUnlockPass("");
        setPostUnlockRegister(true);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleGenerate = async () => {
    clearError();
    setBusy(true);
    try {
      const phrase = await ipc("onboarding_generate_mnemonic");
      setMnemonic(phrase);
      setMnemonicConfirm("");
      setPreview(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const handlePreview = async () => {
    clearError();
    const norm = normalizeMnemonicInput(mnemonic);
    if (!norm) {
      setError("Enter a mnemonic first.");
      return;
    }
    if (normalizeMnemonicInput(mnemonicConfirm) !== norm) {
      setError("Confirmation phrase does not match.");
      return;
    }
    setBusy(true);
    try {
      const p = await ipc("onboarding_derived_preview", { mnemonic: norm });
      setPreview(p);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleCommitVault = async () => {
    clearError();
    if (passphrase.length < 8) {
      setError("Passphrase must be at least 8 characters.");
      return;
    }
    if (passphrase !== passphrase2) {
      setError("Passphrases do not match.");
      return;
    }
    const norm = normalizeMnemonicInput(mnemonic);
    setBusy(true);
    try {
      await ipc("onboarding_commit_vault", { mnemonic: norm, passphrase });
      setRegisteredEns(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleCheckUsername = async () => {
    clearError();
    const label = username.trim().toLowerCase();
    if (!label) {
      setError("Enter a username.");
      return;
    }
    setBusy(true);
    setAvailability(null);
    try {
      const res = await ipc("onboarding_check_username", { label });
      setAvailability(res.available);
      if (res.available) {
        setError(null);
      } else {
        setError("That name is already taken on the registrar.");
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleRegister = async () => {
    clearError();
    const label = username.trim().toLowerCase();
    if (!label) {
      setError("Enter a username.");
      return;
    }
    setBusy(true);
    try {
      const res = await ipc("register_username", { label });
      setTxHash(res.txHash);
      setRegisteredEns(res.ens);
      setPostUnlockRegister(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleFinishToChat = () => {
    navigate("/chat");
  };

  if (vaultExists === null) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-slate-400">
        Checking vault…
      </div>
    );
  }

  /** Unlock screen */
  if (vaultExists && !postUnlockRegister && !registeredEns) {
    return (
      <div className="mx-auto flex h-full max-w-md flex-col justify-center px-6 py-10">
        <h1 className="text-2xl font-semibold tracking-tight">Welcome back</h1>
        <p className="mt-2 text-sm text-slate-400">
          Enter your vault passphrase to derive your wallet and start the AXL sidecar.
        </p>
        <label className="mt-6 block text-xs font-medium text-slate-300">
          Passphrase
          <input
            type="password"
            autoComplete="current-password"
            value={unlockPass}
            onChange={(e) => setUnlockPass(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && unlockPass && !busy) {
                void handleUnlock();
              }
            }}
            className="mt-1 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 text-sm text-slate-100 outline-none focus:border-emerald-600"
          />
        </label>
        {error ? (
          <p className="mt-3 text-sm text-red-400" role="alert">
            {error}
          </p>
        ) : null}
        <button
          type="button"
          disabled={busy || !unlockPass}
          onClick={() => void handleUnlock()}
          className={cn(
            "mt-6 rounded-md px-4 py-2 text-sm font-medium",
            busy || !unlockPass
              ? "cursor-not-allowed bg-slate-800 text-slate-500"
              : "bg-emerald-500/90 text-emerald-950 hover:bg-emerald-400",
          )}
        >
          {busy ? "Unlocking…" : "Unlock"}
        </button>
      </div>
    );
  }

  /** Registration-only after unlock (no ENS yet) */
  if (postUnlockRegister && !registeredEns) {
    return (
      <div className="mx-auto flex h-full max-w-md flex-col justify-center px-6 py-10">
        <h1 className="text-2xl font-semibold tracking-tight">Choose your name</h1>
        <p className="mt-2 text-sm text-slate-400">
          Register <code className="font-mono text-slate-300">username.anton.eth</code> directly on Sepolia ENS. Set{" "}
          <code className="font-mono text-slate-300">ANTON_ENS_REGISTRATION_PRIVATE_KEY</code> before launching the app.
        </p>
        <label className="mt-6 block text-xs font-medium text-slate-300">
          Username (label only)
          <input
            type="text"
            value={username}
            onChange={(e) => {
              setUsername(e.target.value);
              setAvailability(null);
            }}
            placeholder="alice"
            className="mt-1 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 font-mono text-sm text-slate-100 outline-none focus:border-emerald-600"
          />
        </label>
        <div className="mt-3 flex flex-wrap gap-2">
          <button
            type="button"
            disabled={busy || !username.trim()}
            onClick={() => void handleCheckUsername()}
            className="rounded-md border border-slate-600 px-3 py-1.5 text-sm text-slate-200 hover:bg-slate-900 disabled:opacity-50"
          >
            Check availability
          </button>
          <button
            type="button"
            disabled={busy || availability !== true}
            onClick={() => void handleRegister()}
            className="rounded-md bg-emerald-500/90 px-3 py-1.5 text-sm font-medium text-emerald-950 disabled:opacity-50"
          >
            Register on-chain
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => {
              setPostUnlockRegister(false);
              navigate("/chat");
            }}
            className="rounded-md px-3 py-1.5 text-sm text-slate-400 underline-offset-2 hover:text-slate-200 hover:underline"
          >
            Skip for now
          </button>
        </div>
        {availability === true ? (
          <p className="mt-2 text-xs text-emerald-400">Looks available on Sepolia ENS.</p>
        ) : null}
        {error ? (
          <p className="mt-3 text-sm text-red-400" role="alert">
            {error}
          </p>
        ) : null}
      </div>
    );
  }

  /** Success */
  if (registeredEns) {
    return (
      <div className="mx-auto flex h-full max-w-lg flex-col justify-center px-6 py-10">
        <h1 className="text-2xl font-semibold tracking-tight text-emerald-400">You&apos;re set</h1>
        <p className="mt-2 text-sm text-slate-400">
          Registered as{" "}
          <span className="font-mono text-slate-200">{registeredEns}</span>
          {txHash ? (
            <>
              {" "}
              — tx{" "}
              <span className="break-all font-mono text-xs text-slate-500">{txHash}</span>
            </>
          ) : null}
        </p>
        <button
          type="button"
          onClick={handleFinishToChat}
          className="mt-8 rounded-md bg-emerald-500/90 px-4 py-2 text-sm font-medium text-emerald-950 hover:bg-emerald-400"
        >
          Open chat
        </button>
      </div>
    );
  }

  /** New user wizard */
  return (
    <div className="mx-auto flex h-full max-w-xl flex-col px-6 py-10">
      <h1 className="text-2xl font-semibold tracking-tight">Welcome to Anton</h1>
      <p className="mt-2 text-sm text-slate-400">
        One mnemonic derives your Ethereum wallet and your AXL node identity. Your vault is encrypted with Argon2id
        + XChaCha20-Poly1305; chat stays in RAM only.
      </p>

      <div className="mt-6 flex gap-2 border-b border-slate-800 pb-4">
        <button
          type="button"
          onClick={() => {
            setMode("create");
            clearError();
            setPreview(null);
          }}
          className={cn(
            "rounded-md px-3 py-1.5 text-sm font-medium",
            mode === "create" ? "bg-slate-800 text-white" : "text-slate-400 hover:bg-slate-900",
          )}
        >
          Create identity
        </button>
        <button
          type="button"
          onClick={() => {
            setMode("import");
            clearError();
            setPreview(null);
          }}
          className={cn(
            "rounded-md px-3 py-1.5 text-sm font-medium",
            mode === "import" ? "bg-slate-800 text-white" : "text-slate-400 hover:bg-slate-900",
          )}
        >
          Import mnemonic
        </button>
      </div>

      {mode === "create" ? (
        <div className="mt-4 space-y-3">
          <p className="text-xs text-slate-500">{mnemonicWords.length} words · English BIP39</p>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              disabled={busy}
              onClick={() => void handleGenerate()}
              className="rounded-md bg-emerald-500/90 px-3 py-1.5 text-sm font-medium text-emerald-950 disabled:opacity-50"
            >
              Generate new phrase
            </button>
          </div>
          <textarea
            readOnly={false}
            value={mnemonic}
            onChange={(e) => {
              setMnemonic(e.target.value);
              setPreview(null);
            }}
            rows={3}
            placeholder="Click generate or paste 12 words…"
            className="w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 font-mono text-sm text-slate-100 outline-none focus:border-emerald-600"
          />
        </div>
      ) : (
        <div className="mt-4">
          <textarea
            value={mnemonic}
            onChange={(e) => {
              setMnemonic(e.target.value);
              setPreview(null);
            }}
            rows={4}
            placeholder="Paste your 12-word phrase…"
            className="w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 font-mono text-sm text-slate-100 outline-none focus:border-emerald-600"
          />
        </div>
      )}

      <label className="mt-6 block text-xs font-medium text-slate-300">
        Confirm phrase (type or paste again)
        <textarea
          value={mnemonicConfirm}
          onChange={(e) => setMnemonicConfirm(e.target.value)}
          rows={3}
          className="mt-1 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 font-mono text-sm text-slate-100 outline-none focus:border-emerald-600"
        />
      </label>

      <button
        type="button"
        disabled={busy || !mnemonic.trim()}
        onClick={() => void handlePreview()}
        className="mt-4 w-fit rounded-md border border-slate-600 px-3 py-1.5 text-sm text-slate-200 hover:bg-slate-900 disabled:opacity-50"
      >
        Preview derived keys
      </button>

      {preview ? (
        <div className="mt-4 rounded-lg border border-slate-800 bg-slate-900/40 p-4 text-xs">
          <p className="font-medium text-slate-200">Derived preview</p>
          <dl className="mt-2 space-y-1 font-mono text-slate-400">
            <div>
              <dt className="text-slate-500">Ethereum</dt>
              <dd className="break-all text-slate-300">{preview.ethereumAddress}</dd>
            </div>
            <div>
              <dt className="text-slate-500">AXL peer id</dt>
              <dd className="break-all text-slate-300">{preview.peerIdHex}</dd>
            </div>
            <div>
              <dt className="text-slate-500">axl_pubkey (PEM)</dt>
              <dd className="whitespace-pre-wrap break-all text-slate-400">{preview.pubkeyPem}</dd>
            </div>
          </dl>
        </div>
      ) : null}

      <div className="mt-8 grid gap-3 border-t border-slate-800 pt-6">
        <p className="text-sm font-medium text-slate-200">Encrypt vault</p>
        <label className="block text-xs font-medium text-slate-300">
          Passphrase (min 8 characters)
          <input
            type="password"
            value={passphrase}
            onChange={(e) => setPassphrase(e.target.value)}
            className="mt-1 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 text-sm outline-none focus:border-emerald-600"
          />
        </label>
        <label className="block text-xs font-medium text-slate-300">
          Confirm passphrase
          <input
            type="password"
            value={passphrase2}
            onChange={(e) => setPassphrase2(e.target.value)}
            className="mt-1 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 text-sm outline-none focus:border-emerald-600"
          />
        </label>
        <button
          type="button"
          disabled={busy || !preview}
          onClick={() => void handleCommitVault()}
          className="rounded-md bg-emerald-500/90 px-4 py-2 text-sm font-medium text-emerald-950 disabled:opacity-50"
        >
          {busy ? "Saving…" : "Save vault & start AXL"}
        </button>
        <p className="text-xs text-slate-500">
          Uses Argon2id defaults from the design doc (64 MiB memory). The sidecar starts after this step; ensure the
          bundled <code className="font-mono">axl</code> binary is present for your target.
        </p>
      </div>

      <div className="mt-10 border-t border-slate-800 pt-8">
        <h2 className="text-sm font-medium text-slate-200">Register on Sepolia ENS</h2>
        <p className="mt-1 text-xs text-slate-500">
          After your vault is saved, create <code className="font-mono">name.anton.eth</code> on Sepolia. Gas is paid by{" "}
          <code className="font-mono">ANTON_ENS_REGISTRATION_PRIVATE_KEY</code> (the parent-name operator); your derived
          address is the final ENS owner.
        </p>
        <label className="mt-4 block text-xs font-medium text-slate-300">
          Username
          <input
            type="text"
            value={username}
            onChange={(e) => {
              setUsername(e.target.value);
              setAvailability(null);
            }}
            className="mt-1 w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 font-mono text-sm outline-none focus:border-emerald-600"
          />
        </label>
        <div className="mt-3 flex flex-wrap gap-2">
          <button
            type="button"
            disabled={busy || !username.trim()}
            onClick={() => void handleCheckUsername()}
            className="rounded-md border border-slate-600 px-3 py-1.5 text-sm text-slate-200 hover:bg-slate-900 disabled:opacity-50"
          >
            Check ENS
          </button>
          <button
            type="button"
            disabled={busy || availability !== true}
            onClick={() => void handleRegister()}
            className="rounded-md bg-slate-100 px-3 py-1.5 text-sm font-medium text-slate-900 disabled:opacity-50"
          >
            Register
          </button>
        </div>
        {availability === true ? (
          <p className="mt-2 text-xs text-emerald-400">Available on Sepolia ENS.</p>
        ) : null}
      </div>

      {error ? (
        <p className="mt-6 text-sm text-red-400" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}
