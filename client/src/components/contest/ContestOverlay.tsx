import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent, MouseEvent, PointerEvent } from "react";
import { useTranslation } from "react-i18next";

import type {
  ContestKind,
  ContestResult,
  ContestState,
  PlayerId,
} from "../../adapter/types.ts";
import { playContestCue } from "../../game/contestAudio.ts";
import { usePlayerId } from "../../hooks/usePlayerId.ts";
import { CONTEST_RESULT_BANNER_MS, useContestStore } from "../../stores/contestStore.ts";
import { useGameStore } from "../../stores/gameStore.ts";
import { getOpponentDisplayName } from "../../stores/multiplayerStore.ts";

/** Wall-clock cadence of the local countdown redraw. Cosmetic only. */
const COUNTDOWN_REDRAW_MS = 100;

function ContestGlyph({ kind }: { kind: ContestKind }) {
  // Decorative: the contest is always named in text as well.
  return kind === "Staring" ? (
    <svg viewBox="0 0 64 40" className="h-10 w-16" aria-hidden="true" focusable="false">
      <path
        d="M4 20C14 6 26 2 32 2s18 4 28 18C50 34 38 38 32 38S14 34 4 20Z"
        fill="none"
        stroke="currentColor"
        strokeWidth="3"
      />
      <circle cx="32" cy="20" r="9" fill="currentColor" />
    </svg>
  ) : (
    <svg viewBox="0 0 64 40" className="h-10 w-16" aria-hidden="true" focusable="false">
      <path
        d="M4 18C16 8 24 10 32 14c8-4 16-6 28 4-8 14-18 20-28 20S12 32 4 18Z"
        fill="none"
        stroke="currentColor"
        strokeWidth="3"
      />
      <path d="M8 19c16 4 32 4 48 0" fill="none" stroke="currentColor" strokeWidth="3" />
    </svg>
  );
}

/** Clicks the engine has counted for `player`, preferring the fresher live counter. */
function countedClicks(
  contest: ContestState,
  progress: Record<number, number>,
  player: PlayerId,
): number {
  const confirmed = contest.contestants.find((c) => c.player === player)?.clicks ?? 0;
  return Math.max(confirmed, progress[player] ?? 0);
}

/** Display-only: the localized sentence for a settled contest. */
function useResultText(result: ContestResult, me: PlayerId): string {
  const { t } = useTranslation("game");
  if ("Cancelled" in result) return t("physicalContest.result.cancelled");
  if (result.Outcome === "Tie") return t("physicalContest.result.tie");
  const { winner } = result.Outcome.Decided;
  if (winner === me) return t("physicalContest.result.won");
  if (result.Outcome.Decided.loser === me) return t("physicalContest.result.lost");
  return t("physicalContest.result.winner", { name: getOpponentDisplayName(winner) });
}

function resultCue(result: ContestResult, me: PlayerId) {
  if ("Cancelled" in result || result.Outcome === "Tie") return "neutral" as const;
  return result.Outcome.Decided.winner === me ? ("win" as const) : ("lose" as const);
}

/**
 * Eye to Eye / Mouth to Mouth. The Comprehensive Rules define no physical
 * contests, so an online game adapts them: both players keep clicking and the
 * first to stop for the inactivity timeout loses. The engine/host owns the
 * rules; this overlay renders the engine's contest state and relays clicks.
 */
function ContestPanel({ contest }: { contest: ContestState }) {
  const { t } = useTranslation("game");
  const me = usePlayerId();
  const adapter = useGameStore((s) => s.adapter);
  const progress = useContestStore((s) => s.progress);
  const mySlot = contest.contestants.findIndex((c) => c.player === me);
  const isParticipant = mySlot >= 0;
  const paused = contest.paused_since_ms != null;
  const isActive = contest.phase === "Active";
  const isPreparing = contest.phase === "Pending" || contest.phase === "Preparing";

  // Highest sequence used locally; the next click is strictly above both it and
  // what the engine has confirmed, so duplicate/stale protection never rejects a
  // genuine click even across back-to-back contests.
  const seqRef = useRef(0);
  const [pulse, setPulse] = useState(0);
  const click = useCallback(() => {
    if (!isParticipant || !isActive || !adapter) return;
    const confirmed = contest.contestants[mySlot]?.last_seq ?? 0;
    seqRef.current = Math.max(seqRef.current, confirmed) + 1;
    if (adapter.submitContestClick) {
      // A server hosts the contest and stamps the click itself.
      adapter.submitContestClick(seqRef.current);
    } else if (adapter.driveContest) {
      // A local engine hosts it: stamp with this client's own clock, which is the
      // timebase the local driver advances the engine in.
      useContestStore
        .getState()
        .bufferClick({ player: me, seq: seqRef.current, at_ms: performance.now() });
    } else {
      return;
    }
    setPulse((n) => n + 1);
  }, [adapter, contest.contestants, isActive, isParticipant, me, mySlot]);

  // Countdown: milliseconds until GO as of this snapshot, drawn down by local
  // elapsed time. The host's clock is never compared against ours.
  const snapshotAt = useMemo(
    () => ({ at: performance.now(), clock: contest.clock_ms }),
    [contest.clock_ms],
  );
  const [now, setNow] = useState(() => performance.now());
  useEffect(() => {
    if (!isPreparing || paused) return;
    const id = window.setInterval(() => setNow(performance.now()), COUNTDOWN_REDRAW_MS);
    return () => window.clearInterval(id);
  }, [isPreparing, paused]);
  const untilGoMs =
    contest.go_at_ms == null
      ? contest.config.countdown_ms
      : Math.max(
          0,
          contest.go_at_ms -
            snapshotAt.clock -
            (paused ? 0 : Math.max(0, now - snapshotAt.at)),
        );
  const countdown = Math.ceil(untilGoMs / 1000);

  // Audio cues on phase edges and each whole countdown second.
  const lastCountdown = useRef<number | null>(null);
  useEffect(() => {
    if (contest.phase === "Preparing" && countdown > 0 && lastCountdown.current !== countdown) {
      lastCountdown.current = countdown;
      playContestCue("tick");
    }
  }, [contest.phase, countdown]);
  useEffect(() => {
    if (isActive) playContestCue("go");
  }, [isActive]);

  const onPointerDown = (event: PointerEvent<HTMLButtonElement>) => {
    if (event.pointerType === "mouse" && event.button !== 0) return;
    event.preventDefault();
    click();
  };
  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key !== " " && event.key !== "Enter") return;
    event.preventDefault();
    // A held key auto-repeats; one press is one click.
    if (!event.repeat) click();
  };
  // Assistive-technology activation arrives as a click with no pointer or key
  // event (`detail === 0`); pointer and keyboard activation are handled above.
  const onClick = (event: MouseEvent<HTMLButtonElement>) => {
    if (event.detail === 0) click();
  };

  const [first, second] = contest.contestants;
  const name = (player: PlayerId) =>
    player === me ? t("physicalContest.you") : getOpponentDisplayName(player);
  const status = paused
    ? t("physicalContest.status.paused")
    : !isParticipant
      ? t("physicalContest.status.spectating")
      : contest.phase === "Pending"
        ? t("physicalContest.status.pending")
        : contest.phase === "Preparing"
          ? t("physicalContest.status.preparing")
          : contest.phase === "Active"
            ? t("physicalContest.status.go")
            : t("physicalContest.status.resolving");

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 p-4"
      role="dialog"
      aria-modal="true"
      aria-labelledby="contest-title"
    >
      <div className="flex w-full max-w-md flex-col items-center gap-4 rounded-2xl border border-white/20 bg-slate-900 p-5 text-center text-white shadow-2xl">
        <ContestGlyph kind={contest.kind} />
        <h2 id="contest-title" className="text-xl font-semibold">
          {contest.kind === "Staring"
            ? t("physicalContest.title.staring")
            : t("physicalContest.title.breathHolding")}
        </h2>
        <p className="text-sm text-slate-300">
          {t("physicalContest.versus", {
            first: name(first.player),
            second: name(second.player),
          })}
        </p>
        <p className="text-sm text-slate-300">
          {t("physicalContest.instructions", {
            seconds: contest.config.inactivity_timeout_ms / 1000,
          })}
        </p>

        {/* Announced to assistive tech on each phase change, never per click. */}
        <p role="status" aria-live="polite" className="text-lg font-bold">
          {status}
        </p>
        {isPreparing && !paused && (
          <p className="text-6xl font-black tabular-nums" aria-hidden="true">
            {countdown}
          </p>
        )}

        {isParticipant && (
          <button
            type="button"
            aria-disabled={!isActive}
            onPointerDown={onPointerDown}
            onKeyDown={onKeyDown}
            onClick={onClick}
            className={[
              "min-h-32 w-full touch-manipulation select-none rounded-2xl border-4 px-6 text-2xl font-black",
              "focus-visible:outline-4 focus-visible:outline-offset-4 focus-visible:outline-white",
              isActive
                ? "border-white bg-emerald-600 active:bg-emerald-500"
                : "cursor-not-allowed border-dashed border-slate-500 bg-slate-800 text-slate-400",
            ].join(" ")}
          >
            <span key={pulse} className="inline-block motion-safe:animate-pulse-once">
              {isActive ? t("physicalContest.button.active") : t("physicalContest.button.wait")}
            </span>
          </button>
        )}

        <ul className="flex w-full justify-around text-base" aria-label={t("physicalContest.versus", {
          first: name(first.player),
          second: name(second.player),
        })}>
          {[first, second].map((contestant) => (
            <li key={contestant.player} className="tabular-nums">
              {t("physicalContest.clicks", {
                name: name(contestant.player),
                count: countedClicks(contest, progress, contestant.player),
              })}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

/** Banner announcing a settled contest to both players after the engine moves on. */
function ContestResultBanner() {
  const announcement = useContestStore((s) => s.announcement);
  const dismiss = useContestStore((s) => s.dismissAnnouncement);
  const me = usePlayerId();
  const id = announcement?.id;
  useEffect(() => {
    if (id == null) return;
    const timer = window.setTimeout(() => dismiss(id), CONTEST_RESULT_BANNER_MS);
    return () => window.clearTimeout(timer);
  }, [id, dismiss]);
  useEffect(() => {
    if (announcement) playContestCue(resultCue(announcement.result, me));
    // Cue once per announcement.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [announcement?.id]);
  if (!announcement) return null;
  return <ResultBannerBody announcement={announcement} me={me} />;
}

function ResultBannerBody({
  announcement,
  me,
}: {
  announcement: NonNullable<ReturnType<typeof useContestStore.getState>["announcement"]>;
  me: PlayerId;
}) {
  const text = useResultText(announcement.result, me);
  return (
    <div
      role="status"
      aria-live="polite"
      className="pointer-events-none fixed inset-x-0 top-24 z-50 flex justify-center px-4"
    >
      <p className="rounded-xl border border-white/30 bg-slate-900/95 px-5 py-3 text-lg font-bold text-white shadow-xl">
        {text}
      </p>
    </div>
  );
}

/** Interval between local-host ticks. Short against the contest's 2s inactivity timeout. */
const LOCAL_HOST_TICK_MS = 50;

/**
 * Local engines (single-player WASM, P2P host) have no server to host the running
 * contest, so this client drives it: every tick it hands the engine its own monotonic
 * clock, the clicks collected since the last tick, and the seats the engine clicks
 * for. The engine decides everything; this only supplies time and input, then
 * commits the resulting snapshot. Server transports expose no `driveContest`.
 */
function useLocalContestHost(active: boolean) {
  const adapter = useGameStore((s) => s.adapter);
  const me = usePlayerId();
  useEffect(() => {
    const drive = adapter?.driveContest?.bind(adapter);
    if (!active || !adapter || !drive) return;
    let stopped = false;
    let inFlight = false;
    const tick = async () => {
      if (stopped || inFlight) return;
      inFlight = true;
      try {
        const clicks = useContestStore.getState().drainClicks();
        const players = useGameStore.getState().gameState?.players ?? [];
        const aiPlayers = players.map((p) => p.id).filter((id) => id !== me);
        const result = await drive(performance.now(), aiPlayers, clicks);
        if (stopped || (result.step !== "Updated" && result.step !== "Settled")) return;
        const snapshot = await adapter.getSnapshot();
        useGameStore.getState().commitEngineSnapshot(snapshot, {
          events: result.events,
          logEntries: result.log_entries,
        });
      } finally {
        inFlight = false;
      }
    };
    const timer = window.setInterval(() => void tick(), LOCAL_HOST_TICK_MS);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [active, adapter, me]);
}

/**
 * Mount once in the game page: renders the running contest, then announces its
 * result. Records the engine's `PhysicalContestResolved` event so the result
 * outlives the `WaitingFor` that carried it.
 */
export function ContestOverlay() {
  const waitingFor = useGameStore((s) => s.waitingFor);
  const events = useGameStore((s) => s.events);
  const announce = useContestStore((s) => s.announce);
  useLocalContestHost(waitingFor?.type === "PhysicalContest");

  useEffect(() => {
    for (const event of events) {
      if (event.type === "PhysicalContestResolved") {
        announce(event.data.kind, event.data.participants, event.data.result);
      }
    }
  }, [events, announce]);

  return (
    <>
      {waitingFor?.type === "PhysicalContest" && (
        <ContestPanel contest={waitingFor.data.contest} />
      )}
      <ContestResultBanner />
    </>
  );
}
