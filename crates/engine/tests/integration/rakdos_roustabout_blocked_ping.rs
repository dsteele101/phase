//! Rakdos Roustabout: "Whenever this creature becomes blocked, it deals 1 damage
//! to the player or planeswalker it's attacking." (CR 509.3c + CR 506.3 + CR 120.1)
//!
//! Building block under test: `TargetFilter::AttackedPlayerOrPlaneswalker`, the exact entity
//! (player, planeswalker, or battle) the source creature is attacking.

use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameScenario, P0, P1};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{Effect, TargetFilter};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;

const ROUSTABOUT: &str = "Whenever this creature becomes blocked, it deals 1 damage to the player or planeswalker it's attacking.";

#[derive(Clone, Copy)]
enum Attacked {
    Player,
    Planeswalker,
}

/// Outcome after the declare-blockers trigger resolved (before combat damage).
struct Outcome {
    p1_life_lost: i32,
    walker_damage: u32,
}

fn run(attacked: Attacked, block: bool) -> Outcome {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let roustabout = scenario
        .add_creature_from_oracle(P0, "Rakdos Roustabout", 3, 2, ROUSTABOUT)
        .id();
    let wall = scenario.add_creature(P1, "Wall", 0, 4).id();
    let walker: ObjectId = scenario
        .add_planeswalker_from_oracle(P1, "Test Walker", "Jace", 5, "")
        .id();
    let mut runner = scenario.build();
    let life_before = runner.life(P1);
    let target = match attacked {
        Attacked::Player => AttackTarget::Player(P1),
        Attacked::Planeswalker => AttackTarget::Planeswalker(walker),
    };

    let mut declared = false;
    for _ in 0..60 {
        match runner.state().waiting_for.clone() {
            WaitingFor::DeclareAttackers { player, .. } if player == P0 && !declared => {
                declared = true;
                runner
                    .act(GameAction::DeclareAttackers {
                        attacks: vec![(roustabout, target)],
                        bands: vec![],
                    })
                    .expect("declare attackers");
            }
            WaitingFor::DeclareAttackers { .. } => {
                runner
                    .act(GameAction::DeclareAttackers {
                        attacks: vec![],
                        bands: vec![],
                    })
                    .expect("no attackers");
            }
            WaitingFor::DeclareBlockers { .. } => {
                let assignments = if block {
                    vec![(wall, roustabout)]
                } else {
                    vec![]
                };
                runner
                    .act(GameAction::DeclareBlockers { assignments })
                    .expect("declare blockers");
                runner.advance_until_stack_empty();
                let loyalty = runner.state().objects[&walker]
                    .counters
                    .get(&CounterType::Loyalty)
                    .copied()
                    .unwrap_or(0);
                return Outcome {
                    p1_life_lost: life_before - runner.life(P1),
                    walker_damage: 5 - loyalty,
                };
            }
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass priority");
            }
            other => panic!("unexpected waiting state: {other:?}"),
        }
    }
    panic!("did not reach declare blockers");
}

#[test]
fn blocked_roustabout_pings_the_player_it_attacks() {
    let out = run(Attacked::Player, true);
    assert_eq!(out.p1_life_lost, 1);
    assert_eq!(out.walker_damage, 0);
}

#[test]
fn blocked_roustabout_pings_the_planeswalker_it_attacks_not_its_controller() {
    // CR 506.3 + CR 120.1: the planeswalker itself takes the damage, and its
    // controller (the defending player, CR 508.5) does not.
    let out = run(Attacked::Planeswalker, true);
    assert_eq!(out.walker_damage, 1);
    assert_eq!(out.p1_life_lost, 0);
}

#[test]
fn unblocked_roustabout_does_not_ping() {
    // Reach-guard: the same setup without a block deals nothing before damage.
    let out = run(Attacked::Player, false);
    assert_eq!(out.p1_life_lost, 0);
}

#[test]
fn attacked_phrase_parses_to_the_attack_referent_not_a_target() {
    let parsed = parse_oracle_text(
        ROUSTABOUT,
        "Rakdos Roustabout",
        &[],
        &["Creature".to_string()],
        &[],
    );
    let effect = &*parsed.triggers[0]
        .execute
        .as_ref()
        .expect("trigger executes")
        .effect;
    assert!(
        matches!(
            effect,
            Effect::DealDamage {
                target: TargetFilter::AttackedPlayerOrPlaneswalker,
                ..
            }
        ),
        "got {effect:?}"
    );
}
