/**
 * "Test connection" — one real request, validated by the engine.
 *
 * Runs the exact path a game decision runs: the engine builds the request from
 * the player's endpoint config, the transport performs it, and the engine
 * extracts and decodes the reply. That matters because the transport
 * deliberately returns non-2xx bodies rather than rejecting, so a vendor's
 * error message survives to be shown — which makes "the fetch resolved" a
 * meaningless success signal. A rejected key and an unknown model both come
 * back as well-formed HTTP responses.
 */

import { ensureWasmInit } from "../engineRuntime";
import { executeLlmRequest } from "./llmClient";
import { endpointOf, type LlmHttpRequestSpec, type LlmProfile } from "./types";

/** Bound for a probe. Shorter than a real decision: a player is watching it. */
export const LLM_PROBE_TIMEOUT_MS = 20_000;

export type LlmProbeResult = { ok: true } | { ok: false; error: string };

interface ProbeRequestResult {
  request?: LlmHttpRequestSpec;
  error?: string;
}

interface ProbeValidation {
  ok?: boolean;
  error?: string;
}

export async function testLlmEndpoint(
  profile: LlmProfile,
  options: { timeoutMs?: number } = {},
): Promise<LlmProbeResult> {
  let wasm: typeof import("@wasm/engine");
  try {
    await ensureWasmInit();
    wasm = await import("@wasm/engine");
  } catch (error) {
    return { ok: false, error: `Engine unavailable: ${describe(error)}` };
  }

  // The engine refuses here for a missing model, a missing key, or a credential
  // bound for a plaintext endpoint — before anything reaches the network.
  const built = wasm.buildLlmProbeRequest(JSON.stringify(endpointOf(profile))) as ProbeRequestResult;
  if (!built?.request) {
    return { ok: false, error: built?.error ?? "Could not build a request for this endpoint" };
  }

  let body: string;
  try {
    body = await executeLlmRequest(built.request, {
      timeoutMs: options.timeoutMs ?? LLM_PROBE_TIMEOUT_MS,
    });
  } catch (error) {
    return { ok: false, error: describe(error) };
  }

  const verdict = wasm.validateLlmProbeResponse(profile.provider, body) as ProbeValidation;
  if (verdict?.ok) return { ok: true };
  return { ok: false, error: verdict?.error ?? "The endpoint returned a reply the engine could not use" };
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
