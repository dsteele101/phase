//! CR 702.37a + CR 702.37e + CR 118.12: Fathom Seer non-mana morph cost integration test.
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
}
