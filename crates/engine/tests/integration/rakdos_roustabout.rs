//! Rakdos Roustabout — end-to-end integration tests.
//!
//! Tests for the card: Rakdos Roustabout — "Whenever this creature becomes blocked,
//! it deals 1 damage to the player or planeswalker it's attacking."
//!
//! CR 509.1h: A creature becomes blocked when it is declared as blocked.
//! CR 120.1: The damage is dealt to the player or planeswalker being attacked.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::card_type::CoreType;
use engine::types::format::FormatConfig;

const RAKDOS_ROUSTABOUT: &str = "Whenever this creature becomes blocked, it deals 1 damage to the player or planeswalker it's attacking.";

/// CR 509.1h + CR 120.1: When Rakdos Roustabout becomes blocked, it deals 1 damage
/// to the player it's attacking.
#[test]
fn rakdos_roustabout_deals_damage_when_blocked() {
    GameScenario::new_two_player(FormatConfig::standard(), 20)
        .add_creature_from_oracle(P0, "Rakdos Roustabout", 3, 2, RAKDOS_ROUSTABOUT)
        .add_creature_from_oracle(P1, "Blocker", 2, 2, "")
        .start_game()
        .pass_turns_until(P0)
        .tap_and_attack(P0, "Rakdos Roustabout", P1)
        .pass_priority_to(P1)
        .block(P1, "Blocker", "Rakdos Roustabout")
        .pass_priority_to(P0)
        .expect_trigger_ability("Rakdos Roustabout")
        .resolve_trigger()
        .expect_damage_dealt(P0, 1, P1);
}
