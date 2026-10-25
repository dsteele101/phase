//! Canker Abomination
//!
//! Oracle Text:
//! As this creature enters, choose an opponent.
//! This creature enters with a -1/-1 counter on it for each creature that player controls.
//!
//! CR 607.2d + CR 614.1c + CR 614.12a:
//! An as-enters opponent choice linked to a self-ETB counter replacement whose
//! count depends on the chosen opponent's controlled creatures.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::counter::CounterType;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const P2: PlayerId = PlayerId(2);

const CANKER_ABOMINATION: &str = "As this creature enters, choose an opponent.\n\
    This creature enters with a -1/-1 counter on it for each creature that player controls.";

#[test]
fn canker_abomination_enters_with_counters_for_each_opponent_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    // Opponent (P1) controls 2 creatures; caster (P0) controls 0 creatures.
    scenario.add_creature(P1, "Grizzly Bears", 2, 2);
    scenario.add_creature(P1, "Llanowar Elves", 1, 1);

    let canker = scenario
        .add_creature_to_hand_from_oracle(P0, "Canker Abomination", 4, 4, CANKER_ABOMINATION)
        .with_subtypes(vec!["Treefolk", "Horror"])
        .id();

    let mut runner = scenario.build();

    let outcome = runner.cast(canker).choose_option("1").resolve();
    outcome.assert_zone(&[canker], Zone::Battlefield);
    outcome.assert_counters(canker, CounterType::Minus1Minus1, 2);
    outcome.assert_power_toughness(canker, 2, 2);
}

#[test]
fn canker_abomination_enters_with_zero_counters_when_opponent_controls_no_creatures() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    // Opponent controls 0 creatures. Caster (P0) controls 3 creatures.
    // Confirms "that player controls" does not count caster's creatures.
    scenario.add_creature(P0, "Centaur Courser", 3, 3);
    scenario.add_creature(P0, "Grizzly Bears", 2, 2);
    scenario.add_creature(P0, "Llanowar Elves", 1, 1);

    let canker = scenario
        .add_creature_to_hand_from_oracle(P0, "Canker Abomination", 4, 4, CANKER_ABOMINATION)
        .with_subtypes(vec!["Treefolk", "Horror"])
        .id();

    let mut runner = scenario.build();

    let outcome = runner.cast(canker).choose_option("1").resolve();
    outcome.assert_zone(&[canker], Zone::Battlefield);
    outcome.assert_counters(canker, CounterType::Minus1Minus1, 0);
    outcome.assert_power_toughness(canker, 4, 4);
}

#[test]
fn canker_abomination_three_player_choice_binds_to_chosen_opponent() {
    let mut scenario = GameScenario::new_n_player(3, 7);
    scenario.at_phase(Phase::PreCombatMain);

    // P1 controls 1 creature; P2 controls 3 creatures.
    scenario.add_creature(P1, "Grizzly Bears", 2, 2);

    scenario.add_creature(P2, "Grizzly Bears", 2, 2);
    scenario.add_creature(P2, "Llanowar Elves", 1, 1);
    scenario.add_creature(P2, "Elvish Mystic", 1, 1);

    let canker = scenario
        .add_creature_to_hand_from_oracle(P0, "Canker Abomination", 4, 4, CANKER_ABOMINATION)
        .with_subtypes(vec!["Treefolk", "Horror"])
        .id();

    let mut runner = scenario.build();

    // P0 casts Canker Abomination and chooses P2 ("2").
    // As P2 controls 3 creatures, Canker Abomination enters with 3 -1/-1 counters.
    let outcome = runner.cast(canker).choose_option("2").resolve();
    outcome.assert_zone(&[canker], Zone::Battlefield);
    outcome.assert_counters(canker, CounterType::Minus1Minus1, 3);
    outcome.assert_power_toughness(canker, 1, 1);
}
