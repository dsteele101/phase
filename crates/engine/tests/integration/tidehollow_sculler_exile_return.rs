//! CR 607.1 + CR 607.2a + CR 406.6 + CR 603.3: Tidehollow Sculler exile and return.
//!
//! "When this creature enters, target opponent reveals their hand and you choose a
//! nonland card from it. Exile that card.
//! When this creature leaves the battlefield, return the exiled card to its
//! owner's hand."
//!
//! Two linked triggered abilities (CR 607.2a), not a CR 610.3 "until" duration: the
//! return is its own triggered ability that goes on the stack when Sculler leaves
//! the battlefield, so it can be responded to and the card stays exiled until it
//! resolves.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::events::GameEvent;
use engine::types::game_state::{ExileLinkKind, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const TIDEHOLLOW_SCULLER: &str = "When this creature enters, target opponent reveals their hand and you choose a nonland card from it. Exile that card.\n\
When this creature leaves the battlefield, return the exiled card to its owner's hand.";

const MESMERIC_FIEND: &str = "When this creature enters, target opponent reveals their hand and you choose a nonland card from it. Exile that card.\n\
When this creature leaves the battlefield, return the exiled card to its owner's hand.";

const MURDER: &str = "Destroy target creature.";

fn zone_of(runner: &GameRunner, id: ObjectId) -> Zone {
    runner.state().objects.get(&id).unwrap().zone
}

#[test]
fn tidehollow_sculler_returns_exiled_card_to_hand_when_it_leaves_play() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let sculler = scenario
        .add_creature_to_hand_from_oracle(P0, "Tidehollow Sculler", 2, 2, TIDEHOLLOW_SCULLER)
        .id();

    let target_card = scenario.add_card_to_hand(P1, "Doom Blade");
    let land_card = scenario.add_land_to_hand(P1, "Swamp").id();
    let destroy = scenario
        .add_spell_to_hand_from_oracle(P0, "Murder", true, MURDER)
        .id();

    let mut runner = scenario.build();

    let outcome = runner.cast(sculler).target_player(P1).resolve();

    let WaitingFor::RevealChoice { cards, .. } = outcome.final_waiting_for() else {
        panic!(
            "expected RevealChoice prompt after ETB resolves, got {:?}",
            outcome.final_waiting_for()
        );
    };
    assert!(cards.contains(&target_card));
    assert!(!cards.contains(&land_card));

    runner
        .act(GameAction::SelectCards {
            cards: vec![target_card],
        })
        .expect("SelectCards succeeds");

    assert_eq!(zone_of(&runner, target_card), Zone::Exile);
    assert_eq!(zone_of(&runner, land_card), Zone::Hand);
    assert!(runner.state().exile_links.iter().any(|link| {
        link.exiled_id == target_card
            && link.source_id == sculler
            && link.kind == ExileLinkKind::TrackedBySource
    }));

    // Destroy Tidehollow Sculler so it leaves the battlefield.
    runner.cast(destroy).target_object(sculler).resolve();
    runner.advance_until_stack_empty();

    assert_eq!(zone_of(&runner, sculler), Zone::Graveyard);
    assert_eq!(
        zone_of(&runner, target_card),
        Zone::Hand,
        "Target card must return to hand when Tidehollow Sculler leaves the battlefield"
    );
    assert!(runner.state().players[P1.0 as usize]
        .hand
        .contains(&target_card));
    assert!(!runner.state().exile.contains(&target_card));
    assert!(
        runner.state().exile_links.is_empty(),
        "ExileLink must be consumed"
    );
}

#[test]
fn mesmeric_fiend_returns_exiled_card_to_hand_when_it_leaves_play() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let fiend = scenario
        .add_creature_to_hand_from_oracle(P0, "Mesmeric Fiend", 1, 1, MESMERIC_FIEND)
        .id();

    let target_card = scenario.add_card_to_hand(P1, "Counterspell");
    let destroy = scenario
        .add_spell_to_hand_from_oracle(P0, "Murder", true, MURDER)
        .id();

    let mut runner = scenario.build();

    let outcome = runner.cast(fiend).target_player(P1).resolve();

    let WaitingFor::RevealChoice { .. } = outcome.final_waiting_for() else {
        panic!(
            "expected RevealChoice prompt after ETB resolves, got {:?}",
            outcome.final_waiting_for()
        );
    };

    runner
        .act(GameAction::SelectCards {
            cards: vec![target_card],
        })
        .expect("SelectCards succeeds");

    assert_eq!(zone_of(&runner, target_card), Zone::Exile);

    runner.cast(destroy).target_object(fiend).resolve();
    runner.advance_until_stack_empty();

    assert_eq!(zone_of(&runner, fiend), Zone::Graveyard);
    assert_eq!(
        zone_of(&runner, target_card),
        Zone::Hand,
        "Target card must return to hand when Mesmeric Fiend leaves the battlefield"
    );
    assert!(runner.state().players[P1.0 as usize]
        .hand
        .contains(&target_card));
    assert!(!runner.state().exile.contains(&target_card));
    assert!(
        runner.state().exile_links.is_empty(),
        "ExileLink must be consumed"
    );
}

#[test]
fn tidehollow_sculler_with_no_nonland_cards_in_opponents_hand() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let sculler = scenario
        .add_creature_to_hand_from_oracle(P0, "Tidehollow Sculler", 2, 2, TIDEHOLLOW_SCULLER)
        .id();

    let land_card = scenario.add_land_to_hand(P1, "Plains").id();
    let destroy = scenario
        .add_spell_to_hand_from_oracle(P0, "Murder", true, MURDER)
        .id();

    let mut runner = scenario.build();

    let outcome = runner.cast(sculler).target_player(P1).resolve();

    // The ETB resolved (the reveal ran and no card was offered) rather than
    // never executing: Sculler is on the battlefield and the stack is empty.
    assert!(
        !matches!(outcome.final_waiting_for(), WaitingFor::RevealChoice { .. }),
        "no nonland card to choose, so no choice may be offered"
    );
    assert!(
        outcome.events().iter().any(|e| matches!(
            e,
            GameEvent::EffectResolved { source_id, .. }
                if *source_id == sculler
        )),
        "Sculler's ETB must have resolved"
    );
    assert_eq!(zone_of(&runner, land_card), Zone::Hand);
    assert!(runner.state().exile_links.is_empty());

    runner.cast(destroy).target_object(sculler).resolve();
    runner.advance_until_stack_empty();

    assert_eq!(zone_of(&runner, sculler), Zone::Graveyard);
    assert_eq!(zone_of(&runner, land_card), Zone::Hand);
    assert!(runner.state().exile_links.is_empty());
}

const ASHIOKS_ERASURE: &str = "Flash\n\
When this enchantment enters, exile target spell.\n\
Your opponents can't cast spells with the same name as the exiled card.\n\
When this enchantment leaves the battlefield, return the exiled card to its owner's hand.";

/// Pass priority until `done` holds, failing loudly instead of looping forever.
fn pass_until(runner: &mut GameRunner, what: &str, done: impl Fn(&GameRunner) -> bool) {
    for _ in 0..12 {
        if done(runner) {
            return;
        }
        runner
            .act(GameAction::PassPriority)
            .unwrap_or_else(|e| panic!("priority pass failed while waiting for {what}: {e:?}"));
    }
    assert!(done(runner), "never reached: {what}");
}

/// CR 603.3 + CR 607.2a: the printed leaves-the-battlefield ability is a real
/// triggered ability. While it waits on the stack (where it can be responded to
/// or countered), the card stays exiled; it returns only when the ability resolves.
#[test]
fn tidehollow_sculler_card_stays_exiled_until_leaves_trigger_resolves() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let sculler = scenario
        .add_creature_to_hand_from_oracle(P0, "Tidehollow Sculler", 2, 2, TIDEHOLLOW_SCULLER)
        .id();
    let target_card = scenario.add_card_to_hand(P1, "Doom Blade");
    let destroy = scenario
        .add_spell_to_hand_from_oracle(P0, "Murder", true, MURDER)
        .id();
    let mut runner = scenario.build();

    runner.cast(sculler).target_player(P1).resolve();
    runner
        .act(GameAction::SelectCards {
            cards: vec![target_card],
        })
        .expect("SelectCards succeeds");
    assert_eq!(zone_of(&runner, target_card), Zone::Exile);

    drop(runner.cast(destroy).target_object(sculler).commit());
    pass_until(
        &mut runner,
        "Sculler's leaves-the-battlefield trigger on the stack",
        |r| zone_of(r, sculler) == Zone::Graveyard,
    );

    assert_eq!(
        runner.state().stack.len(),
        1,
        "the leaves-the-battlefield trigger waits on the stack"
    );
    assert_eq!(
        zone_of(&runner, target_card),
        Zone::Exile,
        "the card must stay exiled while the LTB trigger is on the stack"
    );

    runner.resolve_top();
    assert_eq!(zone_of(&runner, target_card), Zone::Hand);
    assert!(runner.state().players[P1.0 as usize]
        .hand
        .contains(&target_card));
}

/// Official ruling (2021-03-19): "If Tidehollow Sculler leaves the battlefield
/// before its first ability has resolved, its second ability triggers. This
/// ability does nothing when it resolves. Then its first ability resolves and
/// exiles the chosen card indefinitely." (CR 607.2a: the return only refers to
/// cards already exiled; it is not a CR 610.3 duration, so CR 610.3b does not
/// stop the exile.)
#[test]
fn tidehollow_sculler_leaving_before_etb_resolves_exiles_indefinitely() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let sculler = scenario
        .add_creature_to_hand_from_oracle(P0, "Tidehollow Sculler", 2, 2, TIDEHOLLOW_SCULLER)
        .id();
    let target_card = scenario.add_card_to_hand(P1, "Doom Blade");
    let destroy = scenario
        .add_spell_to_hand_from_oracle(P0, "Murder", true, MURDER)
        .id();
    let mut runner = scenario.build();

    drop(runner.cast(sculler).target_player(P1).commit());
    pass_until(&mut runner, "Sculler's ETB trigger on the stack", |r| {
        zone_of(r, sculler) == Zone::Battlefield && !r.state().stack.is_empty()
    });

    // Respond to the ETB trigger by destroying Sculler.
    drop(runner.cast(destroy).target_object(sculler).commit());
    pass_until(&mut runner, "the exile choice", |r| {
        matches!(r.state().waiting_for, WaitingFor::RevealChoice { .. })
    });
    assert_eq!(zone_of(&runner, sculler), Zone::Graveyard);

    runner
        .act(GameAction::SelectCards {
            cards: vec![target_card],
        })
        .expect("SelectCards succeeds");
    runner.advance_until_stack_empty();

    assert_eq!(
        zone_of(&runner, target_card),
        Zone::Exile,
        "the chosen card is exiled indefinitely; the earlier leaves trigger did nothing"
    );
}

/// CR 607.2a + CR 603.3: Ashiok's Erasure exiles a SPELL (from the stack) and its
/// printed leaves-the-battlefield ability returns that card to its owner's hand —
/// not to the zone it was exiled from.
#[test]
fn ashioks_erasure_returns_exiled_spell_to_owners_hand() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let erasure = {
        let mut builder =
            scenario.add_spell_to_hand_from_oracle(P0, "Ashiok's Erasure", false, ASHIOKS_ERASURE);
        builder.as_enchantment();
        builder.id()
    };
    let victim = scenario
        .add_spell_to_hand_from_oracle(P0, "Victim Spell", false, "You gain 3 life.")
        .id();
    let destroy = scenario
        .add_spell_to_hand_from_oracle(P0, "Disenchant", true, "Destroy target enchantment.")
        .id();
    let mut runner = scenario.build();

    let mut victim_cast = runner.cast(victim).commit();
    victim_cast.cast(erasure).target_object(victim).resolve();
    drop(victim_cast);
    assert_eq!(zone_of(&runner, erasure), Zone::Battlefield);
    assert_eq!(zone_of(&runner, victim), Zone::Exile);

    runner.cast(destroy).target_object(erasure).resolve();
    runner.advance_until_stack_empty();

    assert_eq!(zone_of(&runner, erasure), Zone::Graveyard);
    assert_eq!(
        zone_of(&runner, victim),
        Zone::Hand,
        "the exiled spell returns to its owner's hand, not to the stack"
    );
}
