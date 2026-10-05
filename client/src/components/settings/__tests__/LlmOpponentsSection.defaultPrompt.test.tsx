// A persisted zustand store captures its storage when this file's imports are
// evaluated, so the working-localStorage install has to precede them.
import "../../../test/helpers/persistedStorage";

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../services/llm/catalog", () => ({ loadProviderCatalog: async () => [] }));
const probe = vi.hoisted(() => ({
  testLlmEndpoint: vi.fn<() => Promise<{ ok: true }>>(async () => ({ ok: true })),
}));
vi.mock("../../../services/llm/probe", () => probe);

import { LlmOpponentsSection } from "../LlmOpponentsSection";
import { profileForSeat, useLlmStore } from "../../../stores/llmStore";

const PROMPT = /use this provider as your default opponent\?/i;
const CONNECTED_PROMPT = /connected\. use this as your default opponent\?/i;

afterEach(cleanup);

beforeEach(() => {
  useLlmStore.setState({
    profiles: [],
    seatBindings: {},
    defaultOpponentProfileId: null,
    draftEnabled: false,
    draftProfileId: null,
  });
  probe.testLlmEndpoint.mockClear();
});

function addProfile(patch: Parameters<ReturnType<typeof useLlmStore.getState>["addProfile"]>[0]) {
  return useLlmStore.getState().addProfile({
    name: "Claude",
    provider: "Anthropic",
    model: "claude-sonnet-5",
    apiKey: "sk-test",
    enabled: true,
    ...patch,
  });
}

describe("default-opponent prompt on a provider card", () => {
  it("is offered for a usable provider, and one click makes it the default", () => {
    const id = addProfile({});
    render(<LlmOpponentsSection />);

    fireEvent.click(screen.getByRole("button", { name: /use as default/i }));

    expect(useLlmStore.getState().defaultOpponentProfileId).toBe(id);
    expect(profileForSeat(useLlmStore.getState(), 0)?.id).toBe(id);
  });

  it("collapses to a badge once the provider is the default", () => {
    const id = addProfile({});
    useLlmStore.getState().setDefaultOpponentProfileId(id);
    render(<LlmOpponentsSection />);

    expect(screen.queryByText(PROMPT)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /use as default/i })).not.toBeInTheDocument();
    expect(screen.getByRole("status", { name: "" })).toHaveTextContent(/default opponent/i);
  });

  it("is not offered for a provider the game would ignore", () => {
    // Keys are memory-only, so a reloaded profile has none: it cannot be used,
    // and inviting the player to default to it would set up a silent no-op.
    addProfile({ provider: "OpenAi", apiKey: "" });
    render(<LlmOpponentsSection />);

    expect(screen.queryByText(PROMPT)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /use as default/i })).not.toBeInTheDocument();
  });

  it("is not offered for a disabled provider", () => {
    addProfile({ enabled: false });
    render(<LlmOpponentsSection />);

    expect(screen.queryByRole("button", { name: /use as default/i })).not.toBeInTheDocument();
  });

  it("acknowledges a passing connection test in its wording", async () => {
    addProfile({});
    render(<LlmOpponentsSection />);
    expect(screen.getByText(PROMPT)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /test connection/i }));

    await waitFor(() => expect(screen.getByText(CONNECTED_PROMPT)).toBeInTheDocument());
    expect(probe.testLlmEndpoint).toHaveBeenCalledTimes(1);
  });
});
