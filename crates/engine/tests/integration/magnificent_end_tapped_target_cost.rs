//! CR 601.2f + CR 115.9b: "This spell costs {3} less to cast if it targets a
//! tapped creature" (Magnificent End) reduces the total cost only when the
//! chosen target is tapped.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const MAGNIFICENT_END: &str = "This spell costs {3} less to cast if it targets a tapped creature.\nMagnificent End deals 5 damage to target creature.";

/// Cast the spell at one creature (tapped or not) holding `mana` colorless
/// mana; returns the runner, the spell, and the mana left in the pool.
fn cast_at(target_tapped: bool, mana: usize) -> (GameRunner, ObjectId, Option<usize>) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let victim = scenario.add_creature(P1, "Mark", 2, 2).id();
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Final Verdict", false, MAGNIFICENT_END)
        .as_instant()
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::White],
            generic: 4,
        })
        .id();
    for _ in 0..mana {
        scenario.add_basic_land(P0, ManaColor::White);
    }
    let mut runner = scenario.build();
    runner.state_mut().objects.get_mut(&victim).unwrap().tapped = target_tapped;
    let card_id = runner.state().objects[&spell].card_id;
    let mut action = GameAction::CastSpell {
        object_id: spell,
        card_id,
        targets: vec![],
        payment_mode: CastPaymentMode::Auto,
    };
    for _ in 0..8 {
        if runner.act(action).is_err() {
            return (runner, spell, None);
        }
        action = match runner.state().waiting_for.clone() {
            WaitingFor::TargetSelection { .. } => GameAction::ChooseTarget {
                target: Some(TargetRef::Object(victim)),
            },
            WaitingFor::ManaPayment { .. } => GameAction::PassPriority,
            _ => break,
        };
    }
    let untapped = runner
        .state()
        .objects
        .values()
        .filter(|o| o.zone == Zone::Battlefield && o.controller == P0 && !o.tapped)
        .count();
    (runner, spell, Some(untapped))
}

#[test]
fn tapped_target_reduces_cost_by_three() {
    let (runner, spell, left) = cast_at(true, 5);
    assert_eq!(runner.state().objects[&spell].zone, Zone::Stack);
    assert_eq!(left, Some(3), "{{4}}{{W}} less {{3}} costs 2 of 5 mana");
}

#[test]
fn tapped_target_is_castable_with_reduced_mana_only() {
    let (runner, spell, _) = cast_at(true, 2);
    assert_eq!(runner.state().objects[&spell].zone, Zone::Stack);
}

#[test]
fn untapped_target_pays_full_cost() {
    let (runner, spell, left) = cast_at(false, 5);
    assert_eq!(runner.state().objects[&spell].zone, Zone::Stack);
    assert_eq!(left, Some(0));
}

#[test]
fn untapped_target_is_not_castable_with_reduced_mana() {
    let (runner, spell, _) = cast_at(false, 2);
    assert_eq!(runner.state().objects[&spell].zone, Zone::Hand);
}

/// Two creatures, one untapped and one tapped, and only the reduced amount of
/// mana: the spell is castable because a tapped target is available.
#[test]
fn castable_at_reduced_cost_when_a_tapped_creature_is_among_targets() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let untapped = scenario.add_creature(P1, "Fresh", 2, 2).id();
    let tapped = scenario.add_creature(P1, "Weary", 2, 2).id();
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Final Verdict", false, MAGNIFICENT_END)
        .as_instant()
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::White],
            generic: 4,
        })
        .id();
    for _ in 0..2 {
        scenario.add_basic_land(P0, ManaColor::White);
    }
    let mut runner = scenario.build();
    runner.state_mut().objects.get_mut(&tapped).unwrap().tapped = true;
    let card_id = runner.state().objects[&spell].card_id;
    assert!(
        engine::game::casting::can_cast_object_now(runner.state(), P0, spell),
        "the UI/AI castability probe must see the reduced cost"
    );
    assert!(engine::ai_support::legal_actions(runner.state())
        .iter()
        .any(|a| matches!(a, GameAction::CastSpell { object_id, .. } if *object_id == spell)));
    runner
        .act(GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("castable: a tapped creature is a legal, discounting target");
    let WaitingFor::TargetSelection { selection, .. } = runner.state().waiting_for.clone() else {
        panic!(
            "expected target selection, got {:?}",
            runner.state().waiting_for
        );
    };
    assert!(selection
        .current_legal_targets
        .contains(&TargetRef::Object(untapped)));
    runner
        .act(GameAction::ChooseTarget {
            target: Some(TargetRef::Object(tapped)),
        })
        .unwrap();
    assert_eq!(runner.state().objects[&spell].zone, Zone::Stack);
}
