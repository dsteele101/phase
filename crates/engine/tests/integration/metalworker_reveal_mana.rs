//! CR 106.1 + CR 701.20a: Metalworker activated mana ability:
//! "{T}: Reveal any number of artifact cards in your hand. Add {C}{C} for each card revealed this way."
//!
//! Verifies:
//! 1. Activating Metalworker with artifact cards in hand enters `WaitingFor::RevealChoice`
//!    with `any_number: true` and `optional: true`.
//! 2. Revealing 2 artifact cards produces 4 colorless mana ({C}{C}{C}{C}) and emits `CardsRevealed`.
//! 3. Revealing 1 artifact card produces 2 colorless mana ({C}{C}).
//! 4. Declining / revealing 0 cards produces 0 mana without error.
//! 5. Activating with 0 artifact cards in hand completes immediately with 0 mana.

use engine::game::scenario::{GameScenario, P0};
use engine::types::actions::GameAction;
use engine::types::events::GameEvent;
use engine::types::game_state::WaitingFor;
use engine::types::mana::ManaType;
use engine::types::phase::Phase;

const METALWORKER_ORACLE: &str =
    "{T}: Reveal any number of artifact cards in your hand. Add {C}{C} for each card revealed this way.";

#[test]
fn metalworker_reveal_two_artifacts_adds_four_colorless_mana() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let metalworker = scenario
        .add_creature_from_oracle(P0, "Metalworker", 1, 2, METALWORKER_ORACLE)
        .as_artifact()
        .id();

    let art1 = scenario
        .add_creature_to_hand(P0, "Ornithopter", 0, 2)
        .as_artifact()
        .id();

    let art2 = scenario
        .add_creature_to_hand(P0, "Memnite", 1, 1)
        .as_artifact()
        .id();

    let non_art = scenario
        .add_creature_to_hand(P0, "Grizzly Bears", 2, 2)
        .id();

    let mut runner = scenario.build();

    // Activating Metalworker's {T} ability
    let result = runner.act(GameAction::ActivateAbility {
        source_id: metalworker,
        ability_index: 0,
    });
    assert!(result.is_ok(), "activation succeeds: {result:?}");

    // Engine should be waiting for reveal choice with any_number: true
    match runner.state().waiting_for {
        WaitingFor::RevealChoice {
            player,
            ref cards,
            optional,
            any_number,
            ..
        } => {
            assert_eq!(player, P0);
            assert!(optional);
            assert!(any_number);
            assert!(cards.contains(&art1));
            assert!(cards.contains(&art2));
            assert!(!cards.contains(&non_art));
        }
        ref wf => panic!("expected WaitingFor::RevealChoice, got {:?}", wf),
    }

    // Player chooses to reveal both artifact cards
    let result = runner.act(GameAction::SelectCards {
        cards: vec![art1, art2],
    });
    assert!(result.is_ok(), "selection succeeds: {result:?}");
    let events = result.unwrap().events;

    // Mana pool should have 4 colorless mana
    assert_eq!(
        runner.state().players[P0.0 as usize]
            .mana_pool
            .count_color(ManaType::Colorless),
        4,
        "expected 4 colorless mana from 2 revealed artifacts"
    );

    // CardsRevealed event should have been emitted
    let revealed_events: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            GameEvent::CardsRevealed {
                player, card_ids, ..
            } => Some((*player, card_ids.clone())),
            _ => None,
        })
        .collect();
    assert!(
        revealed_events
            .iter()
            .any(|(p, c)| *p == P0 && c.contains(&art1) && c.contains(&art2)),
        "expected CardsRevealed event containing both artifact cards, got {revealed_events:?}"
    );
}

#[test]
fn metalworker_reveal_one_artifact_adds_two_colorless_mana() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let metalworker = scenario
        .add_creature_from_oracle(P0, "Metalworker", 1, 2, METALWORKER_ORACLE)
        .as_artifact()
        .id();

    let art1 = scenario
        .add_creature_to_hand(P0, "Ornithopter", 0, 2)
        .as_artifact()
        .id();

    let _art2 = scenario
        .add_creature_to_hand(P0, "Memnite", 1, 1)
        .as_artifact()
        .id();

    let mut runner = scenario.build();

    runner
        .act(GameAction::ActivateAbility {
            source_id: metalworker,
            ability_index: 0,
        })
        .expect("activation succeeds");

    // Choose to reveal only 1 artifact
    runner
        .act(GameAction::SelectCards { cards: vec![art1] })
        .expect("selection succeeds");

    // Mana pool should have 2 colorless mana
    assert_eq!(
        runner.state().players[P0.0 as usize]
            .mana_pool
            .count_color(ManaType::Colorless),
        2,
        "expected 2 colorless mana from 1 revealed artifact"
    );
}

#[test]
fn metalworker_reveal_zero_artifacts_by_selecting_empty_adds_zero_mana() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let metalworker = scenario
        .add_creature_from_oracle(P0, "Metalworker", 1, 2, METALWORKER_ORACLE)
        .as_artifact()
        .id();

    let _art1 = scenario
        .add_creature_to_hand(P0, "Ornithopter", 0, 2)
        .as_artifact()
        .id();

    let mut runner = scenario.build();

    runner
        .act(GameAction::ActivateAbility {
            source_id: metalworker,
            ability_index: 0,
        })
        .expect("activation succeeds");

    // Choose to reveal 0 cards
    runner
        .act(GameAction::SelectCards { cards: vec![] })
        .expect("empty selection succeeds");

    // Mana pool should have 0 colorless mana
    assert_eq!(
        runner.state().players[P0.0 as usize]
            .mana_pool
            .count_color(ManaType::Colorless),
        0,
        "expected 0 colorless mana when revealing 0 cards"
    );
}

#[test]
fn metalworker_with_no_artifacts_in_hand_resolves_immediately_with_zero_mana() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let metalworker = scenario
        .add_creature_from_oracle(P0, "Metalworker", 1, 2, METALWORKER_ORACLE)
        .as_artifact()
        .id();

    let _non_art = scenario
        .add_creature_to_hand(P0, "Grizzly Bears", 2, 2)
        .id();

    let mut runner = scenario.build();

    runner
        .act(GameAction::ActivateAbility {
            source_id: metalworker,
            ability_index: 0,
        })
        .expect("activation succeeds");

    // Should NOT be waiting for reveal choice since no eligible cards exist
    assert!(
        !matches!(runner.state().waiting_for, WaitingFor::RevealChoice { .. }),
        "should not prompt for reveal choice when hand has 0 artifacts"
    );

    assert_eq!(
        runner.state().players[P0.0 as usize]
            .mana_pool
            .count_color(ManaType::Colorless),
        0,
        "expected 0 colorless mana"
    );
}
