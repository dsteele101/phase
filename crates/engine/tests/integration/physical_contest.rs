//! Eye to Eye / Mouth to Mouth — the online sustained-clicking adaptation of the
//! staring and breath-holding contests.
//!
//! The Comprehensive Rules define no physical contests, so nothing here cites a
//! CR number for the contest itself. The surrounding rules exercised are CR 608.2c
//! (an "if you win" rider is read from the performed instruction) and CR 109.4
//! (controller of the targeted creature).
//!
//! Every test casts the real card through the cast pipeline, then plays the contest
//! through the host driver (`game::contest::*`) with explicit millisecond stamps —
//! no sleeping, no clock.

use engine::game::contest::{
    advance_contest, cancel_contest, contest_click, set_contest_connection, start_contest,
    ContestStep,
};
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::contest::{
    ClickDisposition, ContestCancelReason, ContestConfig, ContestKind, ContestOutcome,
    ContestPhase, ContestResult,
};
use engine::types::events::GameEvent;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

/// Verbatim Oracle text (MTGJSON/Scryfall).
const EYE_TO_EYE: &str =
    "You and target creature's controller have a staring contest. If you win, destroy that creature.";
const MOUTH_TO_MOUTH: &str = "Choose target opponent and target creature they control. You and that player have a breath-holding contest. If you win, you gain control of that creature.";

/// Short, exact timings so each test reads as a timeline. GO is at t=1000.
fn config() -> ContestConfig {
    ContestConfig {
        countdown_ms: 1_000,
        inactivity_timeout_ms: 1_000,
        reconnect_grace_ms: 5_000,
        settle_window_ms: 0,
        max_active_ms: 600_000,
    }
}

struct Table {
    runner: GameRunner,
    victim: ObjectId,
    events: Vec<GameEvent>,
}

fn eye_to_eye_table() -> Table {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let victim = scenario.add_vanilla(P1, 2, 2);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Eye to Eye", true, EYE_TO_EYE)
        .id();
    let mut runner = scenario.build();
    runner.cast(spell).target_objects(&[victim]).resolve();
    Table {
        runner,
        victim,
        events: Vec::new(),
    }
}

fn mouth_to_mouth_table() -> Table {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let victim = scenario.add_vanilla(P1, 2, 2);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Mouth to Mouth", false, MOUTH_TO_MOUTH)
        .id();
    let mut runner = scenario.build();
    runner
        .cast(spell)
        .target_player(P1)
        .target_objects(&[victim])
        .resolve();
    Table {
        runner,
        victim,
        events: Vec::new(),
    }
}

impl Table {
    fn state(&self) -> &engine::types::game_state::GameState {
        self.runner.state()
    }

    fn contest(&self) -> &engine::types::contest::ContestState {
        match &self.state().waiting_for {
            WaitingFor::PhysicalContest { contest, .. } => contest,
            other => panic!("expected PhysicalContest, got {other:?}"),
        }
    }

    fn start(&mut self) -> ContestStep {
        start_contest(self.runner.state_mut(), config(), 0, &mut self.events)
    }

    fn tick(&mut self, now: u64) -> ContestStep {
        advance_contest(self.runner.state_mut(), now, &mut self.events)
    }

    fn click(&mut self, player: PlayerId, seq: u32, now: u64) -> (ClickDisposition, ContestStep) {
        contest_click(self.runner.state_mut(), player, seq, now, &mut self.events)
    }

    /// Both players click every 400ms starting at GO until `until`; `quit` stops
    /// one player's clicking after the given instant.
    fn rally(&mut self, until: u64, quit: Option<(PlayerId, u64)>) {
        let (mut seq0, mut seq1) = (0, 0);
        let mut t = 1_000;
        while t <= until && matches!(self.state().waiting_for, WaitingFor::PhysicalContest { .. }) {
            for (player, seq) in [(P0, &mut seq0), (P1, &mut seq1)] {
                if quit.is_some_and(|(p, at)| p == player && t > at) {
                    continue;
                }
                *seq += 1;
                self.click(player, *seq, t);
            }
            t += 400;
        }
    }

    fn resolved_events(&self) -> Vec<&GameEvent> {
        self.events
            .iter()
            .filter(|e| matches!(e, GameEvent::PhysicalContestResolved { .. }))
            .collect()
    }
}

#[test]
fn eye_to_eye_parses_to_a_contest_not_unimplemented() {
    let table = eye_to_eye_table();
    let contest = table.contest();
    assert_eq!(contest.kind, ContestKind::Staring);
    assert_eq!(
        contest.players(),
        [P0, P1],
        "controller vs target's controller"
    );
    assert_eq!(
        contest.phase,
        ContestPhase::Pending,
        "pending until the host starts it"
    );
}

#[test]
fn contest_waits_for_the_host_and_has_no_actor() {
    let table = eye_to_eye_table();
    assert!(table.state().waiting_for.acting_players().is_empty());
}

#[test]
fn eye_to_eye_winner_destroys_the_creature() {
    let mut table = eye_to_eye_table();
    assert_eq!(table.start(), ContestStep::Updated);
    table.rally(10_000, Some((P1, 1_500)));
    assert!(
        !matches!(
            table.state().waiting_for,
            WaitingFor::PhysicalContest { .. }
        ),
        "the contest settled"
    );
    assert_eq!(
        table.state().objects[&table.victim].zone,
        Zone::Graveyard,
        "P0 outlasted P1, so the creature is destroyed"
    );
}

#[test]
fn eye_to_eye_loser_keeps_the_creature() {
    let mut table = eye_to_eye_table();
    table.start();
    table.rally(10_000, Some((P0, 1_500)));
    assert!(!matches!(
        table.state().waiting_for,
        WaitingFor::PhysicalContest { .. }
    ));
    assert_eq!(table.state().objects[&table.victim].zone, Zone::Battlefield);
}

#[test]
fn a_tie_does_nothing_for_eye_to_eye() {
    let mut table = eye_to_eye_table();
    table.start();
    // Both players give up together.
    table.rally(1_800, None);
    table.tick(20_000);
    assert!(matches!(
        table.state().waiting_for,
        WaitingFor::Priority { .. }
    ));
    assert_eq!(table.state().objects[&table.victim].zone, Zone::Battlefield);
    let resolved = table.resolved_events();
    assert_eq!(resolved.len(), 1);
    assert!(matches!(
        resolved[0],
        GameEvent::PhysicalContestResolved {
            result: ContestResult::Outcome(ContestOutcome::Tie),
            ..
        }
    ));
}

#[test]
fn a_cancelled_contest_does_nothing_and_is_not_a_tie() {
    let mut table = eye_to_eye_table();
    table.start();
    table.tick(1_000);
    assert_eq!(
        cancel_contest(
            table.runner.state_mut(),
            ContestCancelReason::HostAborted,
            1_100,
            &mut table.events
        ),
        ContestStep::Settled
    );
    assert!(matches!(
        table.state().waiting_for,
        WaitingFor::Priority { .. }
    ));
    assert_eq!(table.state().objects[&table.victim].zone, Zone::Battlefield);
    assert!(matches!(
        table.resolved_events()[0],
        GameEvent::PhysicalContestResolved {
            result: ContestResult::Cancelled(ContestCancelReason::HostAborted),
            ..
        }
    ));
}

#[test]
fn the_result_is_applied_exactly_once() {
    let mut table = eye_to_eye_table();
    table.start();
    table.rally(10_000, Some((P1, 1_500)));
    assert_eq!(table.resolved_events().len(), 1);
    // Duplicate/late deliveries after settlement are inert.
    assert_eq!(table.tick(30_000), ContestStep::NotInContest);
    let (disposition, step) = table.click(P1, 99, 30_100);
    assert_eq!(step, ContestStep::NotInContest);
    assert!(matches!(disposition, ClickDisposition::Rejected(_)));
    assert_eq!(
        cancel_contest(
            table.runner.state_mut(),
            ContestCancelReason::HostAborted,
            30_200,
            &mut table.events
        ),
        ContestStep::NotInContest
    );
    assert_eq!(table.resolved_events().len(), 1);
}

#[test]
fn a_duplicate_start_does_not_restart_the_countdown() {
    let mut table = eye_to_eye_table();
    assert_eq!(table.start(), ContestStep::Updated);
    let go = table.contest().go_at_ms;
    let again = start_contest(table.runner.state_mut(), config(), 700, &mut table.events);
    assert_eq!(again, ContestStep::Unchanged);
    assert_eq!(table.contest().go_at_ms, go);
}

#[test]
fn clicks_before_go_are_not_counted() {
    let mut table = eye_to_eye_table();
    table.start();
    let (disposition, step) = table.click(P0, 1, 500);
    assert!(matches!(disposition, ClickDisposition::Rejected(_)));
    assert_eq!(step, ContestStep::Unchanged);
    assert_eq!(table.contest().contestants[0].clicks, 0);
}

#[test]
fn a_disconnect_past_grace_forfeits_only_the_contest() {
    let mut table = eye_to_eye_table();
    table.start();
    table.tick(1_000);
    assert_eq!(
        set_contest_connection(
            table.runner.state_mut(),
            P1,
            false,
            1_200,
            &mut table.events
        ),
        ContestStep::Updated
    );
    assert_eq!(table.tick(6_300), ContestStep::Settled);
    assert_eq!(
        table.state().objects[&table.victim].zone,
        Zone::Graveyard,
        "P1 forfeited the contest, so P0 won it"
    );
    assert!(
        !matches!(table.state().waiting_for, WaitingFor::GameOver { .. }),
        "a forfeited contest is not a forfeited game"
    );
}

#[test]
fn a_participant_conceding_cancels_the_contest() {
    let mut table = eye_to_eye_table();
    table.start();
    table.tick(1_000);
    engine::game::engine::apply(
        table.runner.state_mut(),
        P1,
        GameAction::Concede { player_id: P1 },
    )
    .expect("concede is always legal");
    // Two-player game: the concession ends the game and replaces the contest state.
    if matches!(
        table.state().waiting_for,
        WaitingFor::PhysicalContest { .. }
    ) {
        assert_eq!(table.tick(1_100), ContestStep::Settled);
    }
    assert!(!matches!(
        table.state().waiting_for,
        WaitingFor::PhysicalContest { .. }
    ));
}

#[test]
fn mouth_to_mouth_winner_gains_control_of_the_creature() {
    let mut table = mouth_to_mouth_table();
    assert_eq!(table.contest().kind, ContestKind::BreathHolding);
    assert_eq!(table.contest().players(), [P0, P1]);
    table.start();
    table.rally(10_000, Some((P1, 1_500)));
    assert!(!matches!(
        table.state().waiting_for,
        WaitingFor::PhysicalContest { .. }
    ));
    assert_eq!(table.state().objects[&table.victim].controller, P0);
}

#[test]
fn mouth_to_mouth_loser_and_tie_leave_control_unchanged() {
    let mut table = mouth_to_mouth_table();
    table.start();
    table.rally(10_000, Some((P0, 1_500)));
    assert_eq!(table.state().objects[&table.victim].controller, P1);

    let mut table = mouth_to_mouth_table();
    table.start();
    table.rally(1_800, None);
    table.tick(20_000);
    assert!(matches!(
        table.state().waiting_for,
        WaitingFor::Priority { .. }
    ));
    assert_eq!(table.state().objects[&table.victim].controller, P1);
}
