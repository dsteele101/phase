//! Uba Mask — "If a player would draw a card, that player exiles that card face
//! up instead. / Each player may play lands and cast spells from among cards
//! they exiled with this artifact this turn." (Oracle text verified vs Scryfall.)
//!
//! Covers the two building blocks the card composes:
//!   * CR 121.1 + CR 614.6: a draw replacement whose head is "exiles that card"
//!     exiles the top card of the *drawing* player's library.
//!   * CR 406.6 + CR 607.1: an "each player may … cards they exiled with ~ this
//!     turn" exile-play grant (`ExileCastGrantee::EachPlayerOwnExiles`, per-turn
//!     pool) lets each player — and only that player — play their own exiles,
//!     only during the turn they were exiled.

use engine::ai_support::legal_actions;
use engine::game::casting::spell_objects_available_to_cast;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{Effect, LibraryPosition, TargetFilter};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::statics::{ExileCardPool, ExileCastGrantee, StaticMode};
use engine::types::zones::Zone;

const UBA_MASK: &str = "If a player would draw a card, that player exiles that card face up instead.\nEach player may play lands and cast spells from among cards they exiled with this artifact this turn.";

const DRAW_A_CARD: &str = "Target player draws a card.";

fn zone(runner: &GameRunner, id: ObjectId) -> Zone {
    runner.state().objects[&id].zone
}

fn in_hand(runner: &GameRunner, player: PlayerId, id: ObjectId) -> bool {
    runner
        .state()
        .players
        .iter()
        .find(|p| p.id == player)
        .is_some_and(|p| p.hand.contains(&id))
}

fn can_play_land(runner: &GameRunner, id: ObjectId) -> bool {
    legal_actions(runner.state())
        .iter()
        .any(|action| matches!(action, GameAction::PlayLand { object_id, .. } if *object_id == id))
}

/// Make a staged card a land card (no rules text needed).
fn make_land(runner: &mut GameRunner, id: ObjectId) {
    let obj = runner.state_mut().objects.get_mut(&id).unwrap();
    obj.card_types.core_types.push(CoreType::Land);
    obj.base_card_types = obj.card_types.clone();
}

#[test]
fn uba_mask_parses_to_drawer_exile_top_and_each_player_grant() {
    let parsed = parse_oracle_text(UBA_MASK, "Uba Mask", &[], &["Artifact".to_string()], &[]);

    let replacement = parsed
        .replacements
        .first()
        .expect("Uba Mask must parse a draw replacement");
    let execute = replacement.execute.as_deref().expect("replacement execute");
    assert!(
        matches!(
            &*execute.effect,
            Effect::ExileTop {
                player: TargetFilter::PostReplacementDamageTarget,
                position: LibraryPosition::Top,
                face_down: false,
                ..
            }
        ),
        "\"that player exiles that card\" must exile the drawing player's top card, got {:?}",
        execute.effect
    );

    let grant = parsed
        .statics
        .iter()
        .find_map(|s| match &s.mode {
            StaticMode::ExileCastPermission { grantee, pool, .. } => Some((*grantee, *pool)),
            _ => None,
        })
        .expect("Uba Mask must parse an ExileCastPermission static");
    assert_eq!(
        grant,
        (
            ExileCastGrantee::EachPlayerOwnExiles,
            ExileCardPool::ThisTurn
        )
    );
    assert!(
        parsed.abilities.is_empty(),
        "no Unimplemented fallback may remain: {:?}",
        parsed.abilities
    );
}

/// CR 121.1 + CR 614.6 + CR 305.1: The controller's own draw is replaced by
/// exiling that card face up, and the controller may play it as a land this
/// turn — the opponent may not.
#[test]
fn controller_draw_is_exiled_and_playable_as_land_this_turn() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_artifact_from_oracle(P0, "Uba Mask", UBA_MASK);
    let land = scenario.add_card_to_library_top(P0, "Masked Forest");
    let draw = scenario
        .add_spell_to_hand_from_oracle(P0, "Peek", true, DRAW_A_CARD)
        .id();
    let mut runner = scenario.build();
    make_land(&mut runner, land);

    runner.cast(draw).target_player(P0).resolve();

    assert_eq!(
        zone(&runner, land),
        Zone::Exile,
        "the draw must exile the card"
    );
    assert!(
        !in_hand(&runner, P0, land),
        "the replaced draw must not happen"
    );
    assert!(
        !runner.state().objects[&land].face_down,
        "CR 406.3: Uba Mask exiles the card face up"
    );

    assert!(
        can_play_land(&runner, land),
        "the exiling player may play the exiled land this turn"
    );
    let card_id = runner.state().objects[&land].card_id;
    runner
        .act(GameAction::PlayLand {
            object_id: land,
            card_id,
        })
        .expect("playing the exiled land must succeed");
    assert_eq!(zone(&runner, land), Zone::Battlefield);
}

/// CR 406.6 + CR 607.1: "Each player may … cards *they* exiled" — an opponent's
/// exiled draw is castable by that opponent, never by Uba Mask's controller.
#[test]
fn opponent_draw_is_castable_only_by_that_opponent() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_artifact_from_oracle(P0, "Uba Mask", UBA_MASK);
    let spell = scenario
        .add_spell_to_library_top(P1, "Opponent Instant", true)
        .id();
    let draw = scenario
        .add_spell_to_hand_from_oracle(P0, "Peek", true, DRAW_A_CARD)
        .id();
    let mut runner = scenario.build();

    runner.cast(draw).target_player(P1).resolve();

    assert_eq!(zone(&runner, spell), Zone::Exile);
    assert!(!in_hand(&runner, P1, spell));
    assert!(
        spell_objects_available_to_cast(runner.state(), P1).contains(&spell),
        "the player who exiled the card may cast it"
    );
    assert!(
        !spell_objects_available_to_cast(runner.state(), P0).contains(&spell),
        "Uba Mask's controller may not cast a card another player exiled"
    );
}

/// CR 406.6: "…exiled with this artifact *this turn*" — a card exiled on an
/// earlier turn stays in exile and is no longer playable.
#[test]
fn exiled_card_is_not_playable_on_a_later_turn() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_artifact_from_oracle(P0, "Uba Mask", UBA_MASK);
    let spell = scenario
        .add_spell_to_library_top(P0, "Old Instant", true)
        .id();
    let draw = scenario
        .add_spell_to_hand_from_oracle(P0, "Peek", true, DRAW_A_CARD)
        .id();
    let mut runner = scenario.build();

    runner.cast(draw).target_player(P0).resolve();
    assert!(spell_objects_available_to_cast(runner.state(), P0).contains(&spell));

    let mut events = Vec::new();
    engine::game::turns::start_next_turn(runner.state_mut(), &mut events);

    assert_eq!(zone(&runner, spell), Zone::Exile, "the card stays exiled");
    for player in [P0, P1] {
        assert!(
            !spell_objects_available_to_cast(runner.state(), player).contains(&spell),
            "a card exiled on a previous turn must not be castable"
        );
    }
}
