use crate::game::ability_utils::build_resolved_from_def_with_chain_root;
use crate::game::zone_pipeline::{self, ZoneMoveRequest};
use crate::types::ability::{Effect, EffectError, EffectKind, ResolvedAbility};
use crate::types::events::GameEvent;
use crate::types::game_state::GameState;
use crate::types::player::PlayerId;
use crate::types::zones::Zone;

use super::resolve_ability_chain;

/// CR 701.13a + CR 202.3 + CR 608.2c: Resolve a top-card exile contest
/// (Timesifter). Every living player contends; each round, every contender
/// exiles the top card of their library, and the contenders whose exiled card
/// has the greatest mana value stay in. Rounds repeat among the tied contenders
/// until one remains, who receives `winner_effect`.
///
/// A contender who exiles no card (an empty library) drops out: per the card's
/// ruling, a player who fails to exile a card can't win. Every round removes a
/// card from each remaining contender's library or removes the contender, so
/// the loop always ends — with one winner, or with nobody when no contender
/// exiled a card in some round.
pub fn resolve(
    state: &mut GameState,
    ability: &ResolvedAbility,
    events: &mut Vec<GameEvent>,
) -> Result<(), EffectError> {
    let Effect::ExileTopCardContest { winner_effect } = &ability.effect else {
        return Err(EffectError::MissingParam(
            "expected ExileTopCardContest effect".into(),
        ));
    };

    // CR 101.4: "each player" — every living player, in APNAP order.
    let mut contenders = crate::game::players::apnap_order(state);
    let winner = loop {
        let Some(exiled) = exile_round(state, ability, &contenders, events) else {
            // A replacement choice paused the exile; the prompt is parked.
            return Ok(());
        };
        // CR 202.3: compare the mana values of the cards exiled this round.
        let Some(greatest) = exiled.iter().map(|&(_, mana_value)| mana_value).max() else {
            break None;
        };
        // "If two or more players' cards are tied for greatest, the tied players
        // repeat this process" — only the players in the tie continue.
        contenders = exiled
            .into_iter()
            .filter(|&(_, mana_value)| mana_value == greatest)
            .map(|(player, _)| player)
            .collect();
        if let [winner] = contenders[..] {
            break Some(winner);
        }
    };

    if let Some(winner) = winner {
        // CR 608.2c + CR 109.5: the winner performs the winner's instruction, bound
        // as the acting player the way a `player_scope` iteration binds its seat;
        // the printed controller stays recorded so "you" references are stable.
        let mut chain = build_resolved_from_def_with_chain_root(
            winner_effect,
            ability.source_id,
            ability.controller,
            ability.context.chain_root_targets.clone(),
        );
        chain.set_original_controller_recursive(
            ability.original_controller.unwrap_or(ability.controller),
        );
        chain.set_controller_recursive(winner);
        chain.set_scoped_player_recursive(winner);
        resolve_ability_chain(state, &chain, events, 1)?;
    }

    events.push(GameEvent::EffectResolved {
        kind: EffectKind::ExileTopCardContest,
        source_id: ability.source_id,
        subject: None,
    });
    Ok(())
}

/// CR 701.13a: Each of `contenders` exiles the top card of their library.
/// Returns each contender who exiled a card paired with that card's mana value,
/// or `None` when a replacement choice paused the exile.
fn exile_round(
    state: &mut GameState,
    ability: &ResolvedAbility,
    contenders: &[PlayerId],
    events: &mut Vec<GameEvent>,
) -> Option<Vec<(PlayerId, u32)>> {
    let track_exiled_by_source =
        crate::game::exile_links::should_track_exiled_by_source(state, ability.source_id, ability);
    let mut exiled = Vec::with_capacity(contenders.len());
    for &player in contenders {
        let Some(card) = state.library_of(player).front().copied() else {
            continue;
        };
        // CR 614.6: route through the zone-change pipeline so exile replacements
        // apply. The library's owner performs "each player exiles".
        let mut request =
            ZoneMoveRequest::effect(card, Zone::Exile, ability.source_id).performed_by(player);
        if track_exiled_by_source {
            request = request.track_exiled_by_source();
        }
        if let zone_pipeline::ZoneMoveResult::NeedsChoice(chooser) =
            zone_pipeline::move_object(state, request, events)
        {
            // CR 616.1: no Exile-targeting redirect exists in the current card
            // pool; park the ordering prompt rather than continue past it
            // (mirrors `exile_top`).
            state.waiting_for =
                crate::game::replacement::replacement_choice_waiting_for(chooser, state);
            return None;
        }
        // A card a replacement sent anywhere but exile was not "exiled", so it
        // does not count for its player.
        if let Some(obj) = state
            .objects
            .get(&card)
            .filter(|obj| obj.zone == Zone::Exile)
        {
            // CR 202.3d + CR 709.4b: off the stack a split card's mana value is
            // its combined mana cost.
            exiled.push((player, obj.effective_mana_value()));
        }
    }
    Some(exiled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::zones::create_object;
    use crate::types::ability::{AbilityDefinition, AbilityKind, QuantityExpr, TargetFilter};
    use crate::types::game_state::ExtraTurn;
    use crate::types::identifiers::{CardId, ObjectId};
    use crate::types::mana::ManaCost;

    fn contest_ability() -> ResolvedAbility {
        let winner_effect = AbilityDefinition::new(
            AbilityKind::Spell,
            Effect::ExtraTurn {
                target: TargetFilter::Controller,
                count: QuantityExpr::Fixed { value: 1 },
            },
        );
        ResolvedAbility::new(
            Effect::ExileTopCardContest {
                winner_effect: Box::new(winner_effect),
            },
            vec![],
            ObjectId(100),
            PlayerId(0),
        )
    }

    /// Builds `player`'s library from top to bottom with the given mana values.
    fn stack_library(
        state: &mut GameState,
        player: PlayerId,
        mana_values: &[u32],
    ) -> Vec<ObjectId> {
        let ids: Vec<ObjectId> = mana_values
            .iter()
            .enumerate()
            .map(|(i, &mv)| {
                let id = create_object(
                    state,
                    CardId(1000 + u64::from(player.0) * 100 + i as u64),
                    player,
                    format!("P{} card {i}", player.0),
                    Zone::Library,
                );
                state.objects.get_mut(&id).unwrap().mana_cost = ManaCost::generic(mv);
                id
            })
            .collect();
        state.players[player.0 as usize].library = ids.iter().copied().collect();
        ids
    }

    fn zone_of(state: &GameState, id: ObjectId) -> Zone {
        state.objects[&id].zone
    }

    fn extra_turn_players(state: &GameState) -> Vec<PlayerId> {
        state
            .extra_turns
            .iter()
            .map(|ExtraTurn { player, .. }| *player)
            .collect()
    }

    #[test]
    fn greatest_mana_value_wins_and_takes_the_extra_turn() {
        let mut state = GameState::new_two_player(42);
        let p0 = stack_library(&mut state, PlayerId(0), &[2, 7]);
        let p1 = stack_library(&mut state, PlayerId(1), &[5, 1]);
        let mut events = Vec::new();

        resolve(&mut state, &contest_ability(), &mut events).unwrap();

        assert_eq!(extra_turn_players(&state), vec![PlayerId(1)]);
        // One round: only each top card is exiled.
        assert_eq!(zone_of(&state, p0[0]), Zone::Exile);
        assert_eq!(zone_of(&state, p1[0]), Zone::Exile);
        assert_eq!(zone_of(&state, p0[1]), Zone::Library);
        assert_eq!(zone_of(&state, p1[1]), Zone::Library);
    }

    #[test]
    fn tie_repeats_among_the_tied_players_until_broken() {
        let mut state = GameState::new(crate::types::format::FormatConfig::standard(), 3, 42);
        let p0 = stack_library(&mut state, PlayerId(0), &[3, 0, 4]);
        let p1 = stack_library(&mut state, PlayerId(1), &[3, 0, 2]);
        // Player 2 loses round one, so their later (larger) cards never count.
        let p2 = stack_library(&mut state, PlayerId(2), &[1, 9, 9]);
        let mut events = Vec::new();

        resolve(&mut state, &contest_ability(), &mut events).unwrap();

        assert_eq!(extra_turn_players(&state), vec![PlayerId(0)]);
        // Rounds two and three exile only from the tied players.
        assert!(p0.iter().all(|&id| zone_of(&state, id) == Zone::Exile));
        assert!(p1.iter().all(|&id| zone_of(&state, id) == Zone::Exile));
        assert_eq!(zone_of(&state, p2[0]), Zone::Exile);
        assert_eq!(zone_of(&state, p2[1]), Zone::Library);
    }

    #[test]
    fn a_player_who_exiles_no_card_cannot_win() {
        let mut state = GameState::new_two_player(42);
        stack_library(&mut state, PlayerId(0), &[]);
        stack_library(&mut state, PlayerId(1), &[0]);
        let mut events = Vec::new();

        resolve(&mut state, &contest_ability(), &mut events).unwrap();

        assert_eq!(extra_turn_players(&state), vec![PlayerId(1)]);
    }

    #[test]
    fn a_tie_that_runs_out_of_cards_has_no_winner() {
        let mut state = GameState::new_two_player(42);
        stack_library(&mut state, PlayerId(0), &[4]);
        stack_library(&mut state, PlayerId(1), &[4]);
        let mut events = Vec::new();

        resolve(&mut state, &contest_ability(), &mut events).unwrap();

        assert!(state.extra_turns.is_empty());
        assert!(events.iter().any(|event| matches!(
            event,
            GameEvent::EffectResolved {
                kind: EffectKind::ExileTopCardContest,
                ..
            }
        )));
    }
}
