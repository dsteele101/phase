//! Wall of Vapor damage prevention integration tests.
//!
//! CR 614.1a (replacement effects using "prevent"), CR 615.1a (damage prevention),
//! CR 509.1g (creatures it's blocking).
//!
//! Verifies that Wall of Vapor:
//! 1. Does NOT prevent damage from spells (e.g. Dry Spell dealing 1 damage to each creature).
//! 2. DOES prevent combat damage from a creature it is currently blocking.
//! 3. Does NOT prevent damage from an attacking creature it is not blocking.

use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const WALL_OF_VAPOR_ORACLE: &str =
    "Defender (This creature can't attack.)\nPrevent all damage that would be dealt to this creature by creatures it's blocking.";
const DRY_SPELL_ORACLE: &str = "Dry Spell deals 1 damage to each creature and each player.";

#[test]
fn wall_of_vapor_takes_damage_from_spell() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let wall = scenario
        .add_creature_from_oracle(P1, "Wall of Vapor", 0, 1, WALL_OF_VAPOR_ORACLE)
        .id();

    let dry_spell = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Dry Spell",
            /* is_instant */ false,
            DRY_SPELL_ORACLE,
        )
        .id();

    let mut runner = scenario.build();

    let outcome = runner.cast(dry_spell).resolve();

    // Wall of Vapor took 1 damage from Dry Spell (damage not prevented)
    // Since toughness is 1, 1 lethal damage causes it to die to state-based actions.
    outcome.assert_zone(&[wall], Zone::Graveyard);
}

#[test]
fn wall_of_vapor_prevents_combat_damage_from_blocked_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let wall = scenario
        .add_creature_from_oracle(P1, "Wall of Vapor", 0, 1, WALL_OF_VAPOR_ORACLE)
        .id();
    let attacker = scenario.add_creature(P0, "Attacker", 2, 2).id();

    let mut runner = scenario.build();

    // Advance to DeclareAttackers
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: vec![(attacker, AttackTarget::Player(P1))],
            bands: vec![],
        })
        .expect("DeclareAttackers should succeed");

    // Pass priority to DeclareBlockers
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareBlockers {
            assignments: vec![(wall, attacker)],
        })
        .expect("DeclareBlockers should succeed");

    // Pass priority through DeclareBlockers and combat damage
    runner.pass_both_players();

    // Combat damage was dealt. Wall of Vapor blocked the 2/2 attacker, so all damage
    // dealt to it by the blocked creature was prevented.
    // Wall of Vapor survives on the battlefield with 0 damage marked.
    assert_eq!(
        runner.state().objects.get(&wall).unwrap().damage_marked,
        0,
        "Combat damage from blocked attacker must be prevented"
    );
    assert_eq!(
        runner.state().objects.get(&wall).unwrap().zone,
        Zone::Battlefield,
        "Wall of Vapor must survive combat"
    );
}

#[test]
fn wall_of_vapor_does_not_prevent_damage_from_unblocked_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let wall = scenario
        .add_creature_from_oracle(P1, "Wall of Vapor", 0, 1, WALL_OF_VAPOR_ORACLE)
        .id();
    let bear = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    let attacker = scenario.add_creature(P0, "Attacker", 2, 2).id();

    let mut runner = scenario.build();

    // Bear blocks attacker; Wall of Vapor does not block attacker.
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: vec![(attacker, AttackTarget::Player(P1))],
            bands: vec![],
        })
        .expect("DeclareAttackers should succeed");

    runner.pass_both_players();
    runner
        .act(GameAction::DeclareBlockers {
            assignments: vec![(bear, attacker)],
        })
        .expect("DeclareBlockers should succeed");

    // Proposed damage from attacker to Wall of Vapor
    // should not be prevented because Wall is not blocking it.
    let mut events = Vec::new();
    let proposed = engine::types::proposed_event::ProposedEvent::Damage {
        source_id: attacker,
        target: engine::types::ability::TargetRef::Object(wall),
        amount: 2,
        is_combat: false,
        applied: Default::default(),
    };
    let result =
        engine::game::replacement::replace_event(runner.state_mut(), proposed, &mut events);
    assert!(
        !matches!(
            result,
            engine::game::replacement::ReplacementResult::Prevented
        ),
        "Damage from creature Wall of Vapor is not blocking must NOT be prevented"
    );
}
