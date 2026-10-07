import { beforeEach, describe, expect, it } from "vitest";

import { useMultiplayerStore } from "../../../stores/multiplayerStore";
import { resolvedEndpointOf } from "../endpoint";
import type { LlmProfile } from "../types";

function profile(patch: Partial<LlmProfile>): LlmProfile {
  return {
    id: "p",
    name: "",
    provider: "OpenAi",
    baseUrl: null,
    apiKey: "k",
    model: "m",
    maxOutputTokens: null,
    temperature: null,
    enabled: true,
    ...patch,
  };
}

describe("resolvedEndpointOf", () => {
  beforeEach(() => {
    useMultiplayerStore.setState({ hostingServer: "wss://phase.example/ws" });
  });

  it("points a Jev profile at the hosting server's relay origin", () => {
    expect(resolvedEndpointOf(profile({ provider: "Jev" })).baseUrl).toBe(
      "https://phase.example",
    );
  });

  it("lets a Jev profile's own endpoint win", () => {
    expect(
      resolvedEndpointOf(profile({ provider: "Jev", baseUrl: "https://mine.example" })).baseUrl,
    ).toBe("https://mine.example");
  });

  it("leaves a Jev profile unresolved when no server address can be derived", () => {
    useMultiplayerStore.setState({ hostingServer: "not a url" });
    expect(resolvedEndpointOf(profile({ provider: "Jev" })).baseUrl).toBeNull();
  });

  it("passes every other provider through as saved", () => {
    expect(resolvedEndpointOf(profile({ provider: "OpenAi" })).baseUrl).toBeNull();
    expect(
      resolvedEndpointOf(profile({ provider: "OpenAiCompatible", baseUrl: "http://localhost:1/v1" }))
        .baseUrl,
    ).toBe("http://localhost:1/v1");
  });
});
