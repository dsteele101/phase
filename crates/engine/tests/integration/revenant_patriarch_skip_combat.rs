//! Revenant Patriarch: "When this creature enters, if {W} was spent to cast
//! it, target player skips their next combat phase."

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::game::turns::{advance_phase, start_next_turn};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;

/// Verbatim Oracle text (Scryfall).
const PATRIARCH_ORACLE: &str = "When this creature enters, if {W} was spent to cast it, target player skips their next combat phase.\nThis creature can't block.";

fn add_mana(runner: &mut GameRunner, player: PlayerId, mana_type: ManaType, amount: u32) {
    let pool = &mut runner
        .state_mut()
        .players
        .iter_mut()
        .find(|p| p.id == player)
        .unwrap()
        .mana_pool;
    for _ in 0..amount {
        pool.add(ManaUnit::new(
            mana_type,
            engine::types::identifiers::ObjectId(0),
            false,
            vec![],
        ));
    }
}

/// Cast Patriarch paying `white` W and `black` B, answer the target prompt
/// with `target`, and resolve everything. Returns whether the trigger was
/// seen on the stack (reach guard).
fn cast_and_resolve(white: u32, black: u32, target: PlayerId) -> (GameRunner, bool) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let patriarch = {
        let mut builder = scenario.add_creature_to_hand(P0, "Revenant Patriarch", 4, 3);
        builder.from_oracle_text(PATRIARCH_ORACLE);
        builder.with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Black],
            generic: 4,
        });
        builder.id()
    };
    let mut runner = scenario.build();
    add_mana(&mut runner, P0, ManaType::White, white);
    add_mana(&mut runner, P0, ManaType::Black, black);
    let card_id = runner.state().objects[&patriarch].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: patriarch,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast accepted");

    let mut trigger_seen = false;
    for _ in 0..40 {
        match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::TriggerTargetSelection { .. }
            | WaitingFor::TargetSelection { .. }
            | WaitingFor::MultiTargetSelection { .. } => {
                trigger_seen = true;
                runner
                    .act(GameAction::ChooseTarget {
                        target: Some(TargetRef::Player(target)),
                    })
                    .or_else(|_| {
                        runner.act(GameAction::SelectTargets {
                            targets: vec![TargetRef::Player(target)],
                        })
                    })
                    .expect("target accepted");
            }
            WaitingFor::Priority { .. } => {
                if !runner.state().stack.is_empty()
                    && runner.state().objects[&patriarch].zone
                        == engine::types::zones::Zone::Battlefield
                {
                    trigger_seen = true;
                }
                runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!("unexpected waiting state: {other:?}"),
        }
    }
    (runner, trigger_seen)
}

/// Advance from precombat main through the combat segment on the current
/// active player's turn; returns the phase reached and how many beginning of
/// combat steps began.
fn run_combat_segment(runner: &mut GameRunner) -> (Phase, u32) {
    let state = runner.state_mut();
    state.phase = Phase::PreCombatMain;
    state.steps_started_this_turn.clear();
    let mut events = Vec::new();
    advance_phase(state, &mut events);
    (
        state.phase,
        state.steps_started_this_turn.count(Phase::BeginCombat),
    )
}

fn combat_skips(runner: &GameRunner, player: PlayerId) -> u32 {
    runner.state().steps_to_skip[player.0 as usize]
        .get(&Phase::BeginCombat)
        .copied()
        .unwrap_or(0)
}

#[test]
fn white_spent_makes_target_player_skip_next_combat() {
    let (runner, seen) = cast_and_resolve(1, 4, P1);
    assert!(seen, "ETB trigger must reach the stack");
    assert_eq!(combat_skips(&runner, P1), 1, "P1 must skip next combat");
    assert_eq!(combat_skips(&runner, P0), 0);
}

#[test]
fn no_white_spent_does_not_skip_combat() {
    let (runner, _) = cast_and_resolve(0, 5, P1);
    assert_eq!(combat_skips(&runner, P1), 0);
    assert_eq!(combat_skips(&runner, P0), 0);
}

/// CR 500.11 + CR 614.10a: the armed skip is used up by the target player's
/// next combat phase only; their following combat is normal again.
#[test]
fn target_player_skips_exactly_their_next_combat_phase() {
    let (mut runner, seen) = cast_and_resolve(1, 4, P1);
    assert!(seen, "ETB trigger must reach the stack");

    // The caster's own combat this turn is unaffected.
    let (phase, combats) = run_combat_segment(&mut runner);
    assert_eq!(combats, 1, "P0's combat is not skipped");
    assert_ne!(phase, Phase::PostCombatMain);

    // P1's next turn: combat is skipped straight to postcombat main.
    let mut events = Vec::new();
    start_next_turn(runner.state_mut(), &mut events);
    assert_eq!(runner.state().active_player, P1);
    let (phase, combats) = run_combat_segment(&mut runner);
    assert_eq!(phase, Phase::PostCombatMain, "P1's combat is skipped");
    assert_eq!(combats, 0, "no combat step begins for P1");
    assert_eq!(combat_skips(&runner, P1), 0, "skip fully consumed");
    assert!(
        runner.state().steps_to_skip[P1.0 as usize].is_empty(),
        "no stale per-step counter remains"
    );

    // P1's following turn: combat runs.
    start_next_turn(runner.state_mut(), &mut events);
    start_next_turn(runner.state_mut(), &mut events);
    assert_eq!(runner.state().active_player, P1);
    let (_, combats) = run_combat_segment(&mut runner);
    assert_eq!(combats, 1, "P1's later combat is normal");
}
