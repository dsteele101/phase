import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

import { DraftIntro } from "../DraftIntro";

afterEach(cleanup);

describe("DraftIntro", () => {
  it("shows the draft's configured pack count and pack size", () => {
    render(
      <DraftIntro
        mode="quick"
        packCount={4}
        cardsPerPack={12}
        onContinue={vi.fn()}
      />,
    );

    expect(screen.getByText("You'll open 4 packs of 12 cards each")).toBeInTheDocument();
  });

  it("lists every booster's size when a multi-set draft mixes them", () => {
    render(
      <DraftIntro
        mode="quick"
        packCount={3}
        cardsPerPack={15}
        packSizes={[15, 14, 15]}
        onContinue={vi.fn()}
      />,
    );

    expect(
      screen.getByText("You'll open 3 packs of mixed sizes — 15, 14, 15 cards, in that order"),
    ).toBeInTheDocument();
  });

  it("keeps the single-size line when every booster agrees", () => {
    render(
      <DraftIntro
        mode="quick"
        packCount={3}
        cardsPerPack={15}
        packSizes={[15, 15, 15]}
        onContinue={vi.fn()}
      />,
    );

    expect(screen.getByText("You'll open 3 packs of 15 cards each")).toBeInTheDocument();
  });
});
