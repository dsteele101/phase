import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

import type { DraftProgressFields } from "../../../adapter/draft-adapter";
import { DraftProgress } from "../DraftProgress";

afterEach(cleanup);

function progressView(overrides: Partial<DraftProgressFields> = {}): DraftProgressFields {
  return {
    current_pack_number: 0,
    pick_number: 0,
    cards_per_pack: 15,
    pack_sizes: [15, 15, 15],
    pack_set_codes: ["ISD", "ISD", "ISD"],
    pack_count: 3,
    pass_direction: "Left",
    ...overrides,
  };
}

/** The pick pips the bar draws, per pack, in pack order. */
function pipCountsPerPack(container: HTMLElement): number[] {
  return [...container.querySelectorAll("div.gap-px")].map(
    (segment) => segment.children.length,
  );
}

describe("DraftProgress", () => {
  it("draws each booster at the size the engine reported for it", () => {
    const { container } = render(
      <DraftProgress
        view={progressView({
          pack_sizes: [15, 14, 20],
          pack_set_codes: ["ISD", "BLB", "CMR"],
        })}
      />,
    );

    expect(pipCountsPerPack(container)).toEqual([15, 14, 20]);
  });

  it("counts picks against the booster in play, not the first one", () => {
    render(
      <DraftProgress
        view={progressView({
          current_pack_number: 1,
          pick_number: 3,
          cards_per_pack: 14,
          pack_sizes: [15, 14, 15],
          pack_set_codes: ["ISD", "BLB", "ISD"],
        })}
      />,
    );

    expect(screen.getByText("4")).toBeInTheDocument();
    expect(screen.getByText("/14")).toBeInTheDocument();
  });

  it("names each booster's set when the draft mixes sets", () => {
    render(
      <DraftProgress
        view={progressView({
          current_pack_number: 1,
          pack_set_codes: ["ISD", "DKA", "AVR"],
        })}
      />,
    );

    expect(screen.getByText("DKA")).toBeInTheDocument();
    expect(screen.getByText("AVR")).toBeInTheDocument();
  });

  it("stays unlabelled when every booster comes from the same set", () => {
    render(<DraftProgress view={progressView()} />);

    expect(screen.queryByText("ISD")).not.toBeInTheDocument();
  });
});
