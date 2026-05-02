/**
 * AXL HTTP client.
 *
 * The AXL Go binary runs as a Tauri sidecar bound to `127.0.0.1:9002` and
 * exposes a tiny HTTP surface for sending and receiving payloads over the
 * encrypted P2P mesh. This client wraps that surface so the desktop UI,
 * tests, and any future Node-side tooling share the same caller.
 *
 * Endpoints (per the AXL spec):
 *   POST /send       body: payload bytes, headers: X-Destination-Peer-Id
 *   GET  /recv       long-poll; returns a payload + X-From-Peer-Id header
 *   GET  /topology   network info
 */

export interface AxlClientOptions {
  /** Defaults to `http://127.0.0.1:9002`. */
  baseUrl?: string;
  /**
   * Long-poll timeout in milliseconds for `/recv`. The AXL sidecar will hold
   * the request open until either a payload arrives or this elapses.
   */
  recvTimeoutMs?: number;
  /** Optional `fetch` override (useful for tests / Node environments). */
  fetch?: typeof globalThis.fetch;
  /**
   * AbortSignal wired into every request, so callers can cancel the long
   * poll loop on app quit or conversation close.
   */
  signal?: AbortSignal;
}

export interface InboundPayload {
  fromPeerId: string;
  body: Uint8Array;
}

export interface Topology {
  selfPeerId: string;
  peerCount: number;
  bootstrapPeers: ReadonlyArray<string>;
}

export class AxlClient {
  readonly baseUrl: string;
  readonly recvTimeoutMs: number;

  private readonly fetchImpl: typeof globalThis.fetch;
  private readonly externalSignal?: AbortSignal;

  constructor(options: AxlClientOptions = {}) {
    this.baseUrl = options.baseUrl ?? "http://127.0.0.1:9002";
    this.recvTimeoutMs = options.recvTimeoutMs ?? 30_000;
    this.fetchImpl = options.fetch ?? globalThis.fetch.bind(globalThis);
    this.externalSignal = options.signal;
  }

  /** Send a binary payload to a destination peer. */
  async send(destinationPeerId: string, body: Uint8Array): Promise<void> {
    const destination = destinationPeerId.trim().replace(/^0x/i, "");
    const payload = new ArrayBuffer(body.byteLength);
    new Uint8Array(payload).set(body);
    const res = await this.fetchImpl(`${this.baseUrl}/send`, {
      method: "POST",
      headers: {
        "Content-Type": "application/octet-stream",
        "X-Destination-Peer-Id": destination,
      },
      body: payload,
      signal: this.externalSignal,
    });
    if (!res.ok) {
      throw new AxlHttpError(res.status, await safeReadText(res));
    }
  }

  /**
   * Long-poll for a single inbound payload. Resolves with the next message
   * (or `null` if the long poll timed out without one).
   */
  async recv(): Promise<InboundPayload | null> {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), this.recvTimeoutMs);
    const composedSignal = composeSignals(controller.signal, this.externalSignal);

    try {
      const res = await this.fetchImpl(`${this.baseUrl}/recv`, {
        method: "GET",
        signal: composedSignal,
      });
      if (res.status === 204) return null;
      if (!res.ok) throw new AxlHttpError(res.status, await safeReadText(res));

      const fromPeerId = res.headers.get("X-From-Peer-Id");
      if (!fromPeerId) throw new Error("AXL /recv missing X-From-Peer-Id header");

      const buf = new Uint8Array(await res.arrayBuffer());
      return { fromPeerId, body: buf };
    } catch (err) {
      if (isAbortError(err) && controller.signal.aborted) return null;
      throw err;
    } finally {
      clearTimeout(timeout);
    }
  }

  /**
   * Convenience helper: yields inbound payloads as they arrive, repeatedly
   * calling `recv()` until the external signal fires.
   */
  async *recvStream(): AsyncIterable<InboundPayload> {
    while (!this.externalSignal?.aborted) {
      const next = await this.recv();
      if (next) yield next;
    }
  }

  /** Returns the local peer's view of the AXL mesh. */
  async topology(): Promise<Topology> {
    const res = await this.fetchImpl(`${this.baseUrl}/topology`, {
      method: "GET",
      signal: this.externalSignal,
    });
    if (!res.ok) throw new AxlHttpError(res.status, await safeReadText(res));
    return (await res.json()) as Topology;
  }
}

export class AxlHttpError extends Error {
  constructor(
    readonly status: number,
    readonly body: string,
  ) {
    super(`AXL HTTP ${status}: ${body || "<empty body>"}`);
    this.name = "AxlHttpError";
  }
}

async function safeReadText(res: Response): Promise<string> {
  try {
    return await res.text();
  } catch {
    return "";
  }
}

function isAbortError(err: unknown): boolean {
  return (
    typeof err === "object" &&
    err !== null &&
    "name" in err &&
    (err as { name?: unknown }).name === "AbortError"
  );
}

function composeSignals(
  ...signals: ReadonlyArray<AbortSignal | undefined>
): AbortSignal {
  const real = signals.filter(
    (signal): signal is AbortSignal => signal != null,
  );
  if (real.length === 0) return new AbortController().signal;
  if (real.length === 1) return real[0]!;

  // AbortSignal.any was standardized in 2024; fall back to a manual
  // controller for environments that haven't shipped it yet.
  if (typeof (AbortSignal as unknown as { any?: unknown }).any === "function") {
    return (AbortSignal as unknown as {
      any: (signals: AbortSignal[]) => AbortSignal;
    }).any(real);
  }

  const controller = new AbortController();
  for (const signal of real) {
    if (signal.aborted) {
      controller.abort();
      break;
    }
    signal.addEventListener("abort", () => controller.abort(), { once: true });
  }
  return controller.signal;
}
