//! Telling Time-class remainder split: "look at the top N, put some into your
//! hand, and split what's left between the TOP and the BOTTOM of your library."
//!
//! CARD TEXT (Telling Time, {1}{U} instant, Scryfall-verified):
//! "Look at the top three cards of your library. Put one of those cards into
//! your hand, one on top of your library, and one on the bottom of your
//! library."
//!
//! Before this change the trailing "one on top ... and one on the bottom"
//! clause was SILENTLY SWALLOWED — the card parsed to
//! `Dig { destination: Hand, keep_count: 1, rest_destination: None }`, and a
//! `rest_destination` of `None` defaults to the GRAVEYARD. Telling Time milled
//! two cards instead of returning them to the library, with no
//! `Effect::Unimplemented` and no red coverage to show for it.
//!
//! The capability is composed of shipped building blocks:
//!   * `Effect::Dig.rest_split_top_count` (CR 401.2 — a library is one
//!     face-down pile, so top and bottom are the only two positions an
//!     instruction can name) carries how many of the remainder go on top.
//!   * `WaitingFor::DigRestSplitChoice` (CR 701.20e — the remainder was shown
//!     only to the looking player) is the follow-up prompt.
//!   * `route_rest_split_then` reuses the same per-card
//!     `ZoneMoveRequest::effect(..).at_library_position(..)` primitive as the
//!     uniform `route_rest_partition_then`; only the position varies.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::game::zones::create_object;
use engine::types::ability::{Effect, QuantityExpr};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::{CardId, ObjectId};
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

/// Verbatim Oracle text — a paraphrase can take a different parser branch and
/// go green while the real card stays broken.
const TELLING_TIME_ORACLE: &str = "Look at the top three cards of your library. \
Put one of those cards into your hand, one on top of your library, and one on \
the bottom of your library.";

fn telling_time_cost() -> ManaCost {
    ManaCost::Cost {
        shards: vec![ManaCostShard::Blue],
        generic: 1,
    }
}

fn add_mana(runner: &mut GameRunner, ty: ManaType, count: usize) {
    for _ in 0..count {
        let unit = ManaUnit::new(ty, ObjectId(0), false, vec![]);
        runner.state_mut().players[0].mana_pool.add(unit);
    }
}

/// Put a plain non-land card into P0's library (pushed on top of the existing
/// library contents, so the last one added is deepest-added-last).
fn add_library_card(runner: &mut GameRunner, name: &str) -> ObjectId {
    let card_id = CardId(runner.state().next_object_id);
    let id = create_object(
        runner.state_mut(),
        card_id,
        P0,
        name.to_string(),
        Zone::Library,
    );
    let obj = runner.state_mut().objects.get_mut(&id).unwrap();
    obj.card_types.core_types.push(CoreType::Creature);
    id
}

// ---------------------------------------------------------------------------
// Test 1 (PARSER): the split clause reaches the AST instead of being swallowed.
// ---------------------------------------------------------------------------

#[test]
fn telling_time_parses_a_library_top_bottom_rest_split() {
    let parsed = engine::parser::oracle::parse_oracle_text(
        TELLING_TIME_ORACLE,
        "Telling Time",
        &[],
        &["Instant".to_string()],
        &[],
    );
    let effect = parsed
        .abilities
        .first()
        .map(|a| a.effect.as_ref())
        .expect("Telling Time must parse to one spell ability");
    match effect {
        Effect::Dig {
            count,
            destination,
            keep_count,
            rest_destination,
            rest_split_top_count,
            ..
        } => {
            assert_eq!(
                *count,
                QuantityExpr::Fixed { value: 3 },
                "look at top three"
            );
            assert_eq!(*destination, Some(Zone::Hand), "one goes into your hand");
            assert_eq!(*keep_count, Some(1), "exactly one is kept");
            // CR 401.2: the remainder goes back into the LIBRARY, not the
            // graveyard that `None` would have defaulted to.
            assert_eq!(
                *rest_destination,
                Some(Zone::Library),
                "the split remainder is library-bound"
            );
            // THE REGRESSION ASSERTION: this is `None` if the "one on top ...
            // and one on the bottom" clause is swallowed again.
            assert_eq!(
                *rest_split_top_count,
                Some(QuantityExpr::Fixed { value: 1 }),
                "exactly one of the remainder goes on TOP"
            );
        }
        other => panic!("expected Effect::Dig, got {other:?}"),
    }
}

/// PAIRED NEGATIVE: the sibling uniform-remainder grammar is untouched. A dig
/// that says "and the rest on the bottom of your library" names ONE position
/// for the whole remainder, so it must keep `rest_split_top_count: None` and
/// its existing uniform routing.
#[test]
fn plain_rest_on_bottom_dig_does_not_parse_as_a_split() {
    let parsed = engine::parser::oracle::parse_oracle_text(
        "Look at the top three cards of your library. Put one of them into your \
         hand and the rest on the bottom of your library in any order.",
        "Uniform Remainder Dig",
        &[],
        &["Instant".to_string()],
        &[],
    );
    let effect = parsed
        .abilities
        .first()
        .map(|a| a.effect.as_ref())
        .expect("the sibling dig must parse to one spell ability");
    match effect {
        Effect::Dig {
            rest_destination,
            rest_split_top_count,
            ..
        } => {
            assert_eq!(
                *rest_destination,
                Some(Zone::Library),
                "the uniform remainder still goes to the library"
            );
            assert_eq!(
                *rest_split_top_count, None,
                "a single named position is not a split"
            );
        }
        other => panic!("expected Effect::Dig, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Test 2 (PRODUCTION PATH): cast the real card, submit both real GameActions.
// ---------------------------------------------------------------------------

/// Cast Telling Time for real and drive BOTH prompts through `apply()`.
///
/// This is the test that proves the whole stack: parser → `Effect::Dig` →
/// `DigChoice` → kept delivery → `DigRestSplitChoice` → `route_rest_split_then`
/// → library top/bottom. Reverting any single layer fails it.
#[test]
fn telling_time_splits_its_remainder_between_library_top_and_bottom() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mut builder =
        scenario.add_spell_to_hand_from_oracle(P0, "Telling Time", false, TELLING_TIME_ORACLE);
    builder.with_mana_cost(telling_time_cost());
    let spell_id = builder.id();

    let mut runner = scenario.build();

    // A deep-library sentinel proves a bottomed card really reached the
    // BOTTOM rather than merely "not the top".
    // `create_object` appends to the library's back, so the three looked-at
    // cards go in first (top) and the sentinel last (bottom).
    add_library_card(&mut runner, "Look0");
    add_library_card(&mut runner, "Look1");
    add_library_card(&mut runner, "Look2");
    let sentinel = add_library_card(&mut runner, "Deep Sentinel");

    add_mana(&mut runner, ManaType::Blue, 2);

    let lib_before = runner.state().players[0].library.len();
    let outcome = runner.cast(spell_id).resolve();

    // Stage 1: the ordinary keep prompt — look at 3, keep exactly 1. Read the
    // looked-at window from the engine rather than assuming a library order.
    let looked_at = match outcome.final_waiting_for() {
        WaitingFor::DigChoice {
            cards, keep_count, ..
        } => {
            assert_eq!(cards.len(), 3, "look at the top three cards");
            assert_eq!(*keep_count, 1, "exactly one card is kept");
            cards.clone()
        }
        other => panic!("expected DigChoice, got {other:?}"),
    };
    assert!(
        !looked_at.contains(&sentinel),
        "the deep sentinel must sit below the looked-at window; \
         the test needs an untouched card deeper than the three"
    );
    let kept = looked_at[0];
    let to_top = looked_at[1];
    let to_bottom = looked_at[2];

    runner
        .act(GameAction::SelectCards { cards: vec![kept] })
        .expect("keeping one of three must be accepted");

    // THE REGRESSION ASSERTION. Before the fix the engine auto-routed the two
    // unkept cards to the GRAVEYARD here and never raised a second prompt.
    let (split_pile, split_top_count) = match &runner.state().waiting_for {
        WaitingFor::DigRestSplitChoice {
            cards, top_count, ..
        } => (cards.clone(), *top_count),
        other => panic!("expected DigRestSplitChoice after the keep step, got {other:?}"),
    };
    assert_eq!(
        split_pile.len(),
        2,
        "the two unkept cards form the remainder"
    );
    assert_eq!(split_top_count, 1, "one of them goes on top");
    assert!(
        split_pile.contains(&to_top) && split_pile.contains(&to_bottom),
        "the remainder is exactly the two unkept cards"
    );
    assert_eq!(
        runner.state().objects[&to_top].zone,
        Zone::Library,
        "an unkept card must NOT have been milled to the graveyard"
    );
    assert_eq!(runner.state().objects[&to_bottom].zone, Zone::Library);

    // Stage 2: submit the split — one card on top, the other falls to the bottom.
    runner
        .act(GameAction::SelectCards {
            cards: vec![to_top],
        })
        .expect("a one-card top selection must be accepted");
    runner.advance_until_stack_empty();

    let st = runner.state();
    assert_eq!(
        st.objects[&kept].zone,
        Zone::Hand,
        "the kept card is in hand"
    );
    let library: Vec<ObjectId> = st.players[0].library.iter().copied().collect();
    assert_eq!(
        library.first(),
        Some(&to_top),
        "the chosen card is on TOP of the library"
    );
    assert_eq!(
        library.last(),
        Some(&to_bottom),
        "the unchosen card is on the BOTTOM of the library, below the sentinel"
    );
    assert!(
        library.contains(&sentinel),
        "the pre-existing library card is untouched"
    );
    assert_eq!(
        st.objects[&to_top].zone,
        Zone::Library,
        "neither remainder card reached the graveyard"
    );
    assert_eq!(st.objects[&to_bottom].zone, Zone::Library);
    // Exactly one card (the kept one) left the library.
    assert_eq!(
        st.players[0].library.len(),
        lib_before - 1,
        "only the kept card leaves the library"
    );
}

// ---------------------------------------------------------------------------
// Test 3 (HOSTILE): malformed split selections are rejected, not absorbed.
// ---------------------------------------------------------------------------

/// Park a `DigRestSplitChoice` over a two-card remainder with `top_count: 1`.
fn split_runner() -> (GameRunner, Vec<ObjectId>) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mut runner = scenario.build();
    let a = add_library_card(&mut runner, "Rest A");
    let b = add_library_card(&mut runner, "Rest B");
    runner.state_mut().waiting_for = WaitingFor::DigRestSplitChoice {
        player: P0,
        library_owner: P0,
        cards: vec![a, b],
        top_count: 1,
        source_id: None,
        completion: None,
    };
    (runner, vec![a, b])
}

#[test]
fn split_rejects_a_wrong_count_selection() {
    let (mut runner, pile) = split_runner();
    // Too many: the split is forced, not an "up to".
    let err = runner
        .act(GameAction::SelectCards {
            cards: pile.clone(),
        })
        .expect_err("a two-card top selection must be rejected when top_count is 1");
    assert!(
        format!("{err:?}").contains("exactly"),
        "expected an exact-count rejection, got {err:?}"
    );
    // REACH GUARD (paired positive): the prompt is still live and a
    // well-formed selection of the same pile IS accepted, so the rejection
    // above is about the count and not about an already-spent prompt.
    assert!(matches!(
        runner.state().waiting_for,
        WaitingFor::DigRestSplitChoice { .. }
    ));
    runner
        .act(GameAction::SelectCards {
            cards: vec![pile[0]],
        })
        .expect("the correctly sized selection must be accepted");
}

#[test]
fn split_rejects_an_empty_selection() {
    let (mut runner, _pile) = split_runner();
    runner
        .act(GameAction::SelectCards { cards: Vec::new() })
        .expect_err("declining is not a legal response to a forced split");
}

#[test]
fn split_rejects_a_duplicate_id() {
    let (mut runner, pile) = split_runner();
    // Count is right (2 ids) but both are the same card — accepting this would
    // put one card on top twice and strand the other.
    runner.state_mut().waiting_for = WaitingFor::DigRestSplitChoice {
        player: P0,
        library_owner: P0,
        cards: pile.clone(),
        top_count: 2,
        source_id: None,
        completion: None,
    };
    let err = runner
        .act(GameAction::SelectCards {
            cards: vec![pile[0], pile[0]],
        })
        .expect_err("a duplicate id must be rejected");
    assert!(
        format!("{err:?}").contains("duplicate"),
        "expected a duplicate rejection, got {err:?}"
    );
}

#[test]
fn split_rejects_a_foreign_id() {
    let (mut runner, _pile) = split_runner();
    let foreign = add_library_card(&mut runner, "Not In The Pile");
    let err = runner
        .act(GameAction::SelectCards {
            cards: vec![foreign],
        })
        .expect_err("an id outside the remainder pile must be rejected");
    assert!(
        format!("{err:?}").contains("not in the rest pile"),
        "expected a membership rejection, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 4 (AI): the candidate enumerator lists every legal split.
// ---------------------------------------------------------------------------

#[test]
fn ai_enumerates_every_legal_split() {
    let (runner, pile) = split_runner();
    let selections: Vec<Vec<ObjectId>> = engine::ai_support::legal_actions(runner.state())
        .into_iter()
        .filter_map(|action| match action {
            GameAction::SelectCards { cards } => Some(cards),
            _ => None,
        })
        .collect();
    assert_eq!(
        selections.len(),
        2,
        "C(2,1) = 2 legal splits, got {selections:?}"
    );
    assert!(selections.contains(&vec![pile[0]]));
    assert!(selections.contains(&vec![pile[1]]));
}

// ---------------------------------------------------------------------------
// Test 5 (SIBLING REGRESSION): a non-splitting dig is entirely unaffected.
// ---------------------------------------------------------------------------

/// `rest_split_top_count: None` must still auto-route the whole remainder to
/// `rest_destination` with NO second prompt — the unchanged path through
/// `route_rest_partition_then`.
#[test]
fn non_splitting_dig_still_auto_routes_its_whole_remainder() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mut runner = scenario.build();
    let a = add_library_card(&mut runner, "Dug A");
    let b = add_library_card(&mut runner, "Dug B");
    let c = add_library_card(&mut runner, "Dug C");
    runner.state_mut().waiting_for = WaitingFor::DigChoice {
        player: P0,
        library_owner: P0,
        cards: vec![a, b, c],
        keep_count: 1,
        up_to: false,
        selectable_cards: vec![a, b, c],
        kept_destination: Some(Zone::Hand),
        rest_destination: Some(Zone::Graveyard),
        rest_split_top_count: None,
        rest_order: engine::types::ability::DigRestOrder::Preserve,
        source_id: None,
        enter_tapped: false,
        enters_attacking: false,
    };

    runner
        .act(GameAction::SelectCards { cards: vec![a] })
        .expect("keeping one of three must be accepted");
    runner.advance_until_stack_empty();

    assert!(
        !matches!(
            runner.state().waiting_for,
            WaitingFor::DigRestSplitChoice { .. }
        ),
        "a dig with no split must never raise the split prompt"
    );
    let st = runner.state();
    assert_eq!(st.objects[&a].zone, Zone::Hand);
    assert_eq!(
        st.objects[&b].zone,
        Zone::Graveyard,
        "the whole remainder still goes uniformly to rest_destination"
    );
    assert_eq!(st.objects[&c].zone, Zone::Graveyard);
}
