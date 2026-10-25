//! Production pipeline test for non-mana Entwine costs (CR 702.42a).
//!
//! Covers:
//! - Solar Tide ("Entwine—Sacrifice two lands.") paying exactly the required lands with both modes chosen;
//! - Solar Tide casting a single mode with no land payment;
//! - Solar Tide being refused with too few lands;
//! - Solar Tide finishing resolution after payment (destroying both <=2 and >=3 power creatures);
//! - Betrayal of Flesh ("Entwine—Sacrifice three lands.") target-selection continuation across modes and land payment;
//! - Entwine spell discovering other casting costs (e.g., granted Casualty 2).

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaColor, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const SOLAR_TIDE: &str = "Choose one —\n• Destroy all creatures with power 2 or less.\n• Destroy all creatures with power 3 or greater.\nEntwine—Sacrifice two lands.";

const BETRAYAL_OF_FLESH: &str = "Choose one —\n• Destroy target creature.\n• Return target creature card from your graveyard to the battlefield.\nEntwine—Sacrifice three lands.";

const ANHELO_STYLE_CASUALTY_STATIC: &str =
    "Each instant and sorcery spell you cast has casualty 2.";

fn add_mana(runner: &mut GameRunner, mana: ManaType, count: usize) {
    let dummy = ObjectId(0);
    let pool = &mut runner.state_mut().players[0].mana_pool;
    for _ in 0..count {
        pool.add(ManaUnit::new(mana, dummy, false, vec![]));
    }
}

/// Scenario 1: Paying exactly the required lands with both modes chosen finishes resolution
/// and destroys creatures matching both modes while putting sacrificed lands into the graveyard.
#[test]
fn solar_tide_entwine_pay_lands_both_modes() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let land1 = scenario.add_basic_land(P0, ManaColor::White);
    let land2 = scenario.add_basic_land(P0, ManaColor::White);
    let small_creature = scenario.add_creature(P0, "Small Creature", 1, 1).id();
    let big_creature = scenario.add_creature(P1, "Big Creature", 4, 4).id();
    let spell_id = scenario
        .add_spell_to_hand_from_oracle(P0, "Solar Tide", false, SOLAR_TIDE)
        .id();
    let mut runner = scenario.build();

    // Solar Tide costs {4}{W}{W}
    add_mana(&mut runner, ManaType::White, 2);
    add_mana(&mut runner, ManaType::Colorless, 4);

    runner
        .cast(spell_id)
        .modes(&[0, 1])
        .pay_cost_with(&[land1, land2])
        .resolve();

    runner.advance_until_stack_empty();

    let state = runner.state();
    // Sacrificed lands are in the graveyard
    assert_eq!(
        state.objects[&land1].zone,
        Zone::Graveyard,
        "land1 must be in graveyard"
    );
    assert_eq!(
        state.objects[&land2].zone,
        Zone::Graveyard,
        "land2 must be in graveyard"
    );

    // Both modes resolved: both small and big creatures are destroyed
    assert_eq!(
        state.objects[&small_creature].zone,
        Zone::Graveyard,
        "power <= 2 creature must be destroyed"
    );
    assert_eq!(
        state.objects[&big_creature].zone,
        Zone::Graveyard,
        "power >= 3 creature must be destroyed"
    );
}

/// Scenario 2: Casting a single mode requires no land payment and leaves lands on the battlefield.
#[test]
fn solar_tide_single_mode_no_land_cost() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let land1 = scenario.add_basic_land(P0, ManaColor::White);
    let land2 = scenario.add_basic_land(P0, ManaColor::White);
    let small_creature = scenario.add_creature(P0, "Small Creature", 1, 1).id();
    let big_creature = scenario.add_creature(P1, "Big Creature", 4, 4).id();
    let spell_id = scenario
        .add_spell_to_hand_from_oracle(P0, "Solar Tide", false, SOLAR_TIDE)
        .id();
    let mut runner = scenario.build();

    add_mana(&mut runner, ManaType::White, 2);
    add_mana(&mut runner, ManaType::Colorless, 4);

    // Cast only mode 0 (destroy all creatures with power 2 or less)
    runner.cast(spell_id).modes(&[0]).resolve();

    runner.advance_until_stack_empty();

    let state = runner.state();
    // Lands were NOT sacrificed
    assert_eq!(
        state.objects[&land1].zone,
        Zone::Battlefield,
        "land1 must remain on battlefield"
    );
    assert_eq!(
        state.objects[&land2].zone,
        Zone::Battlefield,
        "land2 must remain on battlefield"
    );

    // Only mode 0 resolved
    assert_eq!(
        state.objects[&small_creature].zone,
        Zone::Graveyard,
        "power <= 2 creature must be destroyed"
    );
    assert_eq!(
        state.objects[&big_creature].zone,
        Zone::Battlefield,
        "power >= 3 creature must survive"
    );
}

/// Scenario 3: Being refused with too few lands when attempting to choose both modes.
#[test]
fn solar_tide_entwine_refused_with_too_few_lands() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    // P0 controls only 1 land, but Solar Tide entwine requires 2 lands
    let _land1 = scenario.add_basic_land(P0, ManaColor::White);
    let spell_id = scenario
        .add_spell_to_hand_from_oracle(P0, "Solar Tide", false, SOLAR_TIDE)
        .id();
    let mut runner = scenario.build();

    add_mana(&mut runner, ManaType::White, 2);
    add_mana(&mut runner, ManaType::Colorless, 4);

    let result = runner.cast(spell_id).modes(&[0, 1]).try_resolve();
    assert!(
        result.is_err(),
        "casting entwined Solar Tide with too few lands must be refused"
    );
}

/// Scenario 4: Betrayal of Flesh's target-selection continuation across both modes and land payment.
#[test]
fn betrayal_of_flesh_entwine_target_selection() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let land1 = scenario.add_basic_land(P0, ManaColor::Black);
    let land2 = scenario.add_basic_land(P0, ManaColor::Black);
    let land3 = scenario.add_basic_land(P0, ManaColor::Black);

    // Mode 0 target: creature on battlefield to destroy
    let enemy_creature = scenario.add_creature(P1, "Enemy Creature", 2, 2).id();
    // Mode 1 target: creature card in P0's graveyard to reanimate
    let reanimate_target = scenario
        .add_creature_to_graveyard(P0, "Reanimate Target", 3, 3)
        .id();

    let betrayal_id = scenario
        .add_spell_to_hand_from_oracle(P0, "Betrayal of Flesh", true, BETRAYAL_OF_FLESH)
        .id();
    let mut runner = scenario.build();

    // Betrayal of Flesh costs {5}{B}
    add_mana(&mut runner, ManaType::Black, 1);
    add_mana(&mut runner, ManaType::Colorless, 5);

    // Both modes chosen: target enemy creature for mode 0, graveyard creature for mode 1,
    // and pay 3 lands for entwine.
    runner
        .cast(betrayal_id)
        .modes(&[0, 1])
        .target_objects(&[enemy_creature, reanimate_target])
        .pay_cost_with(&[land1, land2, land3])
        .resolve();

    runner.advance_until_stack_empty();

    let state = runner.state();
    // All three lands sacrificed
    assert_eq!(
        state.objects[&land1].zone,
        Zone::Graveyard,
        "land1 must be sacrificed"
    );
    assert_eq!(
        state.objects[&land2].zone,
        Zone::Graveyard,
        "land2 must be sacrificed"
    );
    assert_eq!(
        state.objects[&land3].zone,
        Zone::Graveyard,
        "land3 must be sacrificed"
    );

    // Mode 0 destroyed enemy creature
    assert_eq!(
        state.objects[&enemy_creature].zone,
        Zone::Graveyard,
        "enemy creature must be destroyed"
    );

    // Mode 1 returned reanimate_target to battlefield
    assert_eq!(
        state.objects[&reanimate_target].zone,
        Zone::Battlefield,
        "graveyard creature must be on battlefield"
    );
    assert_eq!(
        state.objects[&reanimate_target].controller, P0,
        "reanimated creature must be controlled by P0"
    );
}

/// Scenario 5: Entwine cast discovers granted additional costs (Casualty 2), ensuring other costs are not skipped.
#[test]
fn solar_tide_entwine_discovers_granted_casualty() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    // Static granting casualty 2 to instant and sorcery spells
    scenario.add_creature_from_oracle(P0, "Anhelo Proxy", 1, 3, ANHELO_STYLE_CASUALTY_STATIC);
    // Casualty fodder: creature with power >= 2
    let casualty_fodder = scenario.add_creature(P0, "Casualty Fodder", 2, 2).id();
    let land1 = scenario.add_basic_land(P0, ManaColor::White);
    let land2 = scenario.add_basic_land(P0, ManaColor::White);

    let spell_id = scenario
        .add_spell_to_hand_from_oracle(P0, "Solar Tide", false, SOLAR_TIDE)
        .id();
    let mut runner = scenario.build();

    add_mana(&mut runner, ManaType::White, 2);
    add_mana(&mut runner, ManaType::Colorless, 4);

    // Cast entwined Solar Tide while Casualty 2 is granted:
    // Casualty is an optional additional cost (accepted by default / accept_optional),
    // and pay_cost_with supplies the fodder for Casualty and the lands for Entwine.
    runner
        .cast(spell_id)
        .modes(&[0, 1])
        .accept_optional()
        .pay_cost_with(&[casualty_fodder, land1, land2])
        .resolve();

    runner.advance_until_stack_empty();

    let state = runner.state();
    assert_eq!(
        state.objects[&casualty_fodder].zone,
        Zone::Graveyard,
        "casualty fodder must be sacrificed"
    );
    assert_eq!(
        state.objects[&land1].zone,
        Zone::Graveyard,
        "land1 must be sacrificed for entwine"
    );
    assert_eq!(
        state.objects[&land2].zone,
        Zone::Graveyard,
        "land2 must be sacrificed for entwine"
    );
}
