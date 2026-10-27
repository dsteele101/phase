//! CR 608.2c + CR 701.27a: Civilized Scholar — "{T}: Draw a card, then discard
//! a card. When you discard a creature card this way, untap Civilized Scholar
//! and transform it."
//!
//! The reported bug: discarding a creature card untapped the Scholar but never
//! transformed it, because the bare "it" in "transform it" bound to the
//! reflexive gate's parent target (the discarded card) instead of the Scholar
//! that "untap ~" had just named.

use super::rules::{GameScenario, Phase, P0};
use engine::game::game_object::BackFaceData;
use engine::game::scenario::GameRunner;
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{AbilityDefinition, Effect, TargetFilter};
use engine::types::actions::GameAction;
use engine::types::card_type::{CardType, CoreType};
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaCost;
use engine::types::zones::Zone;

/// Verbatim Oracle text (Scryfall) of the front face's activated ability.
const ORACLE: &str = "{T}: Draw a card, then discard a card. When you discard a creature card this way, untap Civilized Scholar and transform it.";

fn homicidal_brute_back_face() -> BackFaceData {
    BackFaceData {
        is_swap_snapshot: false,
        trigger_printed_origins: Vec::new(),
        name: "Homicidal Brute".to_string(),
        power: Some(5),
        toughness: Some(1),
        loyalty: None,
        printed_loyalty: None,
        defense: None,
        card_types: CardType {
            supertypes: vec![],
            core_types: vec![CoreType::Creature],
            subtypes: vec!["Human".to_string(), "Warrior".to_string()],
        },
        mana_cost: ManaCost::default(),
        keywords: vec![],
        abilities: vec![],
        trigger_definitions: Default::default(),
        replacement_definitions: Default::default(),
        static_definitions: Default::default(),
        color: vec![],
        printed_ref: None,
        modal: None,
        additional_cost: None,
        strive_cost: None,
        casting_restrictions: vec![],
        casting_options: vec![],
        layout_kind: None,
        parse_warnings: vec![],
    }
}

fn parsed_ability() -> AbilityDefinition {
    let parsed = parse_oracle_text(
        ORACLE,
        "Civilized Scholar",
        &[],
        &["Creature".to_string()],
        &["Human".to_string(), "Advisor".to_string()],
    );
    parsed.abilities[0].clone()
}

/// CR 608.2c: "transform it" names the Scholar, which "untap ~" just named.
#[test]
fn transform_it_binds_to_the_scholar_not_the_parent_target() {
    let ability = parsed_ability();
    let untap = ability
        .sub_ability
        .as_deref()
        .and_then(|discard| discard.sub_ability.as_deref())
        .expect("reflexive untap clause");
    let transform = untap.sub_ability.as_deref().expect("transform clause");
    assert!(
        matches!(
            &*transform.effect,
            Effect::Transform {
                target: TargetFilter::SelfRef,
                ..
            }
        ),
        "expected Transform {{ SelfRef }}, got {:?}",
        transform.effect
    );
}

/// Draws `drawn`, discards `discarded` (a hand holding both, so the discard
/// prompt is reached), and returns the Scholar's id with the finished runner.
fn activate_and_discard(discard_creature: bool) -> (GameRunner, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let scholar = {
        let mut b = scenario.add_creature(P0, "Civilized Scholar", 0, 1);
        b.with_subtypes(vec!["Human", "Advisor"]);
        b.from_oracle_text(ORACLE);
        b.id()
    };
    let creature_card = scenario
        .add_creature_to_hand(P0, "Hand Creature", 2, 2)
        .id();
    let land_card = scenario.add_card_to_hand(P0, "Hand Filler");
    scenario.add_card_to_library_top(P0, "Library Filler");

    let mut runner = scenario.build();
    {
        let obj = runner.state_mut().objects.get_mut(&scholar).unwrap();
        obj.back_face = Some(homicidal_brute_back_face());
    }
    runner
        .act(GameAction::ActivateAbility {
            source_id: scholar,
            ability_index: 0,
        })
        .expect("activate Civilized Scholar");
    runner.advance_until_stack_empty();

    assert!(
        matches!(runner.state().waiting_for, WaitingFor::DiscardChoice { .. }),
        "expected the discard prompt, got {:?}",
        runner.state().waiting_for
    );
    assert!(runner.state().objects[&scholar].tapped, "{{T}} was paid");
    let pick = if discard_creature {
        creature_card
    } else {
        land_card
    };
    runner
        .act(GameAction::SelectCards { cards: vec![pick] })
        .expect("select discard");
    runner.advance_until_stack_empty();
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::Priority { .. }),
        "got {:?}",
        runner.state().waiting_for
    );
    assert!(runner.state().stack.is_empty());
    assert_eq!(runner.state().objects[&pick].zone, Zone::Graveyard);
    (runner, scholar)
}

/// CR 701.27a: discarding a creature card untaps and transforms the Scholar.
#[test]
fn discarding_a_creature_card_untaps_and_transforms() {
    let (runner, scholar) = activate_and_discard(true);
    let obj = &runner.state().objects[&scholar];
    assert_eq!(obj.zone, Zone::Battlefield);
    assert!(!obj.tapped, "the Scholar untaps");
    assert!(obj.transformed, "the Scholar transforms");
    assert_eq!(obj.name, "Homicidal Brute");
}

/// Negative with a reach guard (the discard happened and resolved): a
/// noncreature discard leaves the Scholar tapped and front-face up.
#[test]
fn discarding_a_noncreature_card_does_nothing() {
    let (runner, scholar) = activate_and_discard(false);
    let obj = &runner.state().objects[&scholar];
    assert!(obj.tapped);
    assert!(!obj.transformed);
    assert_eq!(obj.name, "Civilized Scholar");
}
