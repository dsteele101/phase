//! Rakdos Roustabout — end-to-end integration tests.
//!
//! Tests for the card: Rakdos Roustabout — "Whenever this creature becomes blocked,
//! it deals 1 damage to the player or planeswalker it's attacking."
//!
//! Oracle (verified against Scryfall):
//!   "Whenever this creature becomes blocked, it deals 1 damage to the player or
//!    planeswalker it's attacking."
//!
//! CR 509.1h: A creature becomes blocked when it is declared as blocked.
//! CR 506.3: "the player or planeswalker it's attacking" identifies the source
//!   creature's attack target (Player, Planeswalker, or Battle).
//! CR 120.1: The damage is dealt to that player or planeswalker.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::phase::Phase;

use super::rules::AttackTarget;

const RAKDOS_ROUSTABOUT: &str =
    "Whenever this creature becomes blocked, it deals 1 damage to the player or planeswalker it's attacking.";

/// Drive the "becomes blocked" triggered ability to resolution: pass priority
/// through the declare-blockers step until the triggered ability on the stack
/// resolves (CR 117, CR 117.4).
fn resolve_blocked_trigger(runner: &mut GameRunner) {
    let mut empty_stack_passes = 0;
    for _ in 0..60 {
        match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    if empty_stack_passes > 2 {
                        eprintln!(
                            "TEMP state: waiting_for={:?} stack_len={}",
                            runner.state().waiting_for,
                            runner.state().stack.len()
                        );
                        return;
                    }
                    empty_stack_passes += 1;
                    runner.act(GameAction::PassPriority).expect("pass priority");
                    continue;
                }
                runner.act(GameAction::PassPriority).expect("pass priority");
            }
            WaitingFor::OrderTriggers { .. } => {
                runner
                    .act(GameAction::OrderTriggers { order: vec![] })
                    .expect("order triggers");
            }
            other => panic!("unexpected waiting state during blocked trigger: {other:?}"),
        }
    }
    eprintln!(
        "TEMP final state: waiting_for={:?} stack_len={} life_p1={}",
        runner.state().waiting_for,
        runner.state().stack.len(),
        runner.life(P1)
    );
    panic!("Rakdos Roustabout blocked trigger did not resolve");
}

/// CR 509.1h + CR 120.1: When Rakdos Roustabout becomes blocked, it deals 1 damage
/// to the player it's attacking (P1).
#[test]
fn rakdos_roustabout_deals_damage_when_blocked() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let roustabout = scenario
        .add_creature_from_oracle(P0, "Rakdos Roustabout", 3, 2, RAKDOS_ROUSTABOUT)
        .id();
    let blocker = scenario
        .add_creature_from_oracle(P1, "Blocker", 2, 2, "")
        .id();

    let mut runner = scenario.build();
    runner.advance_to_combat();

    // Rakdos attacks P1 (CR 508.1); its attack target is P1 (CR 506.3).
    runner
        .declare_attackers(&[(roustabout, AttackTarget::Player(P1))])
        .expect("declare Rakdos attacking P1");

    // Both players pass through the declare-attackers priority window until the
    // declare-blockers step opens (CR 508.1g).
    for _ in 0..8 {
        if matches!(
            runner.state().waiting_for,
            WaitingFor::DeclareBlockers { .. }
        ) {
            break;
        }
        runner.pass_both_players();
    }
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::DeclareBlockers { .. }
        ),
        "expected declare-blockers step, got {:?}",
        runner.state().waiting_for
    );

    // P1 blocks Rakdos (CR 509.1a). Rakdos becomes blocked (CR 509.1h), and its
    // triggered ability goes on the stack to deal 1 damage to P1.
    runner
        .declare_blockers(&[(blocker, roustabout)])
        .expect("declare blocker");

    eprintln!(
        "DEBUG after declare_blockers: waiting_for={:?} stack_len={}",
        runner.state().waiting_for,
        runner.state().stack.len()
    );

    resolve_blocked_trigger(&mut runner);

    // The triggered damage resolves: P1 (the player Rakdos is attacking) drops
    // from 20 to 19.
    assert_eq!(
        runner.life(P1),
        19,
        "Rakdos Roustabout must deal 1 damage to the player it's attacking"
    );
}
