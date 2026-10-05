import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { CardAnimationStyle } from "../../../animation/types.ts";
import { CARD_ANIMATION_PREVIEW_MOMENTS, momentIndexAt } from "../cardAnimationPreview.ts";
import { CardAnimationStylePicker } from "../CardAnimationStylePicker.tsx";

function renderPicker(value: CardAnimationStyle = "webgl") {
  const onChange = vi.fn();
  const view = render(<CardAnimationStylePicker value={value} onChange={onChange} />);
  const group = screen.getByRole("radiogroup", { name: "Card Animations" });
  const videos = Array.from(view.container.querySelectorAll("video"));
  return { onChange, group, videos };
}

describe("CardAnimationStylePicker", () => {
  let play: ReturnType<typeof vi.spyOn>;
  let pause: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue(undefined);
    pause = vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("exposes the two styles as a radio group and selects on click", () => {
    const { onChange, group } = renderPicker("webgl");

    expect(screen.getByRole("radio", { name: "New" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "Classic" })).not.toBeChecked();

    fireEvent.click(screen.getByRole("radio", { name: "Classic" }));

    expect(onChange).toHaveBeenCalledWith("classic");
    expect(group).toBeInTheDocument();
  });

  it("does not play anything on click alone", () => {
    renderPicker();

    fireEvent.click(screen.getByRole("radio", { name: "Classic" }));

    expect(play).not.toHaveBeenCalled();
  });

  it("plays both clips in lockstep while hovered, then pauses on the moment's peak frame", () => {
    const { group, videos } = renderPicker();
    expect(videos).toHaveLength(2);
    expect(videos.every((video) => video.loop && video.muted)).toBe(true);

    fireEvent.pointerEnter(group, { pointerType: "mouse" });

    expect(play).toHaveBeenCalledTimes(2);
    expect(videos.map((video) => video.currentTime)).toEqual([0, 0]);

    fireEvent.pointerLeave(group, { pointerType: "mouse" });

    expect(pause).toHaveBeenCalledTimes(2);
    expect(videos.map((video) => video.currentTime)).toEqual([
      CARD_ANIMATION_PREVIEW_MOMENTS[0].peak,
      CARD_ANIMATION_PREVIEW_MOMENTS[0].peak,
    ]);
  });

  it("keeps playing, without restarting, while the pointer stays", () => {
    const { group } = renderPicker();

    fireEvent.pointerEnter(group, { pointerType: "mouse" });
    fireEvent.focus(screen.getByRole("radio", { name: "New" }));

    expect(play).toHaveBeenCalledTimes(2);
  });

  it("labels the spell on screen as the loop moves between moments", () => {
    const { group, videos } = renderPicker();
    const [first, second] = CARD_ANIMATION_PREVIEW_MOMENTS;
    expect(screen.getByText(first.spell)).toBeInTheDocument();

    fireEvent.pointerEnter(group, { pointerType: "mouse" });
    act(() => {
      videos[0].currentTime = second.start + 0.5;
      fireEvent.timeUpdate(videos[0]);
    });

    expect(screen.getByText(second.spell)).toBeInTheDocument();
  });

  it("re-syncs the Classic clip when it drifts from the New clip", () => {
    const { group, videos } = renderPicker();

    fireEvent.pointerEnter(group, { pointerType: "mouse" });
    videos[0].currentTime = 3;
    videos[1].currentTime = 2;
    fireEvent.timeUpdate(videos[0]);

    expect(videos[1].currentTime).toBe(3);
  });

  it("previews on keyboard focus and stops when focus leaves the group", () => {
    const { videos } = renderPicker();
    const tile = screen.getByRole("radio", { name: "New" });

    fireEvent.focus(tile);
    expect(play).toHaveBeenCalledTimes(2);

    fireEvent.blur(tile, { relatedTarget: document.body });
    expect(pause).toHaveBeenCalledTimes(2);
    expect(videos[0].paused).toBe(true);
  });

  it("toggles the preview on a touch tap, since touch has no hover", () => {
    const { group } = renderPicker();

    fireEvent.pointerEnter(group, { pointerType: "touch" });
    expect(play).not.toHaveBeenCalled();

    fireEvent.pointerDown(group, { pointerType: "touch" });
    expect(play).toHaveBeenCalledTimes(2);

    fireEvent.pointerDown(group, { pointerType: "touch" });
    expect(pause).toHaveBeenCalledTimes(2);
  });

  it("moves the selection with the arrow keys", () => {
    const { onChange } = renderPicker("webgl");

    fireEvent.keyDown(screen.getByRole("radio", { name: "New" }), { key: "ArrowRight" });
    expect(onChange).toHaveBeenLastCalledWith("classic");

    fireEvent.keyDown(screen.getByRole("radio", { name: "New" }), { key: "ArrowLeft" });
    expect(onChange).toHaveBeenLastCalledWith("classic");
  });

  it("keeps only the selected tile in the tab order", () => {
    renderPicker("classic");

    expect(screen.getByRole("radio", { name: "Classic" })).toHaveAttribute("tabindex", "0");
    expect(screen.getByRole("radio", { name: "New" })).toHaveAttribute("tabindex", "-1");
  });
});

describe("momentIndexAt", () => {
  it("returns the last moment that has started", () => {
    const [, second, third] = CARD_ANIMATION_PREVIEW_MOMENTS;

    expect(momentIndexAt(0)).toBe(0);
    expect(momentIndexAt(second.start - 0.01)).toBe(0);
    expect(momentIndexAt(second.start)).toBe(1);
    expect(momentIndexAt(third.start + 1)).toBe(2);
  });
});
