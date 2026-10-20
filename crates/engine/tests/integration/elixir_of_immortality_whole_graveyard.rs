//! Elixir of Immortality — "{2}, {T}: You gain 5 life. Shuffle this artifact
//! and your graveyard into their owner's library."
//!
//! The graveyard is shuffled as a whole zone: activation must not ask for a
//! graveyard-card target, and every card in the controller's graveyard (plus
//! the Elixir itself) must end up in the library.
//!
//! CR references (verified against docs/MagicCompRules.txt):
//!   - CR 701.24a: to shuffle a library, randomize its cards.
//!   - CR 400.6: a zone change moves the object to the new zone.
//!   - CR 115.1: only words "target" on the ability create targets.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::ability::EffectKind;
use engine::types::events::GameEvent;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const ELIXIR: &str = "{2}, {T}: You gain 5 life. Shuffle this artifact and your graveyard into their owner's library.";

#[test]
fn elixir_shuffles_entire_graveyard_without_targeting() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let elixir = scenario
        .add_artifact_from_oracle(P0, "Elixir of Immortality", ELIXIR)
        .id();
    let mine_a = scenario.add_creature_to_graveyard(P0, "Mine A", 1, 1).id();
    let mine_b = scenario.add_creature_to_graveyard(P0, "Mine B", 2, 2).id();
    let theirs = scenario.add_creature_to_graveyard(P1, "Theirs", 3, 3).id();
    scenario.with_mana_pool(
        P0,
        vec![ManaUnit::new(ManaType::Colorless, ObjectId(9_999), false, vec![]); 2],
    );
    let mut runner = scenario.build();
    let life_before = runner.state().players[0].life;

    // No target selection is supplied: a graveyard-card target slot would stall here.
    let outcome = runner.activate(elixir, 0).resolve();

    // The terminal library shuffle must resolve through the production event path.
    let terminal_shuffles = outcome
        .events()
        .iter()
        .filter(|event| {
            matches!(
                event,
                GameEvent::EffectResolved {
                    kind: EffectKind::Shuffle,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        terminal_shuffles, 1,
        "the terminal library shuffle must resolve"
    );

    let state = outcome.state();
    assert_eq!(state.players[0].life, life_before + 5);
    for id in [elixir, mine_a, mine_b] {
        assert_eq!(
            state.objects[&id].zone,
            Zone::Library,
            "{id:?} must be shuffled in"
        );
    }
    assert_eq!(
        state.objects[&theirs].zone,
        Zone::Graveyard,
        "an opponent's graveyard is untouched"
    );
}
