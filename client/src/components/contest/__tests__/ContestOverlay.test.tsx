import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type {
  ContestPhase,
  ContestResult,
  ContestState,
  EngineAdapter,
  GameEvent,
} from "../../../adapter/types.ts";
import { isWaitingForHandled } from "../../../game/waitingForRegistry.ts";
import { useContestStore } from "../../../stores/contestStore.ts";
import { useGameStore } from "../../../stores/gameStore.ts";
import { useMultiplayerStore } from "../../../stores/multiplayerStore.ts";
import { ContestOverlay } from "../ContestOverlay.tsx";

function contestState(phase: ContestPhase, overrides: Partial<ContestState> = {}): ContestState {
  return {
    kind: "Staring",
    phase,
    config: {
      countdown_ms: 3000,
      inactivity_timeout_ms: 2000,
      reconnect_grace_ms: 10000,
      settle_window_ms: 150,
      max_active_ms: 300000,
    },
    contestants: [
      { player: 0, clicks: 0, last_activity_ms: 0, last_seq: null, connection: "Connected" },
      { player: 1, clicks: 0, last_activity_ms: 0, last_seq: null, connection: "Connected" },
    ],
    clock_ms: 1000,
    go_at_ms: phase === "Active" ? 1000 : 4000,
    ...overrides,
  };
}

function showContest(contest: ContestState, adapter: Partial<EngineAdapter> | null) {
  useGameStore.setState({
    gameMode: "ai",
    adapter: adapter as EngineAdapter | null,
    events: [],
    waitingFor: { type: "PhysicalContest", data: { contest, source_id: 7 } },
  });
}

beforeEach(() => {
  useContestStore.getState().reset();
  useMultiplayerStore.setState({ playerNames: new Map([[1, "Alice"]]) });
});

afterEach(() => {
  cleanup();
  useGameStore.setState({ waitingFor: null, adapter: null, events: [] });
});

describe("ContestOverlay", () => {
  it("is a registered, handled waiting state", () => {
    expect(
      isWaitingForHandled({
        type: "PhysicalContest",
        data: { contest: contestState("Pending"), source_id: 1 },
      }),
    ).toBe(true);
  });

  it("names the contest, explains the rule and shows both contestants", () => {
    showContest(contestState("Preparing"), { submitContestClick: vi.fn() });
    render(<ContestOverlay />);

    expect(screen.getByRole("heading", { name: "Staring Contest" })).toBeInTheDocument();
    expect(screen.getByText(/The first player to stop for 2 seconds loses/)).toBeInTheDocument();
    expect(screen.getByText("You vs. Alice")).toBeInTheDocument();
    expect(screen.getByText("You: 0")).toBeInTheDocument();
    expect(screen.getByText("Alice: 0")).toBeInTheDocument();
  });

  it("uses the breath-holding wording for Mouth to Mouth", () => {
    showContest(contestState("Preparing", { kind: "BreathHolding" }), { submitContestClick: vi.fn() });
    render(<ContestOverlay />);
    expect(screen.getByRole("heading", { name: "Breath-Holding Contest" })).toBeInTheDocument();
  });

  it("does not register clicks before GO and tells the player to wait", () => {
    const submitContestClick = vi.fn();
    showContest(contestState("Preparing"), { submitContestClick });
    render(<ContestOverlay />);

    const button = screen.getByRole("button", { name: "Wait for GO…" });
    expect(button).toHaveAttribute("aria-disabled", "true");
    fireEvent.pointerDown(button);
    fireEvent.keyDown(button, { key: " " });
    expect(submitContestClick).not.toHaveBeenCalled();
    expect(screen.getByRole("status")).toHaveTextContent("Get ready…");
  });

  it("submits each pointer press with a strictly increasing sequence number once Active", () => {
    const submitContestClick = vi.fn();
    showContest(contestState("Active"), { submitContestClick });
    render(<ContestOverlay />);

    const button = screen.getByRole("button", { name: "Keep clicking!" });
    fireEvent.pointerDown(button);
    fireEvent.pointerDown(button);
    fireEvent.pointerDown(button);

    expect(submitContestClick.mock.calls.map(([seq]) => seq)).toEqual([1, 2, 3]);
    expect(screen.getByText("GO! Keep clicking!")).toBeInTheDocument();
  });

  it("continues numbering above what the engine has already confirmed", () => {
    const submitContestClick = vi.fn();
    const contest = contestState("Active");
    contest.contestants[0].last_seq = 41;
    showContest(contest, { submitContestClick });
    render(<ContestOverlay />);

    fireEvent.pointerDown(screen.getByRole("button", { name: "Keep clicking!" }));
    expect(submitContestClick).toHaveBeenCalledWith(42);
  });

  it("supports keyboard activation, counts a press once and ignores key auto-repeat", () => {
    const submitContestClick = vi.fn();
    showContest(contestState("Active"), { submitContestClick });
    render(<ContestOverlay />);

    const button = screen.getByRole("button", { name: "Keep clicking!" });
    fireEvent.keyDown(button, { key: " " });
    fireEvent.keyDown(button, { key: " ", repeat: true });
    fireEvent.keyDown(button, { key: "Enter" });
    fireEvent.keyDown(button, { key: "a" });

    expect(submitContestClick).toHaveBeenCalledTimes(2);
  });

  it("counts an assistive-technology activation (click with no pointer or key event)", () => {
    const submitContestClick = vi.fn();
    showContest(contestState("Active"), { submitContestClick });
    render(<ContestOverlay />);

    const button = screen.getByRole("button", { name: "Keep clicking!" });
    fireEvent.click(button, { detail: 0 });
    // A mouse click is already counted by its pointerdown; its click event must not double-count.
    fireEvent.click(button, { detail: 1 });

    expect(submitContestClick).toHaveBeenCalledTimes(1);
  });

  it("buffers stamped clicks for a local host when no server transport exists", () => {
    const driveContest = vi.fn();
    showContest(contestState("Active"), { driveContest });
    render(<ContestOverlay />);

    fireEvent.pointerDown(screen.getByRole("button", { name: "Keep clicking!" }));

    const [click] = useContestStore.getState().pendingClicks;
    expect(click).toMatchObject({ player: 0, seq: 1 });
    expect(typeof click.at_ms).toBe("number");
  });

  it("shows live counters pushed between full states", () => {
    showContest(contestState("Active"), { submitContestClick: vi.fn() });
    render(<ContestOverlay />);

    act(() => useContestStore.getState().setProgress([[0, 12], [1, 9]]));

    expect(screen.getByText("You: 12")).toBeInTheDocument();
    expect(screen.getByText("Alice: 9")).toBeInTheDocument();
  });

  it("announces a pause while a seat reconnects and never lets spectators click", () => {
    const contest = contestState("Active", { paused_since_ms: 1500 });
    contest.contestants = [
      { ...contest.contestants[0], player: 5 },
      { ...contest.contestants[1], player: 6 },
    ];
    showContest(contest, { submitContestClick: vi.fn() });
    render(<ContestOverlay />);

    expect(screen.getByRole("status")).toHaveTextContent("Contest paused while a player reconnects…");
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it.each<[string, ContestResult, string]>([
    ["a win", { Outcome: { Decided: { winner: 0, loser: 1, reason: "Inactivity" } } }, "You won the contest!"],
    ["a loss", { Outcome: { Decided: { winner: 1, loser: 0, reason: "Inactivity" } } }, "You lost the contest."],
    ["a tie", { Outcome: "Tie" }, "It's a tie — nothing happens."],
    ["a cancellation", { Cancelled: "BothDisconnected" }, "The contest was cancelled — nothing happens."],
  ])("announces %s after the engine settles it", (_name, result, text) => {
    useGameStore.setState({ gameMode: "ai", waitingFor: { type: "Priority", data: { player: 0 } } });
    render(<ContestOverlay />);

    const event: GameEvent = {
      type: "PhysicalContestResolved",
      data: { kind: "Staring", participants: [0, 1], result },
    };
    act(() => useGameStore.setState({ events: [event] }));

    expect(screen.getByText(text)).toBeInTheDocument();
  });
});
