//! Tests for Slow Motion and recursive upkeep tax Auras.
//!
//! Oracle:
//! Enchant creature
//! At the beginning of the upkeep of enchanted creature's controller, that player sacrifices that creature unless they pay {2}.
//! When this Aura is put into a graveyard from the battlefield, return it to its owner's hand.
//!
//! CR 701.21a (Sacrifice) + CR 704.5m (Aura unattached state-based action) + CR 118.12 (Unless payment).

use engine::game::effects::attach::attach_to;
use engine::game::scenario::{GameScenario, P0, P1};
use engine::game::triggers::process_triggers;
use engine::types::actions::GameAction;
use engine::types::events::GameEvent;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::triggers::TriggerMode;
use engine::types::zones::Zone;

const SLOW_MOTION_ORACLE: &str = "Enchant creature\n\
At the beginning of the upkeep of enchanted creature's controller, that player sacrifices that creature unless they pay {2}.\n\
When this Aura is put into a graveyard from the battlefield, return it to its owner's hand.";

fn generic_mana(n: usize) -> Vec<ManaUnit> {
    (0..n)
        .map(|_| ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]))
        .collect()
}

#[test]
fn slow_motion_oracle_parses() {
    let parsed = engine::parser::oracle::parse_oracle_text(
        SLOW_MOTION_ORACLE,
        "Slow Motion",
        &[],
        &["Enchantment".to_string()],
        &["Aura".to_string()],
    );
    assert_eq!(
        parsed.triggers.len(),
        2,
        "Slow Motion must parse 2 triggers"
    );

    let upkeep_trig = parsed
        .triggers
        .iter()
        .find(|t| t.mode == TriggerMode::Phase)
        .expect("upkeep trigger must be present");
    println!("UPKEEP TRIGGER: {:#?}", upkeep_trig);
    assert_eq!(upkeep_trig.phase, Some(Phase::Upkeep));
    assert!(
        upkeep_trig.unless_pay.is_some(),
        "must have unless_pay modifier"
    );

    let dies_trig = parsed
        .triggers
        .iter()
        .find(|t| t.mode == TriggerMode::ChangesZone)
        .expect("leaves-battlefield trigger must be present");
    assert_eq!(dies_trig.destination, Some(Zone::Graveyard));
}

#[test]
fn slow_motion_declining_cost_sacrifices_creature_and_returns_aura_to_hand() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::Upkeep);

    let victim = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    let slow_motion = scenario
        .add_enchantment_from_oracle(P0, "Slow Motion", SLOW_MOTION_ORACLE)
        .with_subtypes(vec!["Aura"])
        .id();

    let mut runner = scenario.build();
    attach_to(runner.state_mut(), slow_motion, victim);

    // P1's upkeep begins
    runner.state_mut().active_player = P1;
    process_triggers(
        runner.state_mut(),
        &[GameEvent::PhaseChanged {
            phase: Phase::Upkeep,
        }],
    );
    runner.advance_until_stack_empty();

    // P1 is prompted to pay {2}
    let waiting = runner.state().waiting_for.clone();
    assert!(
        matches!(waiting, WaitingFor::UnlessPayment { player, .. } if player == P1),
        "P1 must be prompted for unless payment, got {:?}",
        waiting
    );

    // P1 declines to pay
    runner
        .act(GameAction::PayUnlessCost { pay: false })
        .expect("decline upkeep payment");
    runner.advance_until_stack_empty();

    // The creature was sacrificed to P1's graveyard
    assert_eq!(
        runner.state().objects[&victim].zone,
        Zone::Graveyard,
        "enchanted creature must be sacrificed when cost is unpaid"
    );

    // Slow Motion was put into graveyard by SBA and returned to P0's hand by its trigger
    assert_eq!(
        runner.state().objects[&slow_motion].zone,
        Zone::Hand,
        "Slow Motion must return to its owner's hand after the creature is sacrificed"
    );
}

#[test]
fn slow_motion_paying_cost_keeps_creature_and_aura_on_battlefield() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::Upkeep);
    scenario.with_mana_pool(P1, generic_mana(2));

    let victim = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    let slow_motion = scenario
        .add_enchantment_from_oracle(P0, "Slow Motion", SLOW_MOTION_ORACLE)
        .with_subtypes(vec!["Aura"])
        .id();

    let mut runner = scenario.build();
    attach_to(runner.state_mut(), slow_motion, victim);

    // P1's upkeep begins
    runner.state_mut().active_player = P1;
    process_triggers(
        runner.state_mut(),
        &[GameEvent::PhaseChanged {
            phase: Phase::Upkeep,
        }],
    );
    runner.advance_until_stack_empty();

    // P1 pays {2}
    runner
        .act(GameAction::PayUnlessCost { pay: true })
        .expect("pay upkeep cost");
    runner.advance_until_stack_empty();

    // The creature is still on the battlefield
    assert_eq!(
        runner.state().objects[&victim].zone,
        Zone::Battlefield,
        "enchanted creature must remain on battlefield when cost is paid"
    );

    // Slow Motion is still attached to the creature on the battlefield
    assert_eq!(
        runner.state().objects[&slow_motion].zone,
        Zone::Battlefield,
        "Slow Motion must remain on battlefield when cost is paid"
    );
}
