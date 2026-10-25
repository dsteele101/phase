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
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
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
        .add_creature_to_hand_from_oracle(P0, "Canker Abomination", 6, 6, CANKER_ABOMINATION)
        .with_subtypes(vec!["Treefolk", "Horror"])
        .id();

    let mut runner = scenario.build();

    let outcome = runner.cast(canker).choose_option("1").resolve();
    outcome.assert_zone(&[canker], Zone::Battlefield);
    outcome.assert_counters(canker, CounterType::Minus1Minus1, 2);
    outcome.assert_power_toughness(canker, 4, 4);
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
        .add_creature_to_hand_from_oracle(P0, "Canker Abomination", 6, 6, CANKER_ABOMINATION)
        .with_subtypes(vec!["Treefolk", "Horror"])
        .id();

    let mut runner = scenario.build();

    let outcome = runner.cast(canker).choose_option("1").resolve();
    outcome.assert_zone(&[canker], Zone::Battlefield);
    outcome.assert_counters(canker, CounterType::Minus1Minus1, 0);
    outcome.assert_power_toughness(canker, 6, 6);
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
        .add_creature_to_hand_from_oracle(P0, "Canker Abomination", 6, 6, CANKER_ABOMINATION)
        .with_subtypes(vec!["Treefolk", "Horror"])
        .id();

    let mut runner = scenario.build();

    // P0 casts Canker Abomination and chooses P2 ("2").
    // As P2 controls 3 creatures, Canker Abomination enters with 3 -1/-1 counters.
    let outcome = runner.cast(canker).choose_option("2").resolve();
    outcome.assert_zone(&[canker], Zone::Battlefield);
    outcome.assert_counters(canker, CounterType::Minus1Minus1, 3);
    outcome.assert_power_toughness(canker, 3, 3);
}

/// CR 614.12a: An unanswered as-enters choice must pause the game before the
/// permanent enters the battlefield, assert prompt actor and options, verify
/// the entrant is still on the stack (not on the battlefield), and complete to
/// Priority after the choice is answered.
#[test]
fn canker_abomination_choice_prompt_asserts_boundary_and_stack_zone() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature(P1, "Grizzly Bears", 2, 2);
    scenario.add_creature(P1, "Llanowar Elves", 1, 1);

    let canker = scenario
        .add_creature_to_hand_from_oracle(P0, "Canker Abomination", 6, 6, CANKER_ABOMINATION)
        .with_subtypes(vec!["Treefolk", "Horror"])
        .id();

    let mut runner = scenario.build();

    // Cast Canker Abomination without auto-answering the choice
    runner
        .act(GameAction::CastSpell {
            object_id: canker,
            card_id: runner.state().objects[&canker].card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast Canker Abomination");
    runner.advance_until_stack_empty();

    // CR 614.12a: Verify the prompt's actor, options, and entrant zone while pending
    let waiting_for = runner.state().waiting_for.clone();
    let WaitingFor::NamedChoice {
        player,
        choice_type,
        options,
        ..
    } = waiting_for
    else {
        panic!(
            "expected WaitingFor::NamedChoice, got {:?}",
            runner.waiting_for_kind()
        );
    };

    assert_eq!(player, P0, "prompt actor must be controller P0");
    assert!(
        matches!(
            choice_type,
            engine::types::ability::ChoiceType::Opponent { .. }
        ),
        "expected ChoiceType::Opponent, got {:?}",
        choice_type
    );
    assert_eq!(options, vec!["1"], "only opponent P1 is a legal option");
    assert_eq!(
        runner.state().objects[&canker].zone,
        Zone::Stack,
        "CR 614.12a: entrant must not yet be on the battlefield while choice is pending"
    );

    // Answer the choice and let entry complete
    runner
        .act(GameAction::ChooseOption {
            choice: "1".to_string(),
        })
        .expect("choose opponent P1");
    runner.advance_until_stack_empty();

    // Verify entry completed to Priority, on battlefield, with 2 counters and 4/4 stats
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::Priority { .. }),
        "game must return to Priority after entry completes"
    );
    let obj = &runner.state().objects[&canker];
    assert_eq!(obj.zone, Zone::Battlefield);
    assert_eq!(
        obj.counters
            .get(&CounterType::Minus1Minus1)
            .copied()
            .unwrap_or(0),
        2
    );
    assert_eq!(obj.power, Some(4));
    assert_eq!(obj.toughness, Some(4));
}
