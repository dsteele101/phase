//! Fold into Aether — "If that spell is countered this way, its controller may
//! put a creature card from their hand onto the battlefield" offered the
//! countered spell's controller the choice, but accepting it moved nothing:
//! the sub-ability inherited the Counter's object target (the countered
//! spell), so `change_zone::resolve` tried to move that spell out of a hand it
//! was never in and skipped the resolution-time hand scan.
//!
//! Oracle (verbatim, Scryfall):
//!   Counter target spell. If that spell is countered this way, its
//!   controller may put a creature card from their hand onto the battlefield.
//!
//! CR 608.2c: the second sentence is part of the same resolution and names
//! the countered spell's controller. CR 608.2h: that controller is read from
//! the spell's last known information after it left the stack. CR 608.2d: the
//! creature card is a choice that player makes while the effect is applied.
//!
//! Every test drives the real `apply()` pipeline (GameScenario +
//! GameRunner::cast + resolve) and asserts zone deltas.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::card_type::CoreType;
use engine::types::game_state::{CastingVariant, StackEntry, StackEntryKind};
use engine::types::identifiers::{CardId, ObjectId};
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const FOLD_INTO_AETHER: &str = "Counter target spell. If that spell is countered this way, \
                                its controller may put a creature card from their hand onto \
                                the battlefield.";
const RHYTHM_OF_THE_WILD: &str = "Creature spells you control can't be countered.\nNontoken \
                                  creatures you control have riot. (They enter with your \
                                  choice of a +1/+1 counter or haste.)";

struct Board {
    runner: GameRunner,
    fold: ObjectId,
    /// P1's two creature cards in hand — two candidates force a real
    /// `EffectZoneChoice` instead of the single-candidate auto-pick.
    bears: [ObjectId; 2],
    /// A creature card in the caster's hand: never a candidate, because the
    /// choice belongs to the countered spell's controller.
    caster_creature: ObjectId,
}

fn setup(setup: impl FnOnce(&mut GameScenario)) -> Board {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mut cs =
        scenario.add_spell_to_hand_from_oracle(P0, "Fold into Aether", true, FOLD_INTO_AETHER);
    cs.with_mana_cost(ManaCost::Cost {
        generic: 0,
        shards: vec![ManaCostShard::Blue],
    });
    let fold = cs.id();
    scenario.add_basic_land(P0, ManaColor::Blue);
    let bears = [
        scenario
            .add_creature_to_hand(P1, "Grizzly Bears", 2, 2)
            .id(),
        scenario
            .add_creature_to_hand(P1, "Runeclaw Bear", 2, 2)
            .id(),
    ];
    let caster_creature = scenario.add_creature_to_hand(P0, "Hill Giant", 3, 3).id();
    setup(&mut scenario);
    Board {
        runner: scenario.build(),
        fold,
        bears,
        caster_creature,
    }
}

/// Put a spell of type `core` controlled by `controller` on the stack, the
/// shape `counter_rider_tail_8762.rs` uses for its countered spell.
fn put_spell_on_stack(runner: &mut GameRunner, controller: PlayerId, core: CoreType) -> ObjectId {
    let spell = engine::game::zones::create_object(
        runner.state_mut(),
        CardId(701),
        controller,
        "Opposing Spell".to_string(),
        Zone::Stack,
    );
    runner
        .state_mut()
        .objects
        .get_mut(&spell)
        .unwrap()
        .card_types
        .core_types = vec![core];
    runner.state_mut().stack.push_back(StackEntry {
        id: spell,
        source_id: spell,
        controller,
        kind: StackEntryKind::Spell {
            card_id: CardId(701),
            ability: None,
            casting_variant: CastingVariant::Normal,
            actual_mana_spent: 0,
        },
    });
    spell
}

#[test]
fn accepting_lets_the_countered_spells_controller_choose_a_creature_from_hand() {
    let Board {
        mut runner,
        fold,
        bears,
        caster_creature,
    } = setup(|_| {});
    let spell = put_spell_on_stack(&mut runner, P1, CoreType::Instant);

    let outcome = runner
        .cast(fold)
        .target_objects(&[spell])
        .accept_optional()
        .effect_zone(&[bears[1]])
        .resolve();

    assert_eq!(
        outcome.zone_of(spell),
        Zone::Graveyard,
        "reach guard: the target spell must be countered (CR 701.6a)"
    );
    assert_eq!(
        outcome.zone_of(bears[1]),
        Zone::Battlefield,
        "CR 608.2d: the chosen creature card must enter the battlefield"
    );
    assert_eq!(
        outcome.state().objects[&bears[1]].controller,
        P1,
        "the creature enters under the countered spell's controller"
    );
    assert_eq!(
        outcome.zone_of(bears[0]),
        Zone::Hand,
        "only the one chosen creature card is put onto the battlefield"
    );
    assert_eq!(
        outcome.zone_of(caster_creature),
        Zone::Hand,
        "the caster's creature card is not a candidate: the choice is from \
         the countered spell's controller's hand"
    );
}

#[test]
fn declining_leaves_every_creature_card_in_hand() {
    let Board {
        mut runner,
        fold,
        bears,
        caster_creature,
    } = setup(|_| {});
    let spell = put_spell_on_stack(&mut runner, P1, CoreType::Instant);

    let outcome = runner
        .cast(fold)
        .target_objects(&[spell])
        .decline_optional()
        .resolve();

    assert_eq!(
        outcome.zone_of(spell),
        Zone::Graveyard,
        "reach guard: the target spell must still be countered on decline"
    );
    for card in bears.into_iter().chain([caster_creature]) {
        assert_eq!(
            outcome.zone_of(card),
            Zone::Hand,
            "CR 608.2d: declining puts no creature card onto the battlefield"
        );
    }
}

#[test]
fn an_uncounterable_spell_offers_no_creature_put() {
    let Board {
        mut runner,
        fold,
        bears,
        caster_creature,
    } = setup(|scenario| {
        scenario.add_enchantment_from_oracle(P1, "Rhythm of the Wild", RHYTHM_OF_THE_WILD);
    });
    let spell = put_spell_on_stack(&mut runner, P1, CoreType::Creature);

    let outcome = runner
        .cast(fold)
        .target_objects(&[spell])
        .accept_optional()
        .resolve();

    assert_eq!(
        outcome.zone_of(spell),
        Zone::Stack,
        "reach guard: the creature spell can't be countered (CR 101.2)"
    );
    for card in bears.into_iter().chain([caster_creature]) {
        assert_eq!(
            outcome.zone_of(card),
            Zone::Hand,
            "CR 608.2c: the spell was not countered this way, so nothing is put \
             onto the battlefield"
        );
    }
}
