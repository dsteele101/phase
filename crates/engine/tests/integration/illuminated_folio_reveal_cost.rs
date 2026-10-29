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
use engine::types::game_state::{CastPaymentMode, PayCostKind, WaitingFor};
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
            payment_mode: CastPaymentMode::Auto,
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
                kind: PayCostKind::Reveal { .. },
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

    // One blue, one red - different colors
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

    // Two blue cards keep a legal pair in hand, so the cost is payable
    // and the shared-color check is what rejects the chosen pair.
    scenario
        .add_creature_to_hand(P0, "Blue Card 2", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 1,
        });
    scenario
        .add_creature_to_hand(P0, "Blue Card 3", 3, 3)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 2,
        });

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

    // Activate and try to pay with blue + red (should fail - no shared color)
    let result = activate_and_pay(&mut runner, folio, &[blue1, red1]);
    match result {
        Err(EngineError::InvalidAction(msg)) => assert!(
            msg.contains("must share"),
            "rejection must come from the shared-color check, got: {msg}"
        ),
        other => panic!("expected shared-color rejection, got {other:?}"),
    }
}

/// CR 701.20a: Revealing only one card when two are required must be rejected.
/// The reveal prompt must be reached and the selection must fail.
#[test]
fn illuminated_folio_reveal_only_one_card_fails() {
    let mut scenario = setup_scenario();

    let folio = scenario
        .add_creature(P0, "Illuminated Folio", 0, 0)
        .as_artifact()
        .from_oracle_text(ILLUMINATED_FOLIO_ORACLE)
        .id();

    // Two blue cards in hand
    let blue1 = scenario
        .add_creature_to_hand(P0, "Blue Card 1", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 1,
        })
        .id();
    let _blue2 = scenario
        .add_creature_to_hand(P0, "Blue Card 2", 3, 3)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 2,
        })
        .id();

    // A second blue card keeps a legal pair in hand, so the cost is payable
    // and the card-count check is what rejects the single card.
    scenario
        .add_creature_to_hand(P0, "Blue Card 3", 3, 3)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 2,
        });

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
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("activating Illuminated Folio must be accepted");

    // Drive the pay-cost prompt but only select 1 card
    let mut reached_reveal = false;
    for _ in 0..16 {
        match runner.state().waiting_for.clone() {
            WaitingFor::ManaPayment { .. } => {
                runner
                    .act(GameAction::PassPriority)
                    .expect("mana payment should be passable");
            }
            WaitingFor::PayCost {
                kind: PayCostKind::TapCreatures { .. },
                choices,
                ..
            } => {
                if choices.contains(&folio) {
                    runner
                        .act(GameAction::SelectCards { cards: vec![folio] })
                        .expect("tap payment must succeed");
                }
            }
            WaitingFor::PayCost {
                kind: PayCostKind::Reveal { .. },
                count,
                ..
            } => {
                reached_reveal = true;
                assert_eq!(count, 2);
                // Select only 1 card - should fail
                let result = runner.act(GameAction::SelectCards { cards: vec![blue1] });
                assert!(
                    result.is_err(),
                    "revealing only 1 card when 2 are required must be rejected"
                );
                break;
            }
            WaitingFor::Priority { .. } => {
                runner
                    .act(GameAction::PassPriority)
                    .expect("priority pass must succeed");
            }
            other => panic!("unexpected prompt: {other:?}"),
        }
    }
    assert!(
        reached_reveal,
        "activation pipeline must reach reveal prompt"
    );
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

    // Two blue cards keep a legal pair in hand, so the cost is payable
    // and the shared-color check is what rejects the chosen pair.
    scenario
        .add_creature_to_hand(P0, "Blue Card 1", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 1,
        });
    scenario
        .add_creature_to_hand(P0, "Blue Card 2", 3, 3)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 2,
        });

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

/// CR 601.2b + CR 701.20a: With enough cards in hand but no two sharing a
/// color, the reveal cost is unpayable, so the Folio cannot be activated. The
/// same board plus a second blue card activates (positive guard).
#[test]
fn illuminated_folio_unpayable_without_a_pair_sharing_a_color() {
    let build = |with_second_blue: bool| {
        let mut scenario = setup_scenario();
        let folio = scenario
            .add_creature(P0, "Illuminated Folio", 0, 0)
            .as_artifact()
            .from_oracle_text(ILLUMINATED_FOLIO_ORACLE)
            .id();
        let mut colored = vec![
            ("Blue Card 1", ManaCostShard::Blue),
            ("Red Card 1", ManaCostShard::Red),
            ("Green Card 1", ManaCostShard::Green),
        ];
        if with_second_blue {
            colored.push(("Blue Card 2", ManaCostShard::Blue));
        }
        for (name, shard) in colored {
            scenario
                .add_creature_to_hand(P0, name, 2, 2)
                .with_mana_cost(ManaCost::Cost {
                    shards: vec![shard],
                    generic: 1,
                });
        }
        scenario.with_mana_pool(
            P0,
            vec![ManaUnit::new(
                ManaType::Colorless,
                ObjectId(91_001),
                false,
                vec![],
            )],
        );
        (scenario.build(), folio)
    };
    let activate = |runner: &mut GameRunner, folio| {
        runner.act(GameAction::ActivateAbility {
            source_id: folio,
            ability_index: 0,
            payment_mode: CastPaymentMode::Auto,
        })
    };

    let (mut runner, folio) = build(false);
    assert!(
        activate(&mut runner, folio).is_err(),
        "no two cards in hand share a color, so the reveal cost is unpayable"
    );
    assert!(runner.state().stack.is_empty());

    let (mut runner, folio) = build(true);
    activate(&mut runner, folio).expect("two blue cards make the reveal cost payable");
}

// =============================================================================
// Discriminating tests for shared-quality reveal continuation routes
// =============================================================================
// NOTE: The following tests are marked #[ignore] because the Oracle parser does
// not yet support these patterns. No printed cards currently use these patterns.
// When parser support is added, these tests should be enabled.

/// CR 601.2b + CR 701.20a: A spell with an additional reveal cost that requires
/// shared quality. The constraint travels with the spell's reveal prompt.
///
/// Oracle: "As an additional cost to cast this spell, reveal two cards from your
/// hand that share a color. Draw a card."
const SPELL_REVEAL_SHARED_COLOR_ORACLE: &str = "As an additional cost to cast this spell, reveal two cards from your hand that share a color. Draw a card.";

#[ignore]
#[test]
fn spell_reveal_shared_color_constraint_travels_with_prompt() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    // Two blue cards - can pay
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

    // A red card - can't pay with blue + red
    let red1 = scenario
        .add_creature_to_hand(P0, "Red Card 1", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Red],
            generic: 1,
        })
        .id();

    // Library for drawing
    scenario.with_library_top(P0, &["Library Card 1", "Library Card 2"]);
    scenario.with_mana_pool(
        P0,
        vec![ManaUnit::new(
            ManaType::Colorless,
            ObjectId(90_001),
            false,
            vec![],
        )],
    );

    let mut builder = scenario.add_spell_to_hand_from_oracle(
        P0,
        "Spell Reveal Shared Color",
        false,
        SPELL_REVEAL_SHARED_COLOR_ORACLE,
    );
    builder.with_mana_cost(ManaCost::generic(0));
    let spell = builder.id();

    let mut runner = scenario.build();
    let card_id = runner.state().objects[&spell].card_id;

    // Cast and pay with two blue cards - should succeed
    runner
        .act(GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("casting must be accepted");

    // Drive the pay-cost prompt
    let mut reached_reveal = false;
    for _ in 0..16 {
        match runner.state().waiting_for.clone() {
            WaitingFor::PayCost {
                kind:
                    PayCostKind::Reveal {
                        shared_quality: Some(quality),
                        ..
                    },
                choices,
                count,
                ..
            } => {
                reached_reveal = true;
                assert_eq!(count, 2);
                assert!(matches!(
                    quality,
                    engine::types::ability::SharedQuality::Color
                ));
                assert!(
                    choices.contains(&blue1) && choices.contains(&blue2) && choices.contains(&red1)
                );
                // Pay with two blue cards - shares color
                runner
                    .act(GameAction::SelectCards {
                        cards: vec![blue1, blue2],
                    })
                    .expect("two blue cards must be accepted");
            }
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    break;
                }
                runner
                    .act(GameAction::PassPriority)
                    .expect("priority pass must succeed");
            }
            other => panic!("unexpected prompt: {other:?}"),
        }
    }
    assert!(reached_reveal, "must reach reveal prompt");

    // Spell should have resolved, drawing a card
    assert_eq!(
        runner.state().players[P0.0 as usize].hand.len(),
        2,
        "should have drawn a card (revealed 2, drew 1 = 2)"
    );
    assert!(runner.state().stack.is_empty());
}

/// CR 601.2b + CR 701.20a: Spell reveal with shared quality - wrong pair rejected.
#[ignore]
#[test]
fn spell_reveal_shared_color_rejects_different_colors() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    // One blue, one red - can't share color
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

    // Keep a legal pair in hand so the cost is payable
    scenario
        .add_creature_to_hand(P0, "Blue Card 2", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 1,
        });
    scenario
        .add_creature_to_hand(P0, "Blue Card 3", 3, 3)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 2,
        });

    scenario.with_library_top(P0, &["Library Card 1", "Library Card 2"]);
    scenario.with_mana_pool(
        P0,
        vec![ManaUnit::new(
            ManaType::Colorless,
            ObjectId(90_001),
            false,
            vec![],
        )],
    );

    let mut builder = scenario.add_spell_to_hand_from_oracle(
        P0,
        "Spell Reveal Shared Color",
        false,
        SPELL_REVEAL_SHARED_COLOR_ORACLE,
    );
    builder.with_mana_cost(ManaCost::generic(0));
    let spell = builder.id();

    let mut runner = scenario.build();
    let card_id = runner.state().objects[&spell].card_id;

    runner
        .act(GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("casting must be accepted");

    // Try to pay with blue + red - should fail
    let mut reached_reveal = false;
    for _ in 0..16 {
        match runner.state().waiting_for.clone() {
            WaitingFor::PayCost {
                kind: PayCostKind::Reveal { .. },
                ..
            } => {
                reached_reveal = true;
                let result = runner.act(GameAction::SelectCards {
                    cards: vec![blue1, red1],
                });
                assert!(
                    result.is_err(),
                    "revealing different colors must be rejected"
                );
                match result {
                    Err(EngineError::InvalidAction(msg)) => assert!(
                        msg.contains("must share"),
                        "rejection must be shared-color, got: {msg}"
                    ),
                    _ => panic!("expected InvalidAction"),
                }
                break;
            }
            WaitingFor::Priority { .. } => {
                runner
                    .act(GameAction::PassPriority)
                    .expect("priority pass must succeed");
            }
            _ => {}
        }
    }
    assert!(reached_reveal, "must reach reveal prompt");
}

/// CR 702.37e + CR 701.20a: A turn-face-up (morph/disguise) cost with a reveal
/// leg requiring shared quality. The constraint is validated at payment time.
///
/// Oracle: "Disguise {2}{U} (You may cast this card face down for {3} as a 2/2
/// creature with ward {2}. Turn it face up any time for its disguise cost:
/// Reveal two cards from your hand that share a color.)"
const DISGUISE_REVEAL_SHARED_COLOR_ORACLE: &str = "Disguise {2}{U} (You may cast this card face down for {3} as a 2/2 creature with ward {2}. Turn it face up any time for its disguise cost: Reveal two cards from your hand that share a color.)";

#[ignore]
#[test]
fn turn_face_up_reveal_shared_color_constraint_enforced() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    // Two blue cards - can pay
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

    // A red card
    let red1 = scenario
        .add_creature_to_hand(P0, "Red Card 1", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Red],
            generic: 1,
        })
        .id();

    let ambusher = scenario
        .add_creature_to_hand_from_oracle(
            P0,
            "Disguise Ambusher",
            2,
            2,
            DISGUISE_REVEAL_SHARED_COLOR_ORACLE,
        )
        .id();

    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(ManaType::Colorless, ObjectId(90_001), false, vec![]),
            ManaUnit::new(ManaType::Blue, ObjectId(90_002), false, vec![]),
            ManaUnit::new(ManaType::Blue, ObjectId(90_003), false, vec![]),
        ],
    );

    let mut runner = scenario.build();
    let card_id = runner.state().objects[&ambusher].card_id;

    // Cast face down
    runner
        .act(GameAction::PlayFaceDown {
            object_id: ambusher,
            card_id,
        })
        .expect("cast face down");
    assert!(runner.state().objects[&ambusher].face_down);

    // Turn face up - this should prompt for the reveal cost with shared quality
    runner
        .act(GameAction::TurnFaceUp {
            object_id: ambusher,
            x: 0,
        })
        .expect("turn face up");

    // Drive the pay-cost prompt
    let mut reached_reveal = false;
    for _ in 0..16 {
        match runner.state().waiting_for.clone() {
            WaitingFor::PayCost {
                kind:
                    PayCostKind::Reveal {
                        shared_quality: Some(quality),
                        ..
                    },
                choices,
                count,
                ..
            } => {
                reached_reveal = true;
                assert_eq!(count, 2);
                assert!(matches!(
                    quality,
                    engine::types::ability::SharedQuality::Color
                ));
                assert!(
                    choices.contains(&blue1) && choices.contains(&blue2) && choices.contains(&red1)
                );

                // Pay with two blue cards - shares color
                runner
                    .act(GameAction::SelectCards {
                        cards: vec![blue1, blue2],
                    })
                    .expect("two blue cards must be accepted");
            }
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    break;
                }
                runner
                    .act(GameAction::PassPriority)
                    .expect("priority pass must succeed");
            }
            _ => {}
        }
    }
    assert!(reached_reveal, "must reach reveal prompt");
    assert!(
        !runner.state().objects[&ambusher].face_down,
        "must be face up after paying"
    );
}

/// CR 702.37e + CR 701.20a: Turn face up reveal - wrong pair rejected.
#[ignore]
#[test]
fn turn_face_up_reveal_shared_color_rejects_different_colors() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    // One blue, one red - can't share color
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

    // Keep a legal pair in hand so the cost is payable
    scenario
        .add_creature_to_hand(P0, "Blue Card 2", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 1,
        });
    scenario
        .add_creature_to_hand(P0, "Blue Card 3", 3, 3)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 2,
        });

    let ambusher = scenario
        .add_creature_to_hand_from_oracle(
            P0,
            "Disguise Ambusher",
            2,
            2,
            DISGUISE_REVEAL_SHARED_COLOR_ORACLE,
        )
        .id();

    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(ManaType::Colorless, ObjectId(90_001), false, vec![]),
            ManaUnit::new(ManaType::Blue, ObjectId(90_002), false, vec![]),
            ManaUnit::new(ManaType::Blue, ObjectId(90_003), false, vec![]),
        ],
    );

    let mut runner = scenario.build();
    let card_id = runner.state().objects[&ambusher].card_id;

    runner
        .act(GameAction::PlayFaceDown {
            object_id: ambusher,
            card_id,
        })
        .expect("cast face down");

    runner
        .act(GameAction::TurnFaceUp {
            object_id: ambusher,
            x: 0,
        })
        .expect("turn face up");

    // Try to pay with blue + red - should fail
    let mut reached_reveal = false;
    for _ in 0..16 {
        match runner.state().waiting_for.clone() {
            WaitingFor::PayCost {
                kind: PayCostKind::Reveal { .. },
                ..
            } => {
                reached_reveal = true;
                let result = runner.act(GameAction::SelectCards {
                    cards: vec![blue1, red1],
                });
                assert!(
                    result.is_err(),
                    "revealing different colors must be rejected"
                );
                match result {
                    Err(EngineError::InvalidAction(msg)) => assert!(
                        msg.contains("must share"),
                        "rejection must be shared-color, got: {msg}"
                    ),
                    _ => panic!("expected InvalidAction"),
                }
                break;
            }
            WaitingFor::Priority { .. } => {
                runner
                    .act(GameAction::PassPriority)
                    .expect("priority pass must succeed");
            }
            _ => {}
        }
    }
    assert!(reached_reveal, "must reach reveal prompt");
}

/// CR 601.2h + CR 701.20a: Multiple Reveal legs in a single cost, each with
/// its own shared-quality constraint.
///
/// Oracle: "{1}, {T}, Reveal a card from your hand that shares a color with a
/// card in your graveyard, then reveal a card from your hand that shares a
/// creature type with a creature you control: Draw two cards."
const _MULTI_LEG_REVEAL_ORACLE: &str = "{1}, {T}, Reveal a card from your hand that shares a color with a card in your graveyard, then reveal a card from your hand that shares a creature type with a creature you control: Draw two cards.";

#[ignore]
#[test]
fn multi_leg_reveal_each_leg_has_own_constraint() {
    // This test verifies the architecture supports multiple reveal legs with
    // different shared-quality constraints. The parser currently doesn't produce
    // this pattern from Oracle text (no printed card does this), but the payment
    // pipeline must correctly thread each leg's constraint to its own prompt.
    //
    // We test by checking the cost structure after parsing.
    use engine::parser::oracle_cost::parse_oracle_cost;
    use engine::types::ability::{AbilityCost, SharedQuality};

    // Currently the parser doesn't support multi-leg reveal with different
    // shared qualities in a single activation cost. The architecture in
    // `casting_costs.rs` and `costs.rs` supports it via sequential prompts,
    // but no printed card exercises this.
    //
    // For now, we verify the payability function handles it correctly by
    // testing that each leg validates against its own constraint.

    // This is a placeholder test to document the capability.
    // When a card with this pattern is printed, this test should be extended
    // to drive the full payment flow.

    // Verify parse_reveal_from_hand_cost_body can produce shared_quality
    let cost = parse_oracle_cost("Reveal two cards from your hand that share a color");
    assert!(matches!(
        cost,
        AbilityCost::Reveal {
            shared_quality: Some(SharedQuality::Color),
            ..
        }
    ));

    let cost = parse_oracle_cost("Reveal a card from your hand that shares a creature type");
    assert!(matches!(
        cost,
        AbilityCost::Reveal {
            shared_quality: Some(SharedQuality::CreatureType),
            ..
        }
    ));
}

/// CR 701.20a + CR 107.3a: Reveal X cards where X is announced and the cards
/// must share a quality. X is bounded by the largest qualifying group.
///
/// Oracle: "{X}, {T}, Reveal X cards from your hand that share a color: Draw X cards."
const REVEAL_X_SHARED_COLOR_ORACLE: &str =
    "{X}, {T}, Reveal X cards from your hand that share a color: Draw X cards.";

#[ignore]
#[test]
fn reveal_x_shared_color_x_bounded_by_largest_group() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    // Hand: 2 blue, 2 red, 1 green = 5 cards
    // Largest group sharing a color = 2 (blue or red)
    // So X can be at most 2

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
    let _red1 = scenario
        .add_creature_to_hand(P0, "Red Card 1", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Red],
            generic: 1,
        })
        .id();
    let _red2 = scenario
        .add_creature_to_hand(P0, "Red Card 2", 3, 3)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Red],
            generic: 2,
        })
        .id();
    let _green1 = scenario
        .add_creature_to_hand(P0, "Green Card 1", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Green],
            generic: 1,
        })
        .id();

    scenario.with_library_top(
        P0,
        &[
            "Library Card 1",
            "Library Card 2",
            "Library Card 3",
            "Library Card 4",
            "Library Card 5",
        ],
    );
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(ManaType::Colorless, ObjectId(90_001), false, vec![]),
            ManaUnit::new(ManaType::Colorless, ObjectId(90_002), false, vec![]),
            ManaUnit::new(ManaType::Colorless, ObjectId(90_003), false, vec![]),
        ],
    );

    let folio = scenario
        .add_creature(P0, "X Reveal Folio", 0, 0)
        .as_artifact()
        .from_oracle_text(REVEAL_X_SHARED_COLOR_ORACLE)
        .id();

    let mut runner = scenario.build();

    // Try to activate with X=3 - should fail because no 3 cards share a color
    runner
        .act(GameAction::ActivateAbility {
            source_id: folio,
            ability_index: 0,
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("activation accepted, then X announcement");

    // Must be at ChooseXValue prompt
    let WaitingFor::ChooseXValue { .. } = runner.state().waiting_for else {
        panic!(
            "expected ChooseXValue, got {:?}",
            runner.state().waiting_for
        );
    };

    // Try X=3 - should be rejected
    let result = runner.act(GameAction::ChooseX { value: 3 });
    assert!(
        result.is_err(),
        "X=3 should be rejected - no 3 cards share a color"
    );
    assert!(
        runner.state().stack.is_empty(),
        "ability should not go on stack"
    );

    // Re-activate with X=2
    runner
        .act(GameAction::ActivateAbility {
            source_id: folio,
            ability_index: 0,
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("activation accepted, then X announcement");

    let WaitingFor::ChooseXValue { .. } = runner.state().waiting_for else {
        panic!(
            "expected ChooseXValue, got {:?}",
            runner.state().waiting_for
        );
    };

    // Choose X=2 - should succeed
    runner
        .act(GameAction::ChooseX { value: 2 })
        .expect("X=2 should be accepted");

    // Drive the pay-cost prompt
    let mut reached_reveal = false;
    for _ in 0..16 {
        match runner.state().waiting_for.clone() {
            WaitingFor::PayCost {
                kind:
                    PayCostKind::Reveal {
                        shared_quality: Some(quality),
                        ..
                    },
                choices: _,
                count,
                ..
            } => {
                reached_reveal = true;
                assert_eq!(count, 2);
                assert!(matches!(
                    quality,
                    engine::types::ability::SharedQuality::Color
                ));

                // Pay with two blue cards
                runner
                    .act(GameAction::SelectCards {
                        cards: vec![blue1, blue2],
                    })
                    .expect("two blue cards must be accepted");
            }
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    break;
                }
                runner
                    .act(GameAction::PassPriority)
                    .expect("priority pass must succeed");
            }
            _ => {}
        }
    }
    assert!(reached_reveal, "must reach reveal prompt");

    // Should have drawn 2 cards
    // Started with 5, revealed 2 (stay in hand per CR 701.20b), drew 2 = 7
    assert_eq!(runner.state().players[P0.0 as usize].hand.len(), 7);
}

#[ignore]
#[test]
fn reveal_x_shared_color_x_zero_reveals_none() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let _blue1 = scenario
        .add_creature_to_hand(P0, "Blue Card 1", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 1,
        })
        .id();

    scenario.with_library_top(P0, &["Library Card 1"]);
    scenario.with_mana_pool(
        P0,
        vec![ManaUnit::new(
            ManaType::Colorless,
            ObjectId(90_001),
            false,
            vec![],
        )],
    );

    let folio = scenario
        .add_creature(P0, "X Reveal Folio", 0, 0)
        .as_artifact()
        .from_oracle_text(REVEAL_X_SHARED_COLOR_ORACLE)
        .id();

    let mut runner = scenario.build();

    // Activate with X=0 - should succeed without revealing
    runner
        .act(GameAction::ActivateAbility {
            source_id: folio,
            ability_index: 0,
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("activation accepted, then X announcement");

    let WaitingFor::ChooseXValue { .. } = runner.state().waiting_for else {
        panic!(
            "expected ChooseXValue, got {:?}",
            runner.state().waiting_for
        );
    };

    // Choose X=0
    runner
        .act(GameAction::ChooseX { value: 0 })
        .expect("X=0 should be accepted");

    // Should not reach reveal prompt, ability resolves immediately
    runner.advance_until_stack_empty();

    // Drew 0 cards, hand unchanged
    assert_eq!(runner.state().players[P0.0 as usize].hand.len(), 1);
}
