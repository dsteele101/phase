use crate::types::ability::{Effect, EffectError, EffectKind, ResolvedAbility};
use crate::types::events::GameEvent;
use crate::types::game_state::{CastOfferKind, GameState, WaitingFor};
use crate::types::identifiers::ObjectId;
use crate::types::player::PlayerId;
use crate::types::resolved_commands::{
    ResolvedInformationAudience, ResolvedInformationEdit, ResolvedInformationLifetime,
};
use crate::types::zones::Zone;

/// CR 702.60a: Ripple N — "When you cast this spell, you may reveal the top N
/// cards of your library, or, if there are fewer than N cards in your library,
/// you may reveal all the cards in your library. If you reveal cards from your
/// library this way, you may cast any of those cards with the same name as this
/// spell without paying their mana costs, then put all revealed cards not cast
/// this way on the bottom of your library in any order."
///
/// CR 701.20b: revealing does NOT move the revealed cards — they stay on top of
/// the library while the free-cast offer is open. The matching card is cast
/// *from the library* during resolution via the shared
/// `initiate_cast_during_resolution` authority (its
/// `ExileWithAltCost { resolution_cleanup: Some(_) }` grant is zone-agnostic —
/// see `castable_from_current_zone`), and the non-cast revealed cards are put on
/// the bottom by the resolution-choice handler after all same-named cards the
/// player chooses to cast from this reveal have been offered.
///
/// CR 702.60a "you may reveal": the engine always reveals on resolution — a
/// no-strategic-value decline of the whole reveal is not modeled (no other
/// engine keyword models that sub-choice); the "may" is honored by the free
/// cast itself being optional.
pub fn resolve(
    state: &mut GameState,
    ability: &ResolvedAbility,
    events: &mut Vec<GameEvent>,
) -> Result<(), EffectError> {
    let Effect::Ripple { count } = ability.effect else {
        return Err(EffectError::InvalidParam("Expected Ripple".to_string()));
    };

    // CR 603.3a: Re-read the controller from the source spell at resolution time
    // (a control-change between trigger creation and resolution is honored); fall
    // back to the trigger snapshot if the spell has left the stack.
    let controller = state
        .objects
        .get(&ability.source_id)
        .map(|obj| obj.controller)
        .unwrap_or(ability.controller);

    if !state.players.iter().any(|p| p.id == controller) {
        return Err(EffectError::PlayerNotFound);
    }

    // CR 702.60a: same name as *this* spell. Read the source spell's name.
    let source_name = state
        .objects
        .get(&ability.source_id)
        .map(|obj| obj.name.clone())
        .unwrap_or_default();

    // CR 702.60a + CR 701.20b: reveal the top N cards of the library (or all of
    // them, if fewer than N) WITHOUT moving them. Top-first order is preserved
    // for the free-cast offer and the "in any order" bottom placement.
    let revealed: Vec<ObjectId> = state
        .players
        .iter()
        .find(|p| p.id == controller)
        .map(|p| p.library.iter().take(count as usize).copied().collect())
        .unwrap_or_default();

    if revealed.is_empty() {
        // CR 702.60a: an empty library reveals nothing; resolve cleanly.
        events.push(GameEvent::EffectResolved {
            kind: EffectKind::from(&ability.effect),
            source_id: ability.source_id,
            subject: None,
        });
        return Ok(());
    }

    publish_ripple_reveal(state, controller, &revealed, events);

    events.push(GameEvent::EffectResolved {
        kind: EffectKind::from(&ability.effect),
        source_id: ability.source_id,
        subject: None,
    });

    // `partition` preserves top-first order within each bucket.
    let (mut hits, revealed_misses): (Vec<_>, Vec<_>) = revealed.into_iter().partition(|id| {
        !source_name.is_empty() && state.objects.get(id).is_some_and(|o| o.name == source_name)
    });

    match hits.is_empty() {
        false => {
            let hit_card = hits.remove(0);
            // CR 702.60a: offer the free cast. The accept/decline + bottoming of
            // the rest is handled in `engine_resolution_choices`.
            state.waiting_for = WaitingFor::CastOffer {
                player: controller,
                kind: CastOfferKind::Ripple {
                    hit_card,
                    remaining_hits: hits,
                    revealed_misses,
                    source_id: ability.source_id,
                },
            };
        }
        true => {
            // CR 702.60a: no same-named card revealed — put them all on the
            // bottom of the library "in any order". The engine takes the
            // deterministic top-first reveal order (mirroring
            // `DigRestOrder::Preserve` for the same clause on `Effect::Dig`),
            // routed through the shared replacement-aware rest-partition
            // primitive rather than Cascade's `shuffle_to_bottom`.
            let _ = crate::game::engine_resolution_choices::route_rest_partition_then(
                state,
                &revealed_misses,
                Zone::Library,
                Some(ability.source_id),
                None,
                events,
            );
        }
    }

    Ok(())
}

/// CR 701.20a/b: Publish a Ripple reveal. The cards stay in the library, so
/// visibility rides entirely on the resolved-information sets:
///
/// * `Controller` / `UntilActionBoundary` feeds `state.revealed_cards`, which
///   `is_visible_revealed_card` honors for every viewer in every zone (the
///   library included). `apply_action` keeps this set alive across the
///   `CastOffer { kind: Ripple }` boundary (see `engine.rs`).
/// * `Public` / `UntilZoneChange` is the durable CR 701.20a public fact,
///   auto-cleared per card when it changes zones (cast to the stack, or
///   bottomed within the library).
///
/// One `CardsRevealed` event carries the whole simultaneously-revealed pile
/// (CR 701.20a); it also lights up the game log and the client reveal
/// animation. `last_revealed_ids` is set for `LastRevealed` consumers.
fn publish_ripple_reveal(
    state: &mut GameState,
    controller: PlayerId,
    revealed: &[ObjectId],
    events: &mut Vec<GameEvent>,
) {
    if revealed.is_empty() {
        return;
    }
    state
        .resolve_and_apply_information(
            revealed,
            ResolvedInformationAudience::Controller(controller),
            ResolvedInformationLifetime::UntilActionBoundary,
            ResolvedInformationEdit::Reveal,
        )
        .expect("resolved ripple reveal occurrences must be live and distinct");
    state
        .resolve_and_apply_information(
            revealed,
            ResolvedInformationAudience::Public,
            ResolvedInformationLifetime::UntilZoneChange,
            ResolvedInformationEdit::Reveal,
        )
        .expect("published ripple reveal occurrences must be live and distinct");

    let card_names: Vec<String> = revealed
        .iter()
        .filter_map(|id| state.objects.get(id).map(|o| o.name.clone()))
        .collect();
    events.push(GameEvent::CardsRevealed {
        player: controller,
        card_ids: revealed.to_vec(),
        card_names,
    });
    state.last_revealed_ids = revealed.to_vec();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::zones::create_object;
    use crate::types::identifiers::CardId;
    use crate::types::player::PlayerId;

    fn setup(name: &str) -> (GameState, ObjectId) {
        let mut state = GameState::new_two_player(42);
        let source_id = create_object(
            &mut state,
            CardId(1000),
            PlayerId(0),
            name.to_string(),
            Zone::Stack,
        );
        (state, source_id)
    }

    fn add_library_card(state: &mut GameState, name: &str) -> ObjectId {
        let card_id = CardId(state.next_object_id);
        create_object(state, card_id, PlayerId(0), name.to_string(), Zone::Library)
    }

    /// CR 702.60a: a same-named card in the top N is offered for a free cast.
    /// CR 701.20b: the revealed cards stay in the library and are published to
    /// every viewer for the duration of the offer.
    #[test]
    fn offers_same_named_revealed_card() {
        let (mut state, source_id) = setup("Surging Flame");
        let other = add_library_card(&mut state, "Mountain");
        let match_card = add_library_card(&mut state, "Surging Flame");
        state.players[0].library = im::vector![other, match_card];

        let ability =
            ResolvedAbility::new(Effect::Ripple { count: 2 }, vec![], source_id, PlayerId(0));
        let mut events = Vec::new();
        resolve(&mut state, &ability, &mut events).unwrap();

        match &state.waiting_for {
            WaitingFor::CastOffer {
                kind:
                    CastOfferKind::Ripple {
                        hit_card,
                        remaining_hits,
                        revealed_misses,
                        ..
                    },
                ..
            } => {
                assert_eq!(*hit_card, match_card);
                assert!(remaining_hits.is_empty());
                assert_eq!(revealed_misses, &vec![other]);
            }
            other => panic!("expected Ripple CastOffer, got {other:?}"),
        }

        // CR 701.20b: no card moved — both are still in the library.
        for id in [other, match_card] {
            assert_eq!(state.objects.get(&id).map(|o| o.zone), Some(Zone::Library));
        }
        // CR 701.20a: both are publicly revealed while the offer is open.
        assert!(state.revealed_cards.contains(&other));
        assert!(state.revealed_cards.contains(&match_card));
        // CR 701.20a: one event carries the whole revealed pile, top-first.
        let revealed_event = events
            .iter()
            .find_map(|e| match e {
                GameEvent::CardsRevealed {
                    card_ids, player, ..
                } => Some((player, card_ids)),
                _ => None,
            })
            .expect("Ripple emits a CardsRevealed event");
        assert_eq!(*revealed_event.0, PlayerId(0));
        assert_eq!(revealed_event.1, &vec![other, match_card]);
    }

    /// CR 702.60a: all same-named cards revealed by one ripple remain eligible.
    #[test]
    fn offers_all_same_named_revealed_cards_before_misses() {
        let (mut state, source_id) = setup("Surging Flame");
        let first_match = add_library_card(&mut state, "Surging Flame");
        let miss = add_library_card(&mut state, "Mountain");
        let second_match = add_library_card(&mut state, "Surging Flame");
        state.players[0].library = im::vector![first_match, miss, second_match];

        let ability =
            ResolvedAbility::new(Effect::Ripple { count: 3 }, vec![], source_id, PlayerId(0));
        resolve(&mut state, &ability, &mut Vec::new()).unwrap();

        match &state.waiting_for {
            WaitingFor::CastOffer {
                kind:
                    CastOfferKind::Ripple {
                        hit_card,
                        remaining_hits,
                        revealed_misses,
                        ..
                    },
                ..
            } => {
                assert_eq!(*hit_card, first_match);
                assert_eq!(remaining_hits, &vec![second_match]);
                assert_eq!(revealed_misses, &vec![miss]);
            }
            other => panic!("expected Ripple CastOffer, got {other:?}"),
        }
    }

    /// CR 702.60a: no same-named card revealed — all go to the bottom, no offer.
    /// The reveal is still published (CR 701.20a) even though nothing is cast.
    #[test]
    fn no_match_bottoms_revealed_cards() {
        let (mut state, source_id) = setup("Surging Might");
        let a = add_library_card(&mut state, "Forest");
        let b = add_library_card(&mut state, "Bear");
        state.players[0].library = im::vector![a, b];

        let ability =
            ResolvedAbility::new(Effect::Ripple { count: 2 }, vec![], source_id, PlayerId(0));
        let mut events = Vec::new();
        resolve(&mut state, &ability, &mut events).unwrap();

        assert!(
            !matches!(
                state.waiting_for,
                WaitingFor::CastOffer {
                    kind: CastOfferKind::Ripple { .. },
                    ..
                }
            ),
            "no same-named card should produce no offer"
        );
        // Both revealed cards returned to the library (bottom).
        assert_eq!(state.players[0].library.len(), 2);
        for id in [a, b] {
            assert_eq!(state.objects.get(&id).map(|o| o.zone), Some(Zone::Library));
        }
        // CR 701.20a: the reveal fires even with no hit — drives the log + client.
        assert!(events.iter().any(|e| matches!(
            e,
            GameEvent::CardsRevealed { card_ids, .. } if card_ids == &vec![a, b]
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            GameEvent::EffectResolved {
                kind: EffectKind::Ripple,
                ..
            }
        )));
    }

    /// CR 702.60a: empty library reveals nothing and offers nothing.
    #[test]
    fn empty_library_no_offer() {
        let (mut state, source_id) = setup("Surging Aether");
        state.players[0].library.clear();

        let ability =
            ResolvedAbility::new(Effect::Ripple { count: 1 }, vec![], source_id, PlayerId(0));
        resolve(&mut state, &ability, &mut Vec::new()).unwrap();

        assert!(!matches!(
            state.waiting_for,
            WaitingFor::CastOffer {
                kind: CastOfferKind::Ripple { .. },
                ..
            }
        ));
    }
}
