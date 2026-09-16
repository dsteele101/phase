// A persisted zustand store captures its storage when this file's imports are
// evaluated, so the working-localStorage install has to precede them.
import "../../test/helpers/persistedStorage";

import { beforeEach, describe, expect, it, vi } from "vitest";

import { LLM_ENDPOINTS_KEY } from "../../constants/storage";

/**
 * Hydration happens during module construction, so every case here needs a
 * FRESH module evaluation against storage that is already populated. That is
 * the path where persist may invoke `onRehydrateStorage` synchronously — and
 * where a callback touching the store binding would hit its temporal dead zone.
 */
function seed(state: unknown): void {
  localStorage.setItem(LLM_ENDPOINTS_KEY, JSON.stringify({ state, version: 0 }));
}

async function freshStore() {
  vi.resetModules();
  return import("../llmStore");
}

beforeEach(() => {
  vi.resetModules();
  localStorage.clear();
});

describe("persisted store hydration", () => {
  it("constructs against an already-populated synchronous storage", async () => {
    seed({
      profiles: [
        {
          id: "p1",
          name: "Claude",
          provider: "Anthropic",
          baseUrl: null,
          model: "claude-sonnet-5",
          maxOutputTokens: null,
          temperature: null,
          enabled: true,
        },
      ],
      seatBindings: { 0: "p1" },
      draftEnabled: false,
      draftProfileId: null,
    });

    // The regression: importing the module must not throw while persist is
    // hydrating. A store-binding reference inside the hydration callback would
    // raise "Cannot access 'useLlmStore' before initialization" here.
    const { useLlmStore } = await freshStore();

    expect(useLlmStore.getState().profiles).toHaveLength(1);
    expect(useLlmStore.getState().seatBindings).toEqual({ 0: "p1" });
  });

  it("rehydrates a stored profile without its credential", async () => {
    seed({
      profiles: [
        {
          id: "p1",
          name: "Legacy",
          provider: "OpenAi",
          baseUrl: null,
          apiKey: "sk-from-an-older-build",
          model: "gpt-5",
          maxOutputTokens: null,
          temperature: null,
          enabled: true,
        },
      ],
      seatBindings: {},
      draftEnabled: false,
      draftProfileId: null,
    });

    const { useLlmStore } = await freshStore();

    const profile = useLlmStore.getState().profiles.find((p) => p.id === "p1");
    expect(profile?.apiKey).toBe("");
    expect(profile?.name).toBe("Legacy");
  });

  it("scrubs the stored credential from disk once construction has settled", async () => {
    seed({
      profiles: [
        {
          id: "p1",
          name: "Legacy",
          provider: "OpenAi",
          baseUrl: null,
          apiKey: "sk-from-an-older-build",
          model: "gpt-5",
          maxOutputTokens: null,
          temperature: null,
          enabled: true,
        },
      ],
      seatBindings: {},
      draftEnabled: false,
      draftProfileId: null,
    });

    await freshStore();
    // The scrub is deferred to a microtask so it cannot run during
    // construction; let that drain.
    await Promise.resolve();
    await Promise.resolve();

    expect(localStorage.getItem(LLM_ENDPOINTS_KEY)).not.toContain("sk-from-an-older-build");
  });

  it("constructs cleanly when storage holds nothing at all", async () => {
    const { useLlmStore } = await freshStore();
    expect(useLlmStore.getState().profiles).toEqual([]);
  });

  it("constructs cleanly when storage holds an unreadable record", async () => {
    localStorage.setItem(LLM_ENDPOINTS_KEY, "{not json");
    const { useLlmStore } = await freshStore();
    expect(useLlmStore.getState().profiles).toEqual([]);
  });
});
