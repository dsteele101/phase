//! Tests verifying CR 602.5 / CR 602.5b / CR 605.3a:
//! When a static effect prohibits activating abilities of a permanent type
//! (e.g. Collector Ouphe: "Activated abilities of artifacts can't be activated."),
//! that prohibition applies to all activated abilities of those permanents,
//! including mana abilities, during spellcasting auto-tap, payment simulation,
//! and direct ability activation.

use engine::game::scenario::{GameScenario, P0};
use engine::types::actions::GameAction;
use engine::types::game_state::CastPaymentMode;
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const COLLECTOR_OUPHE_ORACLE: &str = "Activated abilities of artifacts can't be activated.";
const ORNITHOPTER_OF_PARADISE_ORACLE: &str = "Flying\n{T}: Add one mana of any color.";
const LLANOWAR_ELVES_ORACLE: &str = "{T}: Add {G}.";
const GREAT_FURNACE_ORACLE: &str = "{T}: Add {R}.";

#[test]
fn collector_ouphe_prevents_ornithopter_of_paradise_auto_tap_and_cast() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    // Collector Ouphe on the battlefield
    let _ouphe = scenario
        .add_creature_from_oracle(P0, "Collector Ouphe", 2, 2, COLLECTOR_OUPHE_ORACLE)
        .id();

    // Ornithopter of Paradise on the battlefield (Artifact Creature)
    let mut ornithopter_builder = scenario.add_creature_from_oracle(
        P0,
        "Ornithopter of Paradise",
        0,
        2,
        ORNITHOPTER_OF_PARADISE_ORACLE,
    );
    ornithopter_builder.as_artifact_creature();
    let ornithopter = ornithopter_builder.id();

    // Spell costing {G} in hand
    let spell = scenario
        .add_creature_to_hand_from_oracle(P0, "Green Spell", 1, 1, "")
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Green],
            generic: 0,
        })
        .id();

    let mut runner = scenario.build();

    // 1. Direct activation of Ornithopter's mana ability must fail under Collector Ouphe
    let direct_act = runner.act(GameAction::ActivateAbility {
        source_id: ornithopter,
        ability_index: 0,
    });
    assert!(
        direct_act.is_err(),
        "Direct activation of Ornithopter of Paradise must fail under Collector Ouphe, got {:?}",
        direct_act
    );

    // 2. Attempting to cast the spell via auto-tap must fail (cannot auto-tap Ornithopter)
    let card_id = runner.state().objects[&spell].card_id;
    let cast_action = GameAction::CastSpell {
        object_id: spell,
        card_id,
        targets: Vec::new(),
        payment_mode: CastPaymentMode::Auto,
    };
    let result = runner.act(cast_action);
    assert!(
        result.is_err(),
        "Casting spell using Ornithopter under Collector Ouphe must fail, got: {:?}",
        result
    );

    // 3. Verify Ornithopter remained untapped and spell is still in hand
    let post_state = runner.state();
    assert!(
        !post_state.objects[&ornithopter].tapped,
        "Ornithopter should remain untapped"
    );
    assert_eq!(
        post_state.objects[&spell].zone,
        Zone::Hand,
        "Spell should remain in hand"
    );
}

#[test]
fn collector_ouphe_allows_non_artifact_creature_mana_ability() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    // Collector Ouphe on the battlefield
    let _ouphe = scenario
        .add_creature_from_oracle(P0, "Collector Ouphe", 2, 2, COLLECTOR_OUPHE_ORACLE)
        .id();

    // Llanowar Elves on the battlefield (Non-artifact creature)
    let elves = scenario
        .add_creature_from_oracle(P0, "Llanowar Elves", 1, 1, LLANOWAR_ELVES_ORACLE)
        .id();

    // Spell costing {G} in hand
    let spell = scenario
        .add_creature_to_hand_from_oracle(P0, "Green Spell", 1, 1, "")
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Green],
            generic: 0,
        })
        .id();

    let mut runner = scenario.build();

    // Casting the spell auto-taps Llanowar Elves and succeeds
    let card_id = runner.state().objects[&spell].card_id;
    let cast_action = GameAction::CastSpell {
        object_id: spell,
        card_id,
        targets: Vec::new(),
        payment_mode: CastPaymentMode::Auto,
    };
    let result = runner.act(cast_action);
    assert!(
        result.is_ok(),
        "Casting spell using Llanowar Elves must succeed, got: {:?}",
        result
    );

    let post_state = runner.state();
    assert!(
        post_state.objects[&elves].tapped,
        "Llanowar Elves should be tapped for mana"
    );
}

#[test]
fn collector_ouphe_prevents_artifact_land_auto_tap() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    // Collector Ouphe on the battlefield
    let _ouphe = scenario
        .add_creature_from_oracle(P0, "Collector Ouphe", 2, 2, COLLECTOR_OUPHE_ORACLE)
        .id();

    // Great Furnace (Artifact Land) on the battlefield
    let mut furnace_builder =
        scenario.add_land_from_oracle(P0, "Great Furnace", GREAT_FURNACE_ORACLE);
    furnace_builder.as_artifact_land();
    let furnace = furnace_builder.id();

    // Spell costing {R} in hand
    let spell = scenario
        .add_creature_to_hand_from_oracle(P0, "Red Spell", 1, 1, "")
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Red],
            generic: 0,
        })
        .id();

    let mut runner = scenario.build();

    // Direct activation of Great Furnace must fail
    let direct_act = runner.act(GameAction::ActivateAbility {
        source_id: furnace,
        ability_index: 0,
    });
    assert!(
        direct_act.is_err(),
        "Direct activation of Great Furnace must fail under Collector Ouphe, got {:?}",
        direct_act
    );

    // Auto-tap cast using Great Furnace must fail
    let card_id = runner.state().objects[&spell].card_id;
    let cast_action = GameAction::CastSpell {
        object_id: spell,
        card_id,
        targets: Vec::new(),
        payment_mode: CastPaymentMode::Auto,
    };
    let result = runner.act(cast_action);
    assert!(
        result.is_err(),
        "Cast using Great Furnace must fail under Collector Ouphe"
    );
    assert!(
        !runner.state().objects[&furnace].tapped,
        "Great Furnace must remain untapped"
    );
}

#[test]
fn collector_ouphe_allows_basic_forest_auto_tap() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    // Collector Ouphe on the battlefield
    let _ouphe = scenario
        .add_creature_from_oracle(P0, "Collector Ouphe", 2, 2, COLLECTOR_OUPHE_ORACLE)
        .id();

    // Forest on the battlefield
    let forest = scenario.add_basic_land(P0, ManaColor::Green);

    // Spell costing {G} in hand
    let spell = scenario
        .add_creature_to_hand_from_oracle(P0, "Green Spell", 1, 1, "")
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Green],
            generic: 0,
        })
        .id();

    let mut runner = scenario.build();

    let card_id = runner.state().objects[&spell].card_id;
    let cast_action = GameAction::CastSpell {
        object_id: spell,
        card_id,
        targets: Vec::new(),
        payment_mode: CastPaymentMode::Auto,
    };
    let result = runner.act(cast_action);
    assert!(
        result.is_ok(),
        "Cast using Forest must succeed under Collector Ouphe, got: {:?}",
        result
    );
    assert!(
        runner.state().objects[&forest].tapped,
        "Forest must be tapped"
    );
}
