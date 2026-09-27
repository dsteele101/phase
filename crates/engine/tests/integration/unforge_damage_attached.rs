//! Unforge — "Destroy target Equipment. If that Equipment was attached to a creature, Unforge deals 2 damage to that creature."
//!
//! CR 301.5: Equipment attachment status.
//! CR 400.7: When an object moves, it becomes a new object; previous status is preserved in LKI.
//! CR 608.2c / CR 608.2h: The sub-ability checks LKI to see if the destroyed Equipment was attached to a creature,
//! and deals 2 damage to that creature.

use engine::game::effects::attach::attach_to;
use engine::game::game_object::AttachTarget;
use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const UNFORGE_ORACLE: &str =
    "Destroy target Equipment. If that Equipment was attached to a creature, Unforge deals 2 damage to that creature.";

#[test]
fn unforge_destroys_attached_equipment_and_deals_2_damage_to_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let equipment = scenario
        .add_artifact_from_oracle(
            P1,
            "Bonesplitter",
            "Equipped creature gets +2/+0.\nEquip {1}",
        )
        .with_subtypes(vec!["Equipment"])
        .id();

    let bearer = scenario
        .add_creature_from_oracle(P1, "Hill Giant", 3, 3, "")
        .id();

    let unforge = scenario
        .add_spell_to_hand_from_oracle(P0, "Unforge", false, UNFORGE_ORACLE)
        .id();

    let mut runner = scenario.build();

    assert_eq!(attach_to(runner.state_mut(), equipment, bearer), None);
    assert_eq!(
        runner.state().objects[&equipment].attached_to,
        Some(AttachTarget::Object(bearer)),
    );

    let outcome = runner.cast(unforge).target_objects(&[equipment]).resolve();

    outcome.assert_zone(&[equipment], Zone::Graveyard);
    outcome.assert_zone(&[bearer], Zone::Battlefield);

    let state = outcome.state();
    assert_eq!(
        state.objects[&bearer].damage_marked, 2,
        "CR 608.2c / CR 608.2h: Unforge must deal 2 damage to the creature the destroyed Equipment was attached to"
    );
}

#[test]
fn unforge_destroys_attached_equipment_and_kills_2_toughness_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let equipment = scenario
        .add_artifact_from_oracle(
            P1,
            "Short Sword",
            "Equipped creature gets +1/+1.\nEquip {1}",
        )
        .with_subtypes(vec!["Equipment"])
        .id();

    let bearer = scenario
        .add_creature_from_oracle(P1, "Grizzly Bears", 2, 2, "")
        .id();

    let unforge = scenario
        .add_spell_to_hand_from_oracle(P0, "Unforge", false, UNFORGE_ORACLE)
        .id();

    let mut runner = scenario.build();

    assert_eq!(attach_to(runner.state_mut(), equipment, bearer), None);

    let outcome = runner.cast(unforge).target_objects(&[equipment]).resolve();

    outcome.assert_zone(&[equipment], Zone::Graveyard);
    outcome.assert_zone(&[bearer], Zone::Graveyard);
}

#[test]
fn unforge_destroys_unattached_equipment_and_deals_no_damage() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let equipment = scenario
        .add_artifact_from_oracle(
            P1,
            "Short Sword",
            "Equipped creature gets +1/+1.\nEquip {1}",
        )
        .with_subtypes(vec!["Equipment"])
        .id();

    let bystander = scenario
        .add_creature_from_oracle(P1, "Hill Giant", 3, 3, "")
        .id();

    let unforge = scenario
        .add_spell_to_hand_from_oracle(P0, "Unforge", false, UNFORGE_ORACLE)
        .id();

    let mut runner = scenario.build();

    let outcome = runner.cast(unforge).target_objects(&[equipment]).resolve();

    outcome.assert_zone(&[equipment], Zone::Graveyard);
    outcome.assert_zone(&[bystander], Zone::Battlefield);

    let state = outcome.state();
    assert_eq!(
        state.objects[&bystander].damage_marked, 0,
        "Unforge must not deal damage if the Equipment was not attached to a creature"
    );
}
