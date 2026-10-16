//! CR 702.37e + CR 116.2b + CR 118.1 + CR 118.3 + CR 118.11 + CR 614.6 + CR 616.1:
//! Non-mana morph cost integration tests.
//!
//! Fathom Seer:
//! "Morph—Return two Islands you control to their owner's hand.
//! When this creature is turned face up, draw two cards."

use engine::game::scenario::{GameScenario, P0};
use engine::types::actions::{AlternativeCastDecision, GameAction};
use engine::types::game_state::{AlternativeCastKeyword, CastPaymentMode, PayCostKind, WaitingFor};
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

#[test]
fn fathom_seer_cast_face_down_and_turn_face_up_paying_islands() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    // 2 Islands on battlefield controlled by P0
    let island1 = scenario.add_basic_land(P0, ManaColor::Blue);
    let island2 = scenario.add_basic_land(P0, ManaColor::Blue);

    // Fathom Seer in P0 hand
    let seer = scenario
        .add_creature_to_hand(P0, "Fathom Seer", 1, 3)
        .from_oracle_text(
            "Morph—Return two Islands you control to their owner's hand.\nWhen this creature is turned face up, draw two cards.",
        )
        .with_mana_cost(ManaCost::Cost {
            generic: 1,
            shards: vec![ManaCostShard::Blue],
        })
        .id();

    // Give P0 3 colorless mana to cast face-down
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                seer,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                seer,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                seer,
                false,
                vec![],
            ),
        ],
    );

    // Put some cards in P0's library so drawing succeeds
    for _ in 0..5 {
        scenario.add_card_to_library_top(P0, "Library Card");
    }

    let mut runner = scenario.build();

    // 1. Cast face down for {3}
    let card_id = runner.state().objects[&seer].card_id;
    let res = runner.act(GameAction::CastSpell {
        object_id: seer,
        card_id,
        targets: vec![],
        payment_mode: CastPaymentMode::Auto,
    });
    assert!(
        res.is_ok(),
        "face-down cast should succeed: {:?}",
        res.err()
    );
    // CR 601.2b / CR 702.37c: Since both normal cast and face-down cast are
    // affordable, the engine surfaces WaitingFor::AlternativeCastChoice with FaceDown.
    match &runner.state().waiting_for {
        WaitingFor::AlternativeCastChoice { keyword, .. } => {
            assert_eq!(*keyword, AlternativeCastKeyword::FaceDown);
            runner
                .act(GameAction::ChooseAlternativeCast {
                    choice: AlternativeCastDecision::Alternative,
                })
                .expect("choosing face down alternative cast should succeed");
        }
        other => panic!("expected AlternativeCastChoice(FaceDown), got {other:?}"),
    }

    // Resolve the face-down creature spell on the stack
    runner.resolve_top();

    // Verify it is on the battlefield and face down
    assert_eq!(runner.state().objects[&seer].zone, Zone::Battlefield);
    assert!(runner.state().objects[&seer].face_down);
    assert_eq!(runner.state().objects[&seer].power, Some(2));
    assert_eq!(runner.state().objects[&seer].toughness, Some(2));

    // 2. Turn face up
    let res = runner.act(GameAction::TurnFaceUp {
        object_id: seer,
        x: 0,
    });
    assert!(
        res.is_ok(),
        "turn face up action should succeed: {:?}",
        res.err()
    );

    // Should be waiting for PayCost::ReturnToHand
    match &runner.state().waiting_for {
        WaitingFor::PayCost {
            kind: PayCostKind::ReturnToHand,
            choices,
            count,
            ..
        } => {
            assert_eq!(*count, 2);
            assert!(choices.contains(&island1));
            assert!(choices.contains(&island2));
        }
        other => panic!("expected WaitingFor::PayCost(ReturnToHand), got {other:?}"),
    }

    // 3. Select the 2 Islands to return
    let res = runner.act(GameAction::SelectCards {
        cards: vec![island1, island2],
    });
    assert!(
        res.is_ok(),
        "selecting cards to pay cost should succeed: {:?}",
        res.err()
    );

    // Verify Islands returned to hand
    assert_eq!(runner.state().objects[&island1].zone, Zone::Hand);
    assert_eq!(runner.state().objects[&island2].zone, Zone::Hand);

    // Verify Fathom Seer is face up
    assert!(!runner.state().objects[&seer].face_down);
    assert_eq!(runner.state().objects[&seer].name, "Fathom Seer");
    assert_eq!(runner.state().objects[&seer].power, Some(1));
    assert_eq!(runner.state().objects[&seer].toughness, Some(3));

    // Resolve the turn face-up trigger ("draw two cards")
    let hand_before = runner.state().players[0].hand.len();
    runner.resolve_top();
    let hand_after = runner.state().players[0].hand.len();
    assert_eq!(
        hand_after - hand_before,
        2,
        "must have drawn two cards from turn face up trigger"
    );
    assert!(
        runner.state().stack.is_empty(),
        "stack must be empty after resolving trigger"
    );
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::Priority { .. }),
        "state must be at Priority boundary"
    );
}

// CR 118.1 + CR 118.3 + CR 702.37e: Multi-object cost payment requires distinct IDs.
#[test]
fn fathom_seer_duplicate_island_selection_rejected() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let island1 = scenario.add_basic_land(P0, ManaColor::Blue);
    let island2 = scenario.add_basic_land(P0, ManaColor::Blue);
    let seer = scenario
        .add_creature_to_hand(P0, "Fathom Seer", 1, 3)
        .from_oracle_text(
            "Morph—Return two Islands you control to their owner's hand.\nWhen this creature is turned face up, draw two cards.",
        )
        .with_mana_cost(ManaCost::Cost {
            generic: 1,
            shards: vec![ManaCostShard::Blue],
        })
        .id();
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                seer,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                seer,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                seer,
                false,
                vec![],
            ),
        ],
    );
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&seer].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: seer,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast face down");
    if let WaitingFor::AlternativeCastChoice { .. } = runner.state().waiting_for {
        runner
            .act(GameAction::ChooseAlternativeCast {
                choice: AlternativeCastDecision::Alternative,
            })
            .expect("choose face down");
    }
    runner.resolve_top();
    assert!(runner.state().objects[&seer].face_down);

    runner
        .act(GameAction::TurnFaceUp {
            object_id: seer,
            x: 0,
        })
        .expect("turn face up");

    // Guard exact prompt before duplicate selection (N3)
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::PayCost {
                kind: PayCostKind::ReturnToHand,
                count: 2,
                ..
            }
        ),
        "expected WaitingFor::PayCost(ReturnToHand, count=2), got {:?}",
        runner.state().waiting_for
    );

    // Supplying duplicate ID [island1, island1] must be rejected
    let res = runner.act(GameAction::SelectCards {
        cards: vec![island1, island1],
    });
    assert!(res.is_err(), "must reject duplicate island IDs");
    // Board and face-down state must remain unchanged
    assert!(runner.state().objects[&seer].face_down);
    assert_eq!(runner.state().objects[&island1].zone, Zone::Battlefield);
    assert_eq!(runner.state().objects[&island2].zone, Zone::Battlefield);
}

// CR 702.37e + CR 118.3: Insufficient resources to pay turn-face-up cost
#[test]
fn fathom_seer_insufficient_islands_cannot_turn_face_up() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let island1 = scenario.add_basic_land(P0, ManaColor::Blue);
    let seer = scenario
        .add_creature_to_hand(P0, "Fathom Seer", 1, 3)
        .from_oracle_text(
            "Morph—Return two Islands you control to their owner's hand.\nWhen this creature is turned face up, draw two cards.",
        )
        .with_mana_cost(ManaCost::Cost {
            generic: 1,
            shards: vec![ManaCostShard::Blue],
        })
        .id();
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                seer,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                seer,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                seer,
                false,
                vec![],
            ),
        ],
    );
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&seer].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: seer,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast face down");
    if let WaitingFor::AlternativeCastChoice { .. } = runner.state().waiting_for {
        runner
            .act(GameAction::ChooseAlternativeCast {
                choice: AlternativeCastDecision::Alternative,
            })
            .expect("choose face down");
    }
    runner.resolve_top();
    assert!(runner.state().objects[&seer].face_down);

    // Attempting to turn face up must be rejected
    let res = runner.act(GameAction::TurnFaceUp {
        object_id: seer,
        x: 0,
    });
    assert!(
        res.is_err(),
        "must reject turn face up when lacking required Islands"
    );
    assert!(runner.state().objects[&seer].face_down);
    assert_eq!(runner.state().objects[&island1].zone, Zone::Battlefield);
}

// CR 702.37e + CR 118.11 + CR 701.9: Discard as turn-face-up cost
#[test]
fn gathan_raiders_turn_face_up_discard_cost() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let discard_card = scenario.add_card_to_hand(P0, "Mountain");
    let raiders = scenario
        .add_creature_to_hand(P0, "Gathan Raiders", 3, 3)
        .from_oracle_text("Morph—Discard a card.")
        .with_mana_cost(ManaCost::Cost {
            generic: 3,
            shards: vec![ManaCostShard::Red, ManaCostShard::Red],
        })
        .id();
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                raiders,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                raiders,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                raiders,
                false,
                vec![],
            ),
        ],
    );
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&raiders].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: raiders,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast face down");
    if let WaitingFor::AlternativeCastChoice { .. } = runner.state().waiting_for {
        runner
            .act(GameAction::ChooseAlternativeCast {
                choice: AlternativeCastDecision::Alternative,
            })
            .expect("choose face down");
    }
    runner.resolve_top();
    assert!(runner.state().objects[&raiders].face_down);

    runner
        .act(GameAction::TurnFaceUp {
            object_id: raiders,
            x: 0,
        })
        .expect("turn face up");
    match &runner.state().waiting_for {
        WaitingFor::PayCost {
            kind: PayCostKind::Discard,
            count,
            choices,
            ..
        } => {
            assert_eq!(*count, 1);
            assert!(choices.contains(&discard_card));
        }
        other => panic!("expected PayCost(Discard), got {other:?}"),
    }
    let res = runner.act(GameAction::SelectCards {
        cards: vec![discard_card],
    });
    assert!(res.is_ok(), "discarding card as cost succeeds: {res:?}");
    assert_eq!(runner.state().objects[&discard_card].zone, Zone::Graveyard);
    assert!(!runner.state().objects[&raiders].face_down);
    assert_eq!(runner.state().objects[&raiders].power, Some(3));
}

// CR 702.37e + CR 701.20a + CR 701.20b: Reveal as turn-face-up cost
#[test]
fn ruthless_ripper_turn_face_up_reveal_cost() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let black_card = scenario
        .add_creature_to_hand(P0, "Black Creature", 1, 1)
        .with_mana_cost(ManaCost::Cost {
            generic: 0,
            shards: vec![ManaCostShard::Black],
        })
        .id();
    let ripper = scenario
        .add_creature_to_hand(P0, "Ruthless Ripper", 1, 1)
        .from_oracle_text("Deathtouch\nMorph—Reveal a black card in your hand.")
        .with_mana_cost(ManaCost::Cost {
            generic: 0,
            shards: vec![ManaCostShard::Black],
        })
        .id();
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                ripper,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                ripper,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                ripper,
                false,
                vec![],
            ),
        ],
    );
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&ripper].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: ripper,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast face down");
    if let WaitingFor::AlternativeCastChoice { .. } = runner.state().waiting_for {
        runner
            .act(GameAction::ChooseAlternativeCast {
                choice: AlternativeCastDecision::Alternative,
            })
            .expect("choose face down");
    }
    runner.resolve_top();
    assert!(runner.state().objects[&ripper].face_down);

    runner
        .act(GameAction::TurnFaceUp {
            object_id: ripper,
            x: 0,
        })
        .expect("turn face up");
    match &runner.state().waiting_for {
        WaitingFor::PayCost {
            kind: PayCostKind::Reveal,
            count,
            choices,
            ..
        } => {
            assert_eq!(*count, 1);
            assert!(choices.contains(&black_card));
        }
        other => panic!("expected PayCost(Reveal), got {other:?}"),
    }
    runner
        .act(GameAction::SelectCards {
            cards: vec![black_card],
        })
        .expect("reveal card");
    assert!(!runner.state().objects[&ripper].face_down);
    assert_eq!(runner.state().objects[&black_card].zone, Zone::Hand);
}

// CR 702.37e + CR 701.21a: Sacrifice as turn-face-up cost
#[test]
fn skirk_volcanist_turn_face_up_sacrifice_cost() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mountain1 = scenario.add_basic_land(P0, ManaColor::Red);
    let mountain2 = scenario.add_basic_land(P0, ManaColor::Red);
    let volcanist = scenario
        .add_creature_to_hand(P0, "Skirk Volcanist", 3, 1)
        .from_oracle_text("Morph—Sacrifice two Mountains.")
        .with_mana_cost(ManaCost::Cost {
            generic: 3,
            shards: vec![ManaCostShard::Red],
        })
        .id();
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                volcanist,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                volcanist,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                volcanist,
                false,
                vec![],
            ),
        ],
    );
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&volcanist].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: volcanist,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast face down");
    if let WaitingFor::AlternativeCastChoice { .. } = runner.state().waiting_for {
        runner
            .act(GameAction::ChooseAlternativeCast {
                choice: AlternativeCastDecision::Alternative,
            })
            .expect("choose face down");
    }
    runner.resolve_top();
    assert!(runner.state().objects[&volcanist].face_down);

    runner
        .act(GameAction::TurnFaceUp {
            object_id: volcanist,
            x: 0,
        })
        .expect("turn face up");
    match &runner.state().waiting_for {
        WaitingFor::PayCost {
            kind: PayCostKind::Sacrifice,
            count,
            choices,
            ..
        } => {
            assert_eq!(*count, 2);
            assert!(choices.contains(&mountain1));
            assert!(choices.contains(&mountain2));
        }
        other => panic!("expected PayCost(Sacrifice), got {other:?}"),
    }
    let res = runner
        .act(GameAction::SelectCards {
            cards: vec![mountain1, mountain2],
        })
        .expect("sacrifice mountains");
    assert!(!runner.state().objects[&volcanist].face_down);
    assert_eq!(runner.state().objects[&mountain1].zone, Zone::Graveyard);
    assert_eq!(runner.state().objects[&mountain2].zone, Zone::Graveyard);

    let sac_events: Vec<_> = res
        .events
        .iter()
        .filter(|e| {
            matches!(
                e,
                engine::types::events::GameEvent::PermanentSacrificed { .. }
            )
        })
        .collect();
    assert_eq!(
        sac_events.len(),
        2,
        "must emit PermanentSacrificed for both Mountains"
    );
}

// CR 702.37e + CR 119: Pay life as turn-face-up cost
#[test]
fn zombie_cutthroat_turn_face_up_pay_life_cost() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let cutthroat = scenario
        .add_creature_to_hand(P0, "Zombie Cutthroat", 3, 4)
        .from_oracle_text("Morph—Pay 5 life.")
        .with_mana_cost(ManaCost::Cost {
            generic: 3,
            shards: vec![ManaCostShard::Black, ManaCostShard::Black],
        })
        .id();
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                cutthroat,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                cutthroat,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                cutthroat,
                false,
                vec![],
            ),
        ],
    );
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&cutthroat].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: cutthroat,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast face down");
    if let WaitingFor::AlternativeCastChoice { .. } = runner.state().waiting_for {
        runner
            .act(GameAction::ChooseAlternativeCast {
                choice: AlternativeCastDecision::Alternative,
            })
            .expect("choose face down");
    }
    runner.resolve_top();
    assert!(runner.state().objects[&cutthroat].face_down);
    assert_eq!(runner.state().players[0].life, 20);

    runner
        .act(GameAction::TurnFaceUp {
            object_id: cutthroat,
            x: 0,
        })
        .expect("turn face up");
    assert!(!runner.state().objects[&cutthroat].face_down);
    assert_eq!(runner.state().players[0].life, 15);
}

// CR 702.37e + F5: Serialization round-trip for MorphCost and CostResume::TurnFaceUp
#[test]
fn morph_cost_and_cost_resume_turn_face_up_serialization_round_trip() {
    use engine::types::ability::{AbilityCost, QuantityExpr, TurnUpCostSource};
    use engine::types::game_state::CostResume;
    use engine::types::identifiers::ObjectId;
    use engine::types::keywords::MorphCost;

    let morph_cost = MorphCost::NonMana(AbilityCost::ReturnToHand {
        count: 2,
        filter: None,
        from_zone: None,
    });
    let json = serde_json::to_string(&morph_cost).expect("serialize morph_cost");
    let deserialized: MorphCost = serde_json::from_str(&json).expect("deserialize morph_cost");
    assert_eq!(morph_cost, deserialized);

    let cost_resume = CostResume::TurnFaceUp {
        object_id: ObjectId(42),
        cost_source: TurnUpCostSource::Morph,
        remaining_cost: Some(Box::new(AbilityCost::PayLife {
            amount: QuantityExpr::Fixed { value: 5 },
        })),
    };
    let json = serde_json::to_string(&cost_resume).expect("serialize cost_resume");
    let deserialized: CostResume = serde_json::from_str(&json).expect("deserialize cost_resume");
    assert_eq!(cost_resume, deserialized);
}

// F3 + F5: Coverage check for MorphCost support
#[test]
fn morph_cost_unsupported_coverage_check() {
    use engine::types::ability::AbilityCost;
    use engine::types::keywords::{is_ability_cost_supported_for_morph, MorphCost};

    let supported = MorphCost::NonMana(AbilityCost::ReturnToHand {
        count: 2,
        filter: None,
        from_zone: None,
    });
    assert!(supported.is_supported());

    let unsupported = MorphCost::NonMana(AbilityCost::Tap);
    assert!(!unsupported.is_supported());
    assert!(!is_ability_cost_supported_for_morph(&AbilityCost::Tap));

    // CR 702.37e: Composite is unsupported for morph (C2/C5)
    let composite = MorphCost::NonMana(AbilityCost::Composite {
        costs: vec![AbilityCost::Tap],
    });
    assert!(!composite.is_supported());
    assert!(!is_ability_cost_supported_for_morph(
        &AbilityCost::Composite {
            costs: vec![AbilityCost::Tap],
        }
    ));
}

// CR 702.37e + CR 701.21a + CR 614.6 + CR 616.1: Sacrifice turn-face-up cost with replacement choice
#[test]
fn skirk_volcanist_turn_face_up_sacrifice_replacement_pause() {
    use engine::types::ability::{
        AbilityDefinition, AbilityKind, Effect, ReplacementMode, TargetFilter,
    };
    use engine::types::replacements::ReplacementEvent;
    use engine::types::zones::EtbTapState;
    use engine::types::ReplacementDefinition;

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mountain1 = scenario.add_basic_land(P0, ManaColor::Red);
    let mountain2 = scenario.add_basic_land(P0, ManaColor::Red);
    let volcanist = scenario
        .add_creature_to_hand(P0, "Skirk Volcanist", 3, 1)
        .from_oracle_text("Morph—Sacrifice two Mountains.")
        .with_mana_cost(ManaCost::Cost {
            generic: 3,
            shards: vec![ManaCostShard::Red],
        })
        .id();
    // Add optional replacement redirect for graveyard moves
    scenario
        .add_creature(engine::game::scenario::P1, "Redirector", 1, 1)
        .with_replacement_definition(
            ReplacementDefinition::new(ReplacementEvent::Moved)
                .destination_zone(Zone::Graveyard)
                .mode(ReplacementMode::Optional { decline: None })
                .execute(AbilityDefinition::new(
                    AbilityKind::Spell,
                    Effect::ChangeZone {
                        origin: None,
                        destination: Zone::Exile,
                        target: TargetFilter::SelfRef,
                        owner_library: false,
                        enter_transformed: false,
                        enters_under: None,
                        enter_tapped: EtbTapState::Unspecified,
                        enters_attacking: false,
                        up_to: false,
                        enter_with_counters: vec![],
                        conditional_enter_with_counters: vec![],
                        enters_modified_if: None,
                        face_down_profile: None,
                    },
                )),
        );
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                volcanist,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                volcanist,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                volcanist,
                false,
                vec![],
            ),
        ],
    );
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&volcanist].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: volcanist,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast face down");
    if let WaitingFor::AlternativeCastChoice { .. } = runner.state().waiting_for {
        runner
            .act(GameAction::ChooseAlternativeCast {
                choice: AlternativeCastDecision::Alternative,
            })
            .expect("choose face down");
    }
    runner.resolve_top();

    runner
        .act(GameAction::TurnFaceUp {
            object_id: volcanist,
            x: 0,
        })
        .expect("turn face up");

    runner
        .act(GameAction::SelectCards {
            cards: vec![mountain1, mountain2],
        })
        .expect("select mountains");

    // Pauses for optional replacement on first mountain
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::ReplacementChoice { .. }
        ),
        "expected ReplacementChoice pause, got {:?}",
        runner.state().waiting_for
    );
    assert!(
        runner.state().pending_cost_move_resume.is_some(),
        "must retain PendingCostMoveResume across replacement pause"
    );

    // Accept replacement (exiles instead of graveyard)
    runner
        .act(GameAction::ChooseReplacement { index: 0 })
        .expect("choose replacement");

    // May pause again for second mountain, or complete
    while matches!(
        runner.state().waiting_for,
        WaitingFor::ReplacementChoice { .. }
    ) {
        runner
            .act(GameAction::ChooseReplacement { index: 0 })
            .expect("choose replacement");
    }

    // Must be face up now
    assert!(!runner.state().objects[&volcanist].face_down);
    assert!(
        runner.state().pending_cost_move_resume.is_none(),
        "pending_cost_move_resume must be cleared"
    );
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::Priority { .. }),
        "must complete at Priority boundary, got {:?}",
        runner.state().waiting_for
    );
}

// CR 702.37e + CR 605.3b + CR 616.1: Cost move root resumes at common PriorityBoundary after child prompt drains
#[test]
fn turn_face_up_cost_move_resumes_after_mandatory_child_drains() {
    use engine::types::ability::TurnUpCostSource;
    use engine::types::game_state::{PendingCostMoveResume, TurnFaceUpMoveKind};

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let island1 = scenario.add_basic_land(P0, ManaColor::Blue);
    let island2 = scenario.add_basic_land(P0, ManaColor::Blue);
    let seer = scenario
        .add_creature_to_hand(P0, "Fathom Seer", 1, 3)
        .from_oracle_text("Morph—Return two Islands you control to their owner's hand.")
        .with_mana_cost(ManaCost::Cost {
            generic: 1,
            shards: vec![ManaCostShard::Blue],
        })
        .id();
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                seer,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                seer,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                seer,
                false,
                vec![],
            ),
        ],
    );
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&seer].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: seer,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast face down");
    if let WaitingFor::AlternativeCastChoice { .. } = runner.state().waiting_for {
        runner
            .act(GameAction::ChooseAlternativeCast {
                choice: AlternativeCastDecision::Alternative,
            })
            .expect("choose face down");
    }
    runner.resolve_top();
    assert!(runner.state().objects[&seer].face_down);

    // Install TurnFaceUpCostMove and a mandatory child prompt (ChooseOneOf)
    runner.state_mut().pending_cost_move_resume = Some(PendingCostMoveResume::TurnFaceUpCostMove {
        player: P0,
        object_id: seer,
        cost_source: TurnUpCostSource::Morph,
        move_kind: TurnFaceUpMoveKind::ReturnToHand,
        chosen: vec![island1, island2],
        paused_at_index: 0,
        remaining_cost: None,
    });
    runner.state_mut().waiting_for = WaitingFor::NamedChoice {
        player: P0,
        choice_type: engine::types::ability::ChoiceType::OddOrEven,
        options: vec!["Odd".to_string(), "Even".to_string()],
        source: None,
        persist_player: None,
        free_entry: None,
    };

    // Submitting the choice must drain the child prompt, reach PriorityBoundary,
    // resume TurnFaceUpCostMove, finish moving island2, and flip Fathom Seer.
    let res = runner.act(GameAction::ChooseOption {
        choice: "Odd".to_string(),
    });
    assert!(res.is_ok(), "choice action should succeed: {:?}", res.err());

    assert_eq!(runner.state().objects[&island2].zone, Zone::Hand);
    assert!(
        !runner.state().objects[&seer].face_down,
        "Fathom Seer must be flipped face up"
    );
    assert!(
        runner.state().pending_cost_move_resume.is_none(),
        "pending_cost_move_resume must be cleared"
    );
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::Priority { .. }),
        "state must settle at Priority boundary, got {:?}",
        runner.state().waiting_for
    );
}

// CR 702.37e + CR 614.6 + CR 616.1: Deferred life cost depth pre-capture ensures child tail completes before flip
#[test]
fn zombie_cutthroat_deferred_life_cost_child_drain_before_flip() {
    use engine::types::ability::TurnUpCostSource;
    use engine::types::game_state::DeferredLifeCostResume;

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let cutthroat = scenario
        .add_creature_to_hand(P0, "Zombie Cutthroat", 3, 4)
        .from_oracle_text("Morph—Pay 5 life.")
        .with_mana_cost(ManaCost::Cost {
            generic: 3,
            shards: vec![ManaCostShard::Black, ManaCostShard::Black],
        })
        .id();
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                cutthroat,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                cutthroat,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                cutthroat,
                false,
                vec![],
            ),
        ],
    );
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&cutthroat].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: cutthroat,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast face down");
    if let WaitingFor::AlternativeCastChoice { .. } = runner.state().waiting_for {
        runner
            .act(GameAction::ChooseAlternativeCast {
                choice: AlternativeCastDecision::Alternative,
            })
            .expect("choose face down");
    }
    runner.resolve_top();
    assert!(runner.state().objects[&cutthroat].face_down);
    assert_eq!(runner.state().players[0].life, 20);

    // Deduct 5 life for the cost payment
    runner.state_mut().players[0].life -= 5;

    // Simulate deferred life continuation with depth 0 and active child prompt
    runner.state_mut().pending_deferred_life_cost_resume =
        Some(DeferredLifeCostResume::TurnFaceUp {
            player: P0,
            object_id: cutthroat,
            cost_source: TurnUpCostSource::Morph,
            resume_at_resolution_depth: 0,
            remaining_cost: None,
        });
    runner.state_mut().waiting_for = WaitingFor::NamedChoice {
        player: P0,
        choice_type: engine::types::ability::ChoiceType::OddOrEven,
        options: vec!["Odd".to_string(), "Even".to_string()],
        source: None,
        persist_player: None,
        free_entry: None,
    };

    // Submitting child answer drains back to resolution depth 0, triggering DeferredLifeCostResume::TurnFaceUp
    let res = runner.act(GameAction::ChooseOption {
        choice: "Odd".to_string(),
    });
    assert!(res.is_ok(), "choice action should succeed: {:?}", res.err());

    // Creature is now turned face up
    assert!(!runner.state().objects[&cutthroat].face_down);
    // Life was deducted exactly once (15)
    assert_eq!(runner.state().players[0].life, 15);
    assert!(
        runner.state().pending_deferred_life_cost_resume.is_none(),
        "deferred life root must be cleared"
    );
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::Priority { .. }),
        "must be at Priority, got {:?}",
        runner.state().waiting_for
    );
}

// CR 702.37e + CR 601.2h + CR 614.6: Preserve ordinary delivery children across sacrifice cost payment
#[test]
fn turn_face_up_sacrifice_cost_preserves_delivery_children_until_drain() {
    use engine::types::ability::{
        AbilityDefinition, AbilityKind, ChoiceType, Effect, ReplacementDefinition, TargetFilter,
        TargetSelectionMode,
    };
    use engine::types::game_state::PendingCostMoveResume;
    use engine::types::replacements::ReplacementEvent;

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mountain1 = scenario.add_basic_land(P0, ManaColor::Red);
    let mountain2 = scenario.add_basic_land(P0, ManaColor::Red);
    let volcanist = scenario
        .add_creature_to_hand(P0, "Skirk Volcanist", 3, 1)
        .from_oracle_text("Morph—Sacrifice two Mountains.")
        .with_mana_cost(ManaCost::Cost {
            generic: 3,
            shards: vec![ManaCostShard::Red],
        })
        .id();
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                volcanist,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                volcanist,
                false,
                vec![],
            ),
            ManaUnit::new(
                engine::types::mana::ManaType::Colorless,
                volcanist,
                false,
                vec![],
            ),
        ],
    );
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&volcanist].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: volcanist,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast face down");
    if let WaitingFor::AlternativeCastChoice { .. } = runner.state().waiting_for {
        runner
            .act(GameAction::ChooseAlternativeCast {
                choice: AlternativeCastDecision::Alternative,
            })
            .expect("choose face down");
    }
    runner.resolve_top();
    assert!(runner.state().objects[&volcanist].face_down);

    // Install delivery-child replacement effect on each Mountain (moved to graveyard -> choose odd or even)
    let delivery_repl = ReplacementDefinition::new(ReplacementEvent::Moved)
        .execute(AbilityDefinition::new(
            AbilityKind::Spell,
            Effect::Choose {
                choice_type: ChoiceType::OddOrEven,
                persist: false,
                selection: TargetSelectionMode::Chosen,
            },
        ))
        .valid_card(TargetFilter::SelfRef)
        .destination_zone(Zone::Graveyard);

    std::sync::Arc::make_mut(
        &mut runner
            .state_mut()
            .objects
            .get_mut(&mountain1)
            .unwrap()
            .base_replacement_definitions,
    )
    .push(delivery_repl.clone());
    runner
        .state_mut()
        .objects
        .get_mut(&mountain1)
        .unwrap()
        .replacement_definitions
        .push(delivery_repl.clone());

    std::sync::Arc::make_mut(
        &mut runner
            .state_mut()
            .objects
            .get_mut(&mountain2)
            .unwrap()
            .base_replacement_definitions,
    )
    .push(delivery_repl.clone());
    runner
        .state_mut()
        .objects
        .get_mut(&mountain2)
        .unwrap()
        .replacement_definitions
        .push(delivery_repl);

    // Public TurnFaceUp special action
    let res = runner.act(GameAction::TurnFaceUp {
        object_id: volcanist,
        x: 0,
    });
    assert!(res.is_ok(), "TurnFaceUp must succeed: {:?}", res.err());
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::PayCost {
                kind: PayCostKind::Sacrifice,
                ..
            }
        ),
        "must be WaitingFor::PayCost(Sacrifice), got {:?}",
        runner.state().waiting_for
    );

    // Submit SelectCards with the two Mountains
    let res = runner.act(GameAction::SelectCards {
        cards: vec![mountain1, mountain2],
    });
    assert!(res.is_ok(), "SelectCards must succeed: {:?}", res.err());

    // First mountain moved to graveyard; its delivery tail paused on NamedChoice (not clobbered by replacement_choice)
    assert_eq!(runner.state().objects[&mountain1].zone, Zone::Graveyard);
    assert_eq!(runner.state().objects[&mountain2].zone, Zone::Battlefield);
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::NamedChoice {
                choice_type: ChoiceType::OddOrEven,
                ..
            }
        ),
        "must be on first Mountain's NamedChoice prompt, got {:?}",
        runner.state().waiting_for
    );
    assert!(
        runner.state().objects[&volcanist].face_down,
        "permanent must remain face down while payment is paused"
    );
    assert!(
        matches!(
            runner.state().pending_cost_move_resume,
            Some(PendingCostMoveResume::TurnFaceUpCostMove {
                paused_at_index: 0,
                ..
            })
        ),
        "must have TurnFaceUpCostMove paused at index 0"
    );

    // Answer first child prompt
    let res = runner.act(GameAction::ChooseOption {
        choice: "Odd".to_string(),
    });
    assert!(
        res.is_ok(),
        "first ChooseOption must succeed: {:?}",
        res.err()
    );

    // Second mountain moved to graveyard; its delivery tail paused on second NamedChoice
    assert_eq!(runner.state().objects[&mountain2].zone, Zone::Graveyard);
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::NamedChoice {
                choice_type: ChoiceType::OddOrEven,
                ..
            }
        ),
        "must be on second Mountain's NamedChoice prompt, got {:?}",
        runner.state().waiting_for
    );
    assert!(
        runner.state().objects[&volcanist].face_down,
        "permanent must remain face down while second payment is paused"
    );
    assert!(
        matches!(
            runner.state().pending_cost_move_resume,
            Some(PendingCostMoveResume::TurnFaceUpCostMove {
                paused_at_index: 1,
                ..
            })
        ),
        "must have TurnFaceUpCostMove paused at index 1"
    );

    // Answer second child prompt
    let res = runner.act(GameAction::ChooseOption {
        choice: "Even".to_string(),
    });
    assert!(
        res.is_ok(),
        "second ChooseOption must succeed: {:?}",
        res.err()
    );

    // Exactly one flip: permanent is turned face up
    assert!(
        !runner.state().objects[&volcanist].face_down,
        "Skirk Volcanist must be turned face up after all costs and children finish"
    );
    // Roots cleared
    assert!(
        runner.state().pending_cost_move_resume.is_none(),
        "pending_cost_move_resume must be cleared"
    );
    // At Priority
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::Priority { .. }),
        "state must settle at Priority, got {:?}",
        runner.state().waiting_for
    );
}
