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
    panic!("Rakdos Roustabout blocked trigger did not resolve");
}

/// Attack with Rakdos Roustabout (at P1, or at a P1 planeswalker when
/// `with_planeswalker`), block it, and resolve the blocked trigger. Returns the
/// runner and the attacked planeswalker's id when one was added.
fn attack_and_block(
    with_planeswalker: bool,
) -> (GameRunner, Option<engine::types::identifiers::ObjectId>) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let roustabout = scenario
        .add_creature_from_oracle(P0, "Rakdos Roustabout", 3, 2, RAKDOS_ROUSTABOUT)
        .id();
    let blocker = scenario
        .add_creature_from_oracle(P1, "Blocker", 2, 2, "")
        .id();
    let walker = with_planeswalker.then(|| {
        scenario
            .add_planeswalker_from_oracle(P1, "Test Walker", "Jace", 5, "")
            .id()
    });
    let attack_target = match walker {
        Some(pw) => AttackTarget::Planeswalker(pw),
        None => AttackTarget::Player(P1),
    };

    let mut runner = scenario.build();
    runner.advance_to_combat();

    // CR 508.1: Rakdos attacks; its attack target is recorded (CR 506.3).
    runner
        .declare_attackers(&[(roustabout, attack_target)])
        .expect("declare Rakdos attacking");

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
    // triggered ability goes on the stack.
    runner
        .declare_blockers(&[(blocker, roustabout)])
        .expect("declare blocker");

    resolve_blocked_trigger(&mut runner);
    (runner, walker)
}

/// CR 509.1h + CR 120.1: When Rakdos Roustabout becomes blocked while attacking
/// a player, it deals 1 damage to that player (P1).
#[test]
fn rakdos_roustabout_deals_damage_when_blocked() {
    let (runner, _) = attack_and_block(false);
    assert_eq!(
        runner.life(P1),
        19,
        "Rakdos Roustabout must deal 1 damage to the player it's attacking"
    );
}

/// CR 506.3 + CR 120.3c: When it is attacking a planeswalker, the damage goes
/// to that planeswalker (as loyalty loss), not to the planeswalker's controller.
#[test]
fn rakdos_roustabout_damages_attacked_planeswalker_when_blocked() {
    let (runner, walker) = attack_and_block(true);
    let walker = walker.expect("planeswalker was added");
    assert_eq!(
        runner.life(P1),
        20,
        "the planeswalker's controller is untouched"
    );
    assert_eq!(
        runner.state().objects[&walker].loyalty,
        Some(4),
        "Rakdos Roustabout must deal 1 damage to the planeswalker it's attacking"
    );
}
