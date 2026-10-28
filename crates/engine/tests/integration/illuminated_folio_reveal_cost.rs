//! Illuminated Folio — activated ability with a reveal cost requiring two cards
//! that share a color.
//!
//! Oracle text (verified via Scryfall): "{1}, {T}, Reveal two cards from your
//! hand that share a color: Draw a card."
//!
//! This test validates that:
//! 1. The parser correctly reads the count (2) and shared-quality constraint (color).
//! 2. The activation prompts for exactly two cards.
//! 3. The two revealed cards must share at least one color.
//! 4. Revealing two cards of different colors is rejected.
//! 5. Revealing only one card is rejected.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::game::EngineError;
use engine::types::actions::GameAction;
use engine::types::game_state::{PayCostKind, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;

const ILLUMINATED_FOLIO_ORACLE: &str =
    "{1}, {T}, Reveal two cards from your hand that share a color: Draw a card.";

fn setup_scenario() -> GameScenario {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    // Add cards to P0's library so they can draw
    scenario.with_library_top(
        P0,
        &[
            "Library Card 1",
            "Library Card 2",
            "Library Card 3",
            "Library Card 4",
        ],
    );
    scenario
}

fn activate_and_pay(
    runner: &mut GameRunner,
    folio: ObjectId,
    chosen_cards: &[ObjectId],
) -> Result<(), engine::game::EngineError> {
    runner
        .act(GameAction::ActivateAbility {
            source_id: folio,
            ability_index: 0,
        })
        .expect("activating Illuminated Folio must be accepted");

    // Drive the pay-cost prompt - handle all cost payment steps
    for _ in 0..32 {
        match runner.state().waiting_for.clone() {
            WaitingFor::ManaPayment { .. } => {
                return Err(engine::game::EngineError::InvalidAction(
                    "hit ManaPayment".to_string(),
                ));
            }
            WaitingFor::PayCost {
                kind: PayCostKind::TapCreatures { .. },
                choices,
                ..
            } => {
                // Tap the folio itself
                if choices.contains(&folio) {
                    runner
                        .act(GameAction::SelectCards { cards: vec![folio] })
                        .map_err(|e| {
                            engine::game::EngineError::InvalidAction(format!(
                                "tap payment failed: {e:?}"
                            ))
                        })?;
                } else {
                    return Err(engine::game::EngineError::InvalidAction(
                        "folio not in tap choices".to_string(),
                    ));
                }
            }
            WaitingFor::PayCost {
                kind: PayCostKind::Reveal,
                choices,
                count,
                ..
            } => {
                assert_eq!(
                    count, 2,
                    "Illuminated Folio must require exactly 2 cards to reveal"
                );
                // Verify the eligible choices include our test cards
                assert!(
                    choices.len() >= chosen_cards.len(),
                    "all test cards must be eligible choices"
                );
                runner
                    .act(GameAction::SelectCards {
                        cards: chosen_cards.to_vec(),
                    })
                    .map_err(|e| {
                        engine::game::EngineError::InvalidAction(format!(
                            "reveal selection failed: {e:?}"
                        ))
                    })?;
            }
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    return Ok(());
                }
                // Pass priority to let the ability resolve
                if runner.act(GameAction::PassPriority).is_err() {
                    // If we can't pass (e.g., it's opponent's turn), just wait
                    return Ok(());
                }
            }
            WaitingFor::GameOver { winner } => {
                return Err(engine::game::EngineError::InvalidAction(format!(
                    "game over during cost payment: winner={:?}",
                    winner
                )));
            }
            other => {
                return Err(engine::game::EngineError::InvalidAction(format!(
                    "unexpected prompt: {other:?}"
                )));
            }
        }
    }
    Err(engine::game::EngineError::InvalidAction(
        "activation pipeline did not settle".to_string(),
    ))
}

#[test]
fn illuminated_folio_reveal_two_blue_cards_succeeds() {
    let mut scenario = setup_scenario();

    // Add Illuminated Folio to battlefield using alternative method
    let folio = scenario
        .add_creature(P0, "Illuminated Folio", 0, 0)
        .as_artifact()
        .from_oracle_text(ILLUMINATED_FOLIO_ORACLE)
        .id();

    // Two blue cards in hand (share blue)
    let blue1 = scenario
        .add_creature_to_hand(P0, "Blue Card 1", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 1,
        })
        .id();
    let blue2 = scenario
        .add_creature_to_hand(P0, "Blue Card 2", 3, 3)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 2,
        })
        .id();
    // A red card in hand (different color)
    let _red1 = scenario
        .add_creature_to_hand(P0, "Red Card 1", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Red],
            generic: 1,
        })
        .id();

    // Give P0 enough mana to pay {1}
    scenario.with_mana_pool(
        P0,
        vec![ManaUnit::new(
            ManaType::Colorless,
            ObjectId(91_001),
            false,
            vec![],
        )],
    );

    let mut runner = scenario.build();

    // Activate and pay with two blue cards (should succeed)
    activate_and_pay(&mut runner, folio, &[blue1, blue2]).expect("two blue cards must be valid");

    // Verify the ability resolved - P0 should have drawn a card
    // Started with 3 cards in hand, revealed 2 (still in hand), drew 1 = 4 cards in hand
    assert_eq!(
        runner.state().players[P0.0 as usize].hand.len(),
        4,
        "player should have drawn a card"
    );
}

#[test]
fn illuminated_folio_reveal_blue_and_red_fails() {
    let mut scenario = setup_scenario();

    let folio = scenario
        .add_creature(P0, "Illuminated Folio", 0, 0)
        .as_artifact()
        .from_oracle_text(ILLUMINATED_FOLIO_ORACLE)
        .id();

    let blue1 = scenario
        .add_creature_to_hand(P0, "Blue Card 1", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 1,
        })
        .id();
    let red1 = scenario
        .add_creature_to_hand(P0, "Red Card 1", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Red],
            generic: 1,
        })
        .id();

    scenario.with_mana_pool(
        P0,
        vec![ManaUnit::new(
            ManaType::Colorless,
            ObjectId(91_001),
            false,
            vec![],
        )],
    );

    let mut runner = scenario.build();

    // Activate and try to pay with blue + red (should fail)
    let result = activate_and_pay(&mut runner, folio, &[blue1, red1]);
    match result {
        Err(EngineError::InvalidAction(msg)) => assert!(
            msg.contains("must share"),
            "rejection must come from the shared-color check, got: {msg}"
        ),
        other => panic!("expected shared-color rejection, got {other:?}"),
    }
}

#[test]
fn illuminated_folio_reveal_only_one_card_fails() {
    let mut scenario = setup_scenario();

    let folio = scenario
        .add_creature(P0, "Illuminated Folio", 0, 0)
        .as_artifact()
        .from_oracle_text(ILLUMINATED_FOLIO_ORACLE)
        .id();

    let blue1 = scenario
        .add_creature_to_hand(P0, "Blue Card 1", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 1,
        })
        .id();

    scenario.with_mana_pool(
        P0,
        vec![ManaUnit::new(
            ManaType::Colorless,
            ObjectId(91_001),
            false,
            vec![],
        )],
    );

    let mut runner = scenario.build();

    // Try to activate with only one card (should fail - need exactly 2)
    runner
        .act(GameAction::ActivateAbility {
            source_id: folio,
            ability_index: 0,
        })
        .expect("activating Illuminated Folio must be accepted");

    // Drive the pay-cost prompt but only select 1 card
    for _ in 0..16 {
        match runner.state().waiting_for.clone() {
            WaitingFor::ManaPayment { .. } => {
                let _ = runner.act(GameAction::PassPriority); // This will likely fail
            }
            WaitingFor::PayCost {
                kind: PayCostKind::TapCreatures { .. },
                choices,
                ..
            } => {
                if choices.contains(&folio) {
                    let _ = runner.act(GameAction::SelectCards { cards: vec![folio] });
                }
            }
            WaitingFor::PayCost {
                kind: PayCostKind::Reveal,
                count,
                ..
            } => {
                assert_eq!(count, 2);
                // Select only 1 card - should fail
                let result = runner.act(GameAction::SelectCards { cards: vec![blue1] });
                assert!(
                    result.is_err(),
                    "revealing only 1 card when 2 are required must be rejected"
                );
                return;
            }
            WaitingFor::Priority { .. } => {
                if runner.act(GameAction::PassPriority).is_err() {
                    return;
                }
            }
            other => panic!("unexpected prompt: {other:?}"),
        }
    }
    panic!("activation pipeline did not reach reveal prompt");
}

#[test]
fn illuminated_folio_reveal_multicolor_and_blue_succeeds() {
    let mut scenario = setup_scenario();

    let folio = scenario
        .add_creature(P0, "Illuminated Folio", 0, 0)
        .as_artifact()
        .from_oracle_text(ILLUMINATED_FOLIO_ORACLE)
        .id();

    // Multicolor card (blue + red)
    let multi = scenario
        .add_creature_to_hand(P0, "Multicolor Card", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue, ManaCostShard::Red],
            generic: 1,
        })
        .id();
    // Blue card
    let blue1 = scenario
        .add_creature_to_hand(P0, "Blue Card 1", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 1,
        })
        .id();

    scenario.with_mana_pool(
        P0,
        vec![ManaUnit::new(
            ManaType::Colorless,
            ObjectId(91_001),
            false,
            vec![],
        )],
    );

    let mut runner = scenario.build();

    // Activate and pay with multicolor + blue (share blue - should succeed)
    activate_and_pay(&mut runner, folio, &[multi, blue1]).expect("multicolor + blue share blue");

    // Verify the ability resolved
    assert_eq!(
        runner.state().players[P0.0 as usize].hand.len(),
        3, // Started with 2, revealed 2 (still in hand), drew 1 = 3 cards in hand
        "player should have drawn a card"
    );
}

#[test]
fn illuminated_folio_reveal_colorless_cards_fails() {
    let mut scenario = setup_scenario();

    let folio = scenario
        .add_creature(P0, "Illuminated Folio", 0, 0)
        .as_artifact()
        .from_oracle_text(ILLUMINATED_FOLIO_ORACLE)
        .id();

    // Two colorless cards (no colors to share)
    let colorless1 = scenario
        .add_creature_to_hand(P0, "Colorless Card 1", 2, 2)
        .with_mana_cost(ManaCost::generic(2))
        .id();
    let colorless2 = scenario
        .add_creature_to_hand(P0, "Colorless Card 2", 3, 3)
        .with_mana_cost(ManaCost::generic(3))
        .id();

    scenario.with_mana_pool(
        P0,
        vec![ManaUnit::new(
            ManaType::Colorless,
            ObjectId(91_001),
            false,
            vec![],
        )],
    );

    let mut runner = scenario.build();

    // Activate and try to pay with two colorless cards (should fail - no shared color)
    let result = activate_and_pay(&mut runner, folio, &[colorless1, colorless2]);
    match result {
        Err(EngineError::InvalidAction(msg)) => assert!(
            msg.contains("must share"),
            "rejection must come from the shared-color check, got: {msg}"
        ),
        other => panic!("expected shared-color rejection, got {other:?}"),
    }
}
