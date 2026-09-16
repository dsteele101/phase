import { beforeEach, describe, expect, it, vi } from "vitest";

const llmMocks = vi.hoisted(() => ({
  executeLlmRequest: vi.fn<
    (spec: unknown, options?: { timeoutMs?: number; signal?: AbortSignal }) => Promise<string>
  >(),
}));

const leaseMocks = vi.hoisted(() => ({
  withDraftEngineOperation: vi.fn(),
}));

vi.mock("../llmClient", () => ({ executeLlmRequest: llmMocks.executeLlmRequest }));
vi.mock("../../../adapter/draft-adapter", () => ({
  withDraftEngineOperation: leaseMocks.withDraftEngineOperation,
}));
vi.mock("../../setCatalog", () => ({
  ensureSetCatalog: async () => ({
    mrd: { name: "Mirrodin", released_at: "2003-10-02" },
  }),
}));
const debugMocks = vi.hoisted(() => ({ debugLog: vi.fn() }));
vi.mock("../../../game/debugLog", () => ({ debugLog: debugMocks.debugLog }));

import type { DraftPlayerView } from "../../../adapter/draft-adapter";
import {
  botSeatIndices,
  cancelLlmDraftRun,
  collectLlmDraftResponses,
  isLlmDraftDisabled,
  reportLlmDraftOutcomes,
  resetLlmDraftBreaker,
} from "../draftLlm";
import type { LlmProfile } from "../types";

const PROFILE: LlmProfile = {
  id: "p1",
  name: "Test",
  provider: "Anthropic",
  baseUrl: null,
  apiKey: "k",
  model: "claude-sonnet-5",
  maxOutputTokens: null,
  temperature: null,
  enabled: true,
};

const REQUEST = {
  url: "https://x.test",
  method: "POST",
  headers: [] as [],
  body: "{}",
};

function pickRequest(seat: number, fingerprint: string) {
  return {
    seat,
    fingerprint,
    optionCount: 15,
    requiredPickCount: 1,
    request: REQUEST,
  };
}

/** Drives the mocked lease, recording whether it was held during network I/O. */
function leaseReturning(requests: unknown, onEnter?: () => void) {
  leaseMocks.withDraftEngineOperation.mockImplementation(async (work: (l: unknown) => unknown) => {
    onEnter?.();
    const lease = {
      buildLlmDraftPickRequests: vi.fn(() => requests),
    };
    const result = await work(lease);
    leaseHeld = false;
    return result;
  });
}

let leaseHeld = false;

beforeEach(() => {
  llmMocks.executeLlmRequest.mockReset();
  leaseMocks.withDraftEngineOperation.mockReset();
  debugMocks.debugLog.mockReset();
  resetLlmDraftBreaker();
  cancelLlmDraftRun();
  leaseHeld = false;
});

describe("LLM drafters", () => {
  it("collects nothing when the pod has no bot seats", async () => {
    leaseReturning([pickRequest(1, "fp-1")]);

    await expect(collectLlmDraftResponses(PROFILE, [], () => true)).resolves.toEqual([]);
    expect(leaseMocks.withDraftEngineOperation).not.toHaveBeenCalled();
  });

  it("returns one reply per seat, tagged with the pack fingerprint it was built from", async () => {
    leaseReturning([pickRequest(1, "fp-1"), pickRequest(2, "fp-2")]);
    llmMocks.executeLlmRequest.mockResolvedValue('{"choice":3}');

    await expect(collectLlmDraftResponses(PROFILE, [1, 2], () => true)).resolves.toEqual([
      { seat: 1, fingerprint: "fp-1", provider: "Anthropic", body: '{"choice":3}' },
      { seat: 2, fingerprint: "fp-2", provider: "Anthropic", body: '{"choice":3}' },
    ]);
  });

  /// The follow-up this split exists for: the singleton draft engine queue must
  /// not be blocked while a provider is answering.
  it("performs provider I/O with no engine lease held", async () => {
    leaseReturning([pickRequest(1, "fp-1")], () => {
      leaseHeld = true;
    });
    let heldDuringFetch: boolean | null = null;
    llmMocks.executeLlmRequest.mockImplementation(async () => {
      heldDuringFetch = leaseHeld;
      return '{"choice":0}';
    });

    await collectLlmDraftResponses(PROFILE, [1], () => true);

    expect(heldDuringFetch).toBe(false);
  });

  it("gives the engine the format's set names so the brief can read 'Triple Mirrodin'", async () => {
    let seenSetNames: unknown;
    leaseMocks.withDraftEngineOperation.mockImplementation(
      async (work: (l: unknown) => unknown) =>
        work({
          buildLlmDraftPickRequests: (_endpoint: string, _seats: number[], setNames: unknown) => {
            seenSetNames = setNames;
            return [];
          },
        }),
    );

    await collectLlmDraftResponses(PROFILE, [1], () => true);

    expect(seenSetNames).toEqual({ MRD: "Mirrodin" });
  });

  it("collects nothing when every provider call fails", async () => {
    leaseReturning([pickRequest(1, "fp-1")]);
    llmMocks.executeLlmRequest.mockRejectedValue(new Error("network down"));

    await expect(collectLlmDraftResponses(PROFILE, [1], () => true)).resolves.toEqual([]);
  });

  it("collects nothing when the engine builds no requests", async () => {
    leaseReturning([]);

    await expect(collectLlmDraftResponses(PROFILE, [1], () => true)).resolves.toEqual([]);
    expect(llmMocks.executeLlmRequest).not.toHaveBeenCalled();
  });

  it("collects nothing when request building throws", async () => {
    leaseMocks.withDraftEngineOperation.mockRejectedValue(new Error("draft not initialized"));

    await expect(collectLlmDraftResponses(PROFILE, [1], () => true)).resolves.toEqual([]);
  });

  /// A pick the player has already superseded must not reach the provider.
  it("abandons the round trip when the pick is no longer current", async () => {
    leaseReturning([pickRequest(1, "fp-1")]);

    await expect(collectLlmDraftResponses(PROFILE, [1], () => false)).resolves.toEqual([]);
    expect(llmMocks.executeLlmRequest).not.toHaveBeenCalled();
  });

  it("still returns the seats that answered when others did not", async () => {
    leaseReturning([pickRequest(1, "fp-1"), pickRequest(2, "fp-2")]);
    llmMocks.executeLlmRequest
      .mockResolvedValueOnce('{"choice":1}')
      .mockRejectedValueOnce(new Error("timeout"));

    const responses = await collectLlmDraftResponses(PROFILE, [1, 2], () => true);

    expect(responses).toHaveLength(1);
    expect(responses[0]?.seat).toBe(1);
  });

  it("reads bot seats off the engine-published seat list", () => {
    const view = {
      seats: [
        { seat_index: 0, is_bot: false },
        { seat_index: 1, is_bot: true },
        { seat_index: 2, is_bot: true },
      ],
    } as unknown as DraftPlayerView;

    expect(botSeatIndices(view)).toEqual([1, 2]);
  });

  /// Reasoning is derived from a seat's private pack and pool, and debugLog
  /// writes a public game-log entry — so it must never be reported.
  it("never reports model reasoning, only failures", () => {
    reportLlmDraftOutcomes([
      { seat: 1, used: true, reasoning: "I am hoarding removal" },
      { seat: 2, used: false, error: "provider timeout" },
    ]);

    const messages = debugMocks.debugLog.mock.calls.map((call) => String(call[0]));
    expect(messages.some((message) => message.includes("hoarding removal"))).toBe(false);
    expect(messages.some((message) => message.includes("provider timeout"))).toBe(true);
  });

  // ── Cancellation lifecycle ───────────────────────────────────────────────

  it("passes an abort signal to every provider call", async () => {
    leaseReturning([pickRequest(1, "fp-1")]);
    let seenSignal: AbortSignal | undefined;
    llmMocks.executeLlmRequest.mockImplementation(async (_spec, options) => {
      seenSignal = options?.signal;
      return '{"choice":0}';
    });

    await collectLlmDraftResponses(PROFILE, [1], () => true);

    expect(seenSignal).toBeInstanceOf(AbortSignal);
    expect(seenSignal?.aborted).toBe(false);
  });

  it("contains a superseded round rather than letting it reach the provider", async () => {
    leaseReturning([pickRequest(1, "fp-1")]);
    llmMocks.executeLlmRequest.mockResolvedValue('{"choice":0}');

    // Two picks in flight: the second supersedes the first before the first
    // gets past its (awaited) request-building phase.
    const [first, second] = await Promise.all([
      collectLlmDraftResponses(PROFILE, [1], () => true),
      collectLlmDraftResponses(PROFILE, [1], () => true),
    ]);

    // The superseded round contributes nothing and never spends a request on a
    // pack that has moved on; only the current round does.
    expect(first).toEqual([]);
    expect(second).toHaveLength(1);
    expect(llmMocks.executeLlmRequest).toHaveBeenCalledTimes(1);
  });

  it("aborts calls already dispatched when a later round supersedes them", async () => {
    leaseReturning([pickRequest(1, "fp-1")]);
    const signals: AbortSignal[] = [];
    let release!: () => void;
    const hung = new Promise<void>((resolve) => {
      release = resolve;
    });
    let markDispatched!: () => void;
    const dispatched = new Promise<void>((resolve) => {
      markDispatched = resolve;
    });
    llmMocks.executeLlmRequest.mockImplementation(async (_spec, options) => {
      if (options?.signal) signals.push(options.signal);
      markDispatched();
      await hung;
      return '{"choice":0}';
    });

    const first = collectLlmDraftResponses(PROFILE, [1], () => true);
    // Wait for the call to actually be in flight rather than guessing at a
    // number of microtask ticks — request building awaits both the set catalog
    // and the engine lease before it dispatches.
    await dispatched;

    cancelLlmDraftRun();
    release();
    await first;

    // The dispatched call's socket is cut loose rather than left to run out its
    // 20-second timeout.
    expect(signals[0]?.aborted).toBe(true);
  });

  it("abandons a round explicitly cancelled by the draft store", async () => {
    leaseReturning([pickRequest(1, "fp-1")]);
    llmMocks.executeLlmRequest.mockImplementation(async () => {
      cancelLlmDraftRun();
      return '{"choice":0}';
    });

    // Replies that arrive after cancellation describe a pack that has passed.
    await expect(collectLlmDraftResponses(PROFILE, [1], () => true)).resolves.toEqual([]);
  });

  it("discards replies for a pick that went stale mid-flight", async () => {
    leaseReturning([pickRequest(1, "fp-1")]);
    llmMocks.executeLlmRequest.mockResolvedValue('{"choice":0}');
    let fresh = true;
    const stillCurrent = () => fresh;
    llmMocks.executeLlmRequest.mockImplementation(async () => {
      fresh = false;
      return '{"choice":0}';
    });

    await expect(collectLlmDraftResponses(PROFILE, [1], stillCurrent)).resolves.toEqual([]);
  });

  // ── Per-profile failure breaker ──────────────────────────────────────────

  it("gives up on a profile after consecutive failed rounds and stops calling it", async () => {
    leaseReturning([pickRequest(1, "fp-1")]);
    llmMocks.executeLlmRequest.mockRejectedValue(new Error("provider down"));

    for (let round = 0; round < 3; round += 1) {
      await collectLlmDraftResponses(PROFILE, [1], () => true);
    }
    expect(isLlmDraftDisabled(PROFILE.id)).toBe(true);

    const callsBefore = llmMocks.executeLlmRequest.mock.calls.length;
    await collectLlmDraftResponses(PROFILE, [1], () => true);

    // No further latency is spent on a provider the session has given up on.
    expect(llmMocks.executeLlmRequest.mock.calls.length).toBe(callsBefore);
  });

  it("resets the breaker on any successful round", async () => {
    leaseReturning([pickRequest(1, "fp-1")]);
    llmMocks.executeLlmRequest.mockRejectedValueOnce(new Error("blip"));
    await collectLlmDraftResponses(PROFILE, [1], () => true);

    llmMocks.executeLlmRequest.mockResolvedValue('{"choice":0}');
    await collectLlmDraftResponses(PROFILE, [1], () => true);

    llmMocks.executeLlmRequest.mockRejectedValue(new Error("blip"));
    await collectLlmDraftResponses(PROFILE, [1], () => true);
    await collectLlmDraftResponses(PROFILE, [1], () => true);

    // Two failures after a success is below the ceiling.
    expect(isLlmDraftDisabled(PROFILE.id)).toBe(false);
  });

  /// A pod with no eligible seat is not a provider failure and must not count
  /// toward giving up on the profile.
  it("does not blame the profile when the engine offers no seats", async () => {
    leaseReturning([]);

    for (let round = 0; round < 5; round += 1) {
      await collectLlmDraftResponses(PROFILE, [1], () => true);
    }

    expect(isLlmDraftDisabled(PROFILE.id)).toBe(false);
  });
});
