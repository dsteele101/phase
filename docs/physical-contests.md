# Physical contests (Eye to Eye, Mouth to Mouth)

Two Unhinged cards ask players to do something physical:

| Card | Oracle text |
|------|-------------|
| Eye to Eye | You and target creature's controller have a staring contest. If you win, destroy that creature. |
| Mouth to Mouth | Choose target opponent and target creature they control. You and that player have a breath-holding contest. If you win, you gain control of that creature. |

The Comprehensive Rules define no physical contests, so the contest itself carries no CR number. The surrounding rules it plugs into (CR 608.2c "if you win", CR 109.4, CR 115) are annotated where they are implemented.

An online game cannot ask anyone to stare or hold their breath, so both cards use one **sustained-clicking contest**: both players keep activating a control, and the first player to stop for longer than the inactivity timeout loses.

> The Oracle text says "breath-holding contest" for Mouth to Mouth. The adaptation spec called it a "mouth-to-mouth contest"; the Oracle wording is the one rendered in the UI.

## Where things live

| Layer | Location | Owns |
|-------|----------|------|
| Data model | `crates/engine/src/types/contest.rs` | `ContestState`, lifecycle phases, outcome/result/cancel types, `ContestConfig` |
| Rules | `crates/engine/src/game/contest.rs` | the deterministic state machine and the host driver entry points |
| Effect | `Effect::PhysicalContest { kind, contestant }`, `game/effects/physical_contest.rs` | enters `WaitingFor::PhysicalContest` |
| Parser | `parser/oracle_effect/mod.rs` (`try_parse_physical_contest`) | "you and \<player\> have a \<staring\|breath-holding\> contest" |
| AI | `crates/phase-ai/src/contest.rs` | when an AI seat clicks (shared by server and WASM) |
| Server | `crates/server-core/src/contest.rs`, `phase-server` (`run_contest_driver`) | clock, rate limit, tick task, broadcast |
| Local engines | `drive_contest_js` (engine-wasm), `useLocalContestHost` (client) | the same host duties for single-player / P2P host |
| UI | `client/src/components/contest/ContestOverlay.tsx` | rendering and click input only |

The engine never reads a clock. Every input is stamped by the **host** with a monotonic millisecond counter, so the state machine is deterministic and fully testable without sleeping.

## Lifecycle

`Pending → Preparing → Active → Resolving → Completed`, with `Cancelled` reachable from any non-terminal phase. Illegal transitions are no-ops (`ContestPhase::can_transition_to`), which makes every entry point idempotent: a duplicate start, a repeated tick, a late click or a second settle changes nothing.

* **Pending** – the effect resolved; the host has not supplied a clock yet.
* **Preparing** – countdown to GO. Clicks are not counted.
* **Active** – clicks accepted, inactivity monitored.
* **Resolving** – outcome decided, input ignored. Transient: settled in the same host call.
* **Completed / Cancelled** – terminal. `ContestState::complete()` returns the result exactly once.

## Deciding the result

Outcome is decided by sustained activity, never by click count or rate.

* A player's deadline is `last_activity + inactivity_timeout`.
* **Ordering rule.** Before any input is applied, time is advanced to that input's stamp. A click stamped *at* the deadline counts; a timeout needs `now > deadline`.
* **Settle window.** A lapsed player may still recover by clicking until `deadline + settle_window`, which doubles as the jitter allowance. The contest is decided once at least one player has lapsed and every lapsed player is out of that window.
* **Tie.** If both lapsed and their deadlines are within `settle_window` of each other the result is `ContestOutcome::Tie`: a distinct outcome, not a win, loss or cancellation. It never restarts the contest and triggers neither player's consequences. Otherwise the earlier deadline loses.
* The result depends only on stamps and deadlines, never on how late a tick ran.

Defaults (`ContestConfig`): countdown 3s, inactivity 2s, reconnect grace 10s, settle window 150ms, hard cap 5 minutes of Active play (→ `Cancelled(TimeLimit)`).

## Disconnects

* While any seat is away the contest clock is **paused for everyone** (a connected player cannot lose to a clock the absentee does not share). On return every running clock is shifted by the outage, so nobody gains or loses inactivity budget and nothing resets.
* An absentee past `reconnect_grace` **forfeits the contest only** (`ContestLossReason::Forfeit`), not the game.
* If both seats are away when a grace expires, or either is gone for good, the contest is `Cancelled(BothDisconnected)`.
* A participant who concedes or is eliminated → `Cancelled(ParticipantLeftGame)`.
* A restored server session cannot continue a contest (its clock epoch is gone) → `Cancelled(HostAborted)`.
* Disconnecting after your own deadline has passed does not dodge the loss: lapses resolve before the connection change is applied.

## Anti-cheat and validation

* The server resolves the sender from the authenticated token and the instant from its own clock. `ClientMessage::ContestClick` carries only a per-contest sequence number; clients can claim neither a timestamp, a click count nor a winner. P2P guests do the same (`contest_click`, host-stamped).
* Duplicate or reordered deliveries are rejected by the sequence number; clicks outside `Active`, from non-participants, or after a player's recovery window are rejected by the engine.
* Per-seat token bucket (`ContestPolicy`, default 30 clicks/s sustained, burst 40). A dropped click is harmless — it only ever signals activity, and a player at the limit is active. Fast human clicking tops out near 15/s, so legitimate input is never throttled.
* Macros and automation are bounded, not detected: a bot can sustain the maximum rate, but only for the 5-minute cap, after which the contest is cancelled with no effect.
* Held keys do not auto-click (`event.repeat` is ignored), so holding Space is not a cheap macro.

## Game integration

* `WaitingFor::PhysicalContest` has no acting player (`NoActor::HostContestDriver`) — the host advances it.
* On settle the controller's win is stamped on the parked continuation as `optional_effect_performed`, which the existing `AbilityCondition::EventOutcomeWon` ("If you win") reads. A loss, a tie and a cancellation all leave it false, so Eye to Eye does not destroy and Mouth to Mouth does not steal.
* Results are applied exactly once; after settlement every further tick/click/cancel is `NotInContest`.

## Transports

| Mode | Host |
|------|------|
| phase-server (online, native-ai) | server tick task every 50ms; clients send `ContestClick`; live counters via `ContestProgress` |
| Single-player WASM | client drives `drive_contest_js` every 50ms with `performance.now()` |
| P2P | host drives like single-player; guests send `contest_click`, stamped on arrival |

AI seats click on a human-ish schedule (`phase_ai::contest::ContestClicker`): a 220–380ms cadence for a random 2.5–9s, so the AI can win or lose.

## Assumptions and known limits

* A contest is only ever between the controller and one other player (both printed cards). "You and each opponent" is deliberately declined by the parser.
* A recorded replay cannot reproduce a real-time result; starting a contest invalidates an in-progress WASM replay recording, as debug actions do.
* P2P guests see opponent click counters only at state transitions (no live `ContestProgress` over the P2P wire).
* Server restarts cancel a running contest.
