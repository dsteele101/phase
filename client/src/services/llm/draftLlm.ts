/**
 * LLM-driven bot seats in a draft pod.
 *
 * Strictly opt-in and always recoverable: with no configured profile, or on any
 * failure, the pick is submitted through the ordinary path and every bot seat
 * is drafted by the heuristic bot in `draft_wasm::bot_ai`.
 *
 * As in the game path, the engine owns the decision. It renders each seat's own
 * `DraftPlayerView` into a prompt, builds the HTTP request, and resolves the
 * reply into pack cards; this module performs the calls.
 */

import { withDraftEngineOperation } from "../../adapter/draft-adapter";
import type {
  DraftPlayerView,
  LlmDraftResponsePayload,
} from "../../adapter/draft-adapter";
import { ensureSetCatalog } from "../setCatalog";
import { debugLog } from "../../game/debugLog";
import { executeLlmRequest } from "./llmClient";
import { endpointOf, type LlmDraftOutcome, type LlmDraftPickRequest, type LlmProfile } from "./types";

/**
 * Per-seat ceiling for a draft pick.
 *
 * Tighter than the in-game budget because the player is waiting on a click they
 * just made. The call runs with no engine lease held, so a seat that misses the
 * window is simply drafted by the heuristic bot, with no consequence beyond
 * that one card.
 */
export const LLM_DRAFT_TIMEOUT_MS = 20_000;

/** Set code -> printed name, so the format brief reads "Triple Mirrodin". */
async function setNameMap(): Promise<Record<string, string>> {
  const catalog = await ensureSetCatalog();
  if (!catalog) return {};
  return Object.fromEntries(
    Object.entries(catalog).map(([code, info]) => [code.toUpperCase(), info.name]),
  );
}

/**
 * Gather every LLM bot seat's reply for the pick about to be submitted.
 *
 * Deliberately split across the engine-operation lease rather than run inside
 * it. The lease is a SINGLETON serializing every draft engine call, so holding
 * it across a provider round trip would block `getView`, autosave and any other
 * draft operation for the whole request timeout -- up to 20s per pick, on a
 * queue the UI depends on. The three phases here are:
 *
 *   1. build the requests under a short-lived lease (a read-only engine call),
 *   2. perform the network I/O with NO lease held,
 *   3. hand the replies back so the caller can reacquire and submit.
 *
 * Correctness across the gap is the engine's, not this function's: every reply
 * carries the pack fingerprint it was built from, and `submitPickWithLlmBotPicks`
 * re-reads the live pack and refuses any seat whose pack moved on. A slow
 * provider therefore costs that seat its flavour, never a wrong pick.
 *
 * Returns an empty list whenever the LLM path cannot contribute, which the
 * caller treats as "submit the ordinary way".
 */
export async function collectLlmDraftResponses(
  profile: LlmProfile,
  botSeats: number[],
  stillCurrent: () => boolean,
): Promise<LlmDraftResponsePayload[]> {
  if (botSeats.length === 0) return [];

  // Phase 1 -- under lease, read-only. No pick is applied here.
  let requests: LlmDraftPickRequest[];
  try {
    const setNames = await setNameMap();
    requests = await withDraftEngineOperation((lease) =>
      lease.buildLlmDraftPickRequests(
        JSON.stringify(endpointOf(profile)),
        botSeats,
        setNames,
      ),
    );
  } catch (error) {
    debugLog(`LLM drafters unavailable; using the engine bots: ${describe(error)}`, "warn");
    return [];
  }
  if (requests.length === 0 || !stillCurrent()) return [];

  // Phase 2 -- no lease held. One call per seat, in parallel: the seats pick
  // simultaneously in the rules (CR 905.1a), and serializing them would
  // multiply the pick's latency by the pod size.
  const settled = await Promise.all(
    requests.map(async (request): Promise<LlmDraftResponsePayload | null> => {
      try {
        const body = await executeLlmRequest(request.request, {
          timeoutMs: LLM_DRAFT_TIMEOUT_MS,
        });
        return {
          seat: request.seat,
          fingerprint: request.fingerprint,
          provider: profile.provider,
          body,
        };
      } catch (error) {
        debugLog(`LLM drafter (seat ${request.seat}) failed: ${describe(error)}`, "warn");
        return null;
      }
    }),
  );

  return settled.filter((entry): entry is LlmDraftResponsePayload => entry !== null);
}

/**
 * Report what the engine did with each seat's reply.
 *
 * Only failures are reported. A drafter's reasoning is derived from its own
 * pack and pool -- both private to that seat -- and `debugLog` writes a
 * `visibility: "Public"` entry into the shared game log, so publishing it would
 * turn hidden draft information into a public artifact.
 */
export function reportLlmDraftOutcomes(outcomes: LlmDraftOutcome[]): void {
  for (const outcome of outcomes) {
    if (!outcome.used && outcome.error) {
      debugLog(
        `LLM drafter (seat ${outcome.seat}) fell back to the engine bot: ${outcome.error}`,
        "warn",
      );
    }
  }
}

/** Bot seats in a pod, in seat order. Seat 0 is the local player. */
export function botSeatIndices(view: DraftPlayerView): number[] {
  return view.seats.filter((seat) => seat.is_bot).map((seat) => seat.seat_index);
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
