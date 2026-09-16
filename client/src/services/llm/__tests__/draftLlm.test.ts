import { beforeEach, describe, expect, it, vi } from "vitest";

const llmMocks = vi.hoisted(() => ({
  executeLlmRequest: vi.fn<() => Promise<string>>(),
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
import { botSeatIndices, collectLlmDraftResponses, reportLlmDraftOutcomes } from "../draftLlm";
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
});
