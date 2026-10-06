//! Format context for an LLM opponent: which Magic format is being played, and
//! how a player approaches it.
//!
//! The game engine already knows the format — it is `GameState::format_config`,
//! fixed when the game is created — and a draft pod knows its `DraftKind`. A
//! model that is not told is left to guess whether it is in a two-player
//! 60-card duel, a four-player Commander pod, or a 40-card Limited game, and
//! those want different play: who should be the aggressor, whether to hold
//! interaction, whether being the biggest threat is a liability.
//!
//! # Two kinds of statement
//!
//! - **Facts** come from the engine's [`FormatConfig`] (player count, starting
//!   life, deck-size rule, singleton, commander damage). They are read, never
//!   restated, so a format whose rules change cannot drift from the prompt.
//! - **Strategy** is the per-format approach from the format strategy guide:
//!   the mindset a player brings and the tendencies that follow from it. It is
//!   deliberately general. Named decks are not carried over — metagames move,
//!   and a model told which deck is "top" will play as if that were still true.
//!
//! Every statement here is engine-authored and static, so it belongs in the
//! system prompt, outside the untrusted-data fence: none of it is rendered from
//! card text, player names, or provider output.
//!
//! # Freeform
//!
//! `Freeform`, `FreeformCommander`, and `Custom` formats have no fixed card
//! pool, power level, or deck rule, so there is no format-specific approach to
//! teach. They receive [`GENERIC_STRATEGY`] — the guide's "unknown meta"
//! guidance — plus whatever facts their `FormatConfig` carries.

use engine::types::format::{FormatConfig, GameFormat};
use phase_ai::config::AiDifficulty;

/// Principles that hold in every format. Appended after the format's own
/// strategy so a format that says nothing about, say, mulligans still inherits
/// them.
pub const UNIVERSAL_PRINCIPLES: &str = "In every format: decide whether you are the beatdown \
     (the player who should pressure) or the control in this matchup and play to that role; \
     track your resources, your opponents' open mana and graveyards, and what they could be \
     holding; know your outs and theirs; and when deciding whether to mulligan, remember that \
     a functional hand with fewer cards beats a non-functional seven.";

/// The guide's guidance for an unknown metagame, and the strategy for every
/// format with no format-specific approach. It makes no claim about the format's
/// rules: a custom format may fix its card pool and deck rules, and the format
/// facts stated beside this text are what say so.
pub const GENERIC_STRATEGY: &str = "This format has no established metagame, so do not assume \
     anything about your opponents' decks beyond what you can see; the format rules stated \
     above are the only rules you can rely on. Play a consistent, proactive game: develop your mana and board, apply pressure with \
     what you have shown you can protect, and keep interaction for the cards that actually \
     threaten you.";

// ── Strategy text, one entry per format family ───────────────────────────────
//
// Each is the mindset a player brings, not a decklist. Wording follows the
// format strategy guide; where a format's pool is a moving target the guide says
// to verify the live metagame, so these name tendencies rather than decks.

const STANDARD: &str = "Standard: a small, rotating card pool, so decks are streamlined and \
     reward synergy and a coherent plan. Expect fast aggro, midrange value, and control. \
     Proactive, consistent plans are favoured; work out early who is the beatdown, and plan for \
     the mirror and for the two or three most common archetypes.";

const LIMITED: &str = "Limited (draft or sealed): a deck built from a pool, with about 16-17 \
     lands, so power is lower and games are decided by card quality, curve, and removal. Bombs, \
     evasive threats, and efficient removal are the strongest cards. Develop on curve, make \
     efficient trades, and count the race before attacking. Aggressive, consistent decks tend \
     to beat clunky ones.";

const PIONEER: &str = "Pioneer: a streamlined 'Standard plus' format where aggro, combo, and \
     midrange are closely matched and mana bases are good but not perfect. Fair decks live or \
     die by whether they interact early, so deal with the pieces of an opposing combo or aggro \
     plan before they come online.";

const MODERN: &str = "Modern: a fast format in which each deck is tuned to do something powerful, \
     often by turn four. Be proactive; a reactive plan needs very efficient answers. Respect \
     graveyard strategies, artifacts and enchantments, and combo, and weigh speed against life \
     loss where fetch lands, shock lands, and pain lands are in play.";

const PREMODERN: &str = "Premodern: a fundamentals-first format of removal, counterspells, \
     creature quality, and card advantage, with longer games than Modern. Play to the board and \
     make efficient trades; overextending into a sweeper such as Wrath of God is a real risk. \
     Blue control and tempo are strong because counterspells are cheap.";

const LEGACY: &str = "Legacy: powerful decks kept honest by cheap interaction — cheap and free \
     counterspells, discard, and removal. Tempo and disruption are strong: cheap threats backed \
     by interaction punish slow draws. Combo punishes durdling, so keep relevant answers or \
     pressure available, and sequence card-selection spells such as Brainstorm carefully to hide \
     information.";

const VINTAGE: &str =
    "Vintage: raw power is the baseline, so speed, redundancy, and the timing of \
     interaction decide games, and many games end on turns one to three. Assume an opponent can \
     win on the spot; hold cheap interaction (free spells, counterspells, hate pieces) and use \
     it at the right moment. Tutors and card selection find the right card for the matchup; \
     favour hands that can both execute and interact.";

const PAUPER: &str = "Pauper: commons only, so no single bomb decides games — card quality, mana, \
     and tempo do. Card advantage and value engines beat decks that only trade one-for-one. \
     Aggro needs efficient creatures and reach such as burn; control needs a real plan to close \
     the game, since commons rarely win quickly.";

const HISTORIC: &str = "Historic: a non-rotating Arena format with powerful digital-only cards. \
     The best decks are streamlined versions of established archetypes. Play to the board, \
     expect well-tuned archetypes, and read digital-only card text carefully.";

const TIMELESS: &str = "Timeless: an extremely high-powered Arena format. Speed and interaction \
     matter. Play to the board and be ready for explosive turns.";

// CR 903.8: a commander may be cast from the command zone for an additional {2}
// per previous cast, so recasting gets steadily more expensive.
// CR 903.10a: 21 or more combat damage from the same commander eliminates a player.
const COMMANDER: &str = "Commander: a command-zone format that rewards resource management and \
     long-game planning over speed. At a table of more than two players it is also social: do \
     not be the first or biggest threat — develop your board and resources while the others \
     fight, assess which opponent is the real threat, and consider when to hold removal \
     rather than spend it. In a two-player game, play the head-to-head matchup directly. \
     Your commander is a repeatable engine: protect it, and remember each recast costs more in \
     commander tax. Commander damage is tracked per commander, so watch who is accumulating \
     it. If the format facts above say the deck is singleton, redundancy comes from different \
     cards with similar effects.";

const COMMANDER_DRAFT: &str = "Commander Draft: your deck came from a draft pool rather than a \
     tuned list, so play the strengths of the cards you actually have rather than an \
     idealised plan.";

const PAUPER_COMMANDER: &str = "Pauper Commander: a lower-power, social Commander variant where \
     common-based synergy and value decks dominate. Look for repeatable value engines and good \
     removal, and play around the synergy your commander gives your deck.";

const DUEL_COMMANDER: &str = "Duel Commander: a two-player, tempo-oriented Commander variant that \
     plays closer to Legacy than to multiplayer EDH. Be proactive: tempo and commander damage \
     matter more, and board wipes are less useful. Your commander is a repeatable threat you can \
     recast, so plan around commander tax.";

const TINY_LEADERS: &str = "Tiny Leaders: a low-curve singleton format where mana value 3 or less \
     shapes everything. Tempo and efficiency matter: curve out with efficient threats and \
     acceleration rather than waiting for expensive bombs.";

const OATHBREAKER: &str = "Oathbreaker: a tight, strategic singleton format built around a \
     planeswalker and a signature spell in the command zone. Use the planeswalker's repeatable \
     ability and the signature spell as a reliable engine. Games usually run faster than \
     Commander because decks are smaller.";

const BRAWL: &str = "Brawl: a commander format with a restricted card pool, so games tend to \
     feel more focused than in unrestricted Commander. Build your play around your commander \
     and your deck's synergy, within the deck size and card pool the format facts above state.";

const FREE_FOR_ALL: &str = "Free-for-all: every player is playing for themselves. Avoid being the \
     biggest threat; politics and resource management matter more than raw aggression.";

// CR 810.9: damage, life loss, and life gain happen to each player individually
// and the result is applied to the team's shared life total.
// CR 810.8a: players win and lose the game only as a team.
const TWO_HEADED_GIANT: &str = "Two-Headed Giant: you and your teammate share one life total and \
     win or lose together. Damage and life gain apply to the team's total, so coordinate with \
     your partner, protect the shared life total, and pressure the opposing team's.";

// CR 904.1: one archenemy, strengthened by scheme cards, faces a team.
const ARCHENEMY: &str = "Archenemy: one archenemy strengthened by scheme cards faces a team of \
     heroes. The heroes should coordinate against the archenemy and its schemes; the archenemy \
     should leverage its schemes to snowball.";

// CR 901.1: Planechase adds plane and phenomenon cards to a normal game.
const PLANECHASE: &str = "Planechase: plane cards and the planar die add chaos to a normal game. \
     Do not over-rely on a fixed board state, and account for the current plane's effect when \
     planning your turn.";

const MOMIR: &str = "Momir's Madness: the deck is only basic lands and the game is driven by the \
     Momir emblem. Almost everything is luck, but mana efficiency matters: choose when to make \
     a large creature and when a small one.";

/// The format-specific strategy for a built-in format, or `None` for a format
/// that has no fixed approach and takes [`GENERIC_STRATEGY`].
///
/// Exhaustive over [`GameFormat`] with no wildcard: a new built-in format must
/// decide here whether it teaches something specific, and the compiler holds
/// that decision.
fn format_strategy(format: GameFormat) -> Option<&'static [&'static str]> {
    let parts: &'static [&'static str] = match format {
        GameFormat::Standard => &[STANDARD],
        GameFormat::Limited => &[LIMITED],
        GameFormat::Pioneer => &[PIONEER],
        GameFormat::Modern => &[MODERN],
        GameFormat::Premodern => &[PREMODERN],
        GameFormat::Legacy => &[LEGACY],
        GameFormat::Vintage => &[VINTAGE],
        GameFormat::Pauper => &[PAUPER],
        GameFormat::Historic => &[HISTORIC],
        GameFormat::Timeless => &[TIMELESS],
        GameFormat::Commander => &[COMMANDER],
        GameFormat::CommanderDraft => &[COMMANDER_DRAFT, COMMANDER],
        GameFormat::PauperCommander => &[PAUPER_COMMANDER],
        GameFormat::DuelCommander => &[DUEL_COMMANDER],
        GameFormat::TinyLeaders => &[TINY_LEADERS],
        GameFormat::Oathbreaker => &[OATHBREAKER],
        GameFormat::Brawl | GameFormat::HistoricBrawl => &[BRAWL],
        GameFormat::FreeForAll => &[FREE_FOR_ALL],
        GameFormat::TwoHeadedGiant => &[TWO_HEADED_GIANT],
        GameFormat::Archenemy => &[ARCHENEMY],
        GameFormat::Planechase => &[PLANECHASE],
        GameFormat::Momir => &[MOMIR],
        // No fixed pool, power level, or deck rule: nothing format-specific to
        // teach, so these take the generic guidance.
        GameFormat::Freeform | GameFormat::FreeformCommander | GameFormat::Custom(_) => {
            return None;
        }
    };
    Some(parts)
}

/// The engine-derived facts of a game format, as one sentence.
///
/// Read from `config` rather than from the format's name so a custom format
/// reports the rules it actually runs under.
fn format_facts(config: &FormatConfig) -> String {
    // The format's permitted seat counts, not this game's: the board shows who is
    // actually seated.
    let players = if config.min_players == config.max_players {
        format!("exactly {} players", config.max_players)
    } else {
        format!(
            "allows {}-{} players",
            config.min_players, config.max_players
        )
    };
    let mut facts = vec![
        players,
        format!("{} starting life", config.starting_life),
        format!("a deck of {} cards", config.deck_size.requirement_phrase()),
    ];
    if config.singleton {
        facts.push("singleton".to_string());
    }
    if config.team_based {
        facts.push("team-based".to_string());
    }
    if let Some(threshold) = config.commander_damage_threshold {
        // CR 903.10a: this much combat damage from one commander eliminates a player.
        facts.push(format!("{threshold} commander damage eliminates a player"));
    }
    format!("{}: {}.", config.format.label(), facts.join(", "))
}

/// The format section of an in-game system prompt.
///
/// The lowest difficulty receives the facts only. Difficulty is the lever that
/// makes an LLM seat comparable to a heuristic seat at the same setting, and a
/// brand-new player does not know format theory — knowing the player count and
/// starting life is not the same as knowing how to play to a metagame.
///
/// The section yields to the difficulty brief on how hard and how fast to play:
/// format strategy says what the game is about, not how well to play it.
pub fn game_format_brief(config: &FormatConfig, difficulty: AiDifficulty) -> String {
    let facts = format_facts(config);
    if matches!(difficulty, AiDifficulty::VeryEasy) {
        return format!("FORMAT: {facts}");
    }
    let strategy = match format_strategy(config.format) {
        Some(parts) => parts.join("\n"),
        None => GENERIC_STRATEGY.to_string(),
    };
    format!(
        "FORMAT: {facts}\n{strategy}\n{UNIVERSAL_PRINCIPLES}\nIf this format guidance and \
         your playing-strength description disagree about how fast or how aggressively to \
         play, follow your playing-strength description."
    )
}

// ── Draft strategy ───────────────────────────────────────────────────────────

/// Draft-pod guidance. Gated with the rest of the draft rendering so a game-only
/// consumer (`engine-wasm`) neither links `draft-core` nor carries this text.
#[cfg(feature = "draft")]
mod draft {
    use super::*;

    const BOOSTER_DRAFT: &str =
        "Booster draft: stay flexible early, settle on two colours (rarely \
         three) around picks 6-8 of the first pack, and prioritise removal, bombs, and a smooth \
         curve. Read signals — which colours are still flowing tells you what the players passing \
         to you are not taking. In pack 1 take the best card; in pack 2 adjust to what flowed; in \
         pack 3 fill the gaps in your curve.";

    const SEALED: &str = "Sealed deck: you have more cards than you can play, so building is the \
         skill. Build the most consistent deck rather than the one with the most flashy cards: pick \
         the best two colours, count removal and bombs, build a smooth curve, and splash only for \
         strong cards with easy mana.";

    const WINSTON_DRAFT: &str = "Winston draft: information and denial are central. Each decision \
         is two-sided — consider both what you want and what you would be leaving for your \
         opponent.";

    const CUBE_DRAFT: &str =
        "This is a cube: its power level and themes decide how to draft. Every \
         pick is usually strong, so synergy and a coherent archetype matter more than raw card \
         power. Draft mana fixing and a plan, since there is no filler.";

    const COMMANDER_DRAFT_PICKS: &str = "Commander draft: you take two cards per step and play a \
         multiplayer Commander game with the result, so value cards that fit the plan of a \
         multiplayer game — ramp, card advantage, removal, and a commander you will want to cast \
         repeatedly.";

    /// The format section of a draft system prompt.
    ///
    /// What is being drafted — the procedure, and whether the card source is a
    /// cube — is already stated as data in the user message; this adds how a drafter
    /// approaches it.
    pub fn draft_format_brief(
        view: &draft_core::view::DraftPlayerView,
        difficulty: AiDifficulty,
    ) -> String {
        use draft_core::types::DraftKind;
        use draft_core::view::DraftSourceView;

        if matches!(difficulty, AiDifficulty::VeryEasy) {
            return String::new();
        }
        let mut parts = vec![match view.kind {
            DraftKind::Quick | DraftKind::Premier | DraftKind::Traditional => BOOSTER_DRAFT,
            DraftKind::Sealed => SEALED,
            DraftKind::CommanderDraft => COMMANDER_DRAFT_PICKS,
            DraftKind::Winston => WINSTON_DRAFT,
        }];
        if matches!(view.source, DraftSourceView::Cube { .. }) {
            parts.push(CUBE_DRAFT);
        }
        parts.push(
            "Limited decks are built around card quality, a smooth curve, and removal; \
             aggressive, consistent decks tend to beat clunky good-stuff piles.",
        );
        format!("FORMAT GUIDANCE:\n{}", parts.join("\n"))
    }
}

#[cfg(feature = "draft")]
pub use draft::draft_format_brief;

#[cfg(test)]
mod tests {
    use super::*;

    const DIFFICULTIES: [AiDifficulty; 6] = [
        AiDifficulty::VeryEasy,
        AiDifficulty::Easy,
        AiDifficulty::Medium,
        AiDifficulty::Hard,
        AiDifficulty::VeryHard,
        AiDifficulty::CEDH,
    ];

    /// The registry is the engine's own list of every user-selectable built-in
    /// format, so iterating it covers a format added after this module was written.
    fn builtin_configs() -> Vec<FormatConfig> {
        GameFormat::registry()
            .into_iter()
            .map(|meta| meta.default_config)
            .collect()
    }

    #[test]
    fn every_builtin_format_gets_a_brief_naming_that_format() {
        for config in builtin_configs() {
            let brief = game_format_brief(&config, AiDifficulty::Medium);
            assert!(
                brief.contains(&*config.format.label()),
                "{} brief does not name its format: {brief}",
                config.format
            );
            assert!(brief.contains(UNIVERSAL_PRINCIPLES), "{brief}");
        }
    }

    /// Only the formats with no fixed pool or power level take generic guidance;
    /// every other built-in format teaches something of its own.
    #[test]
    fn only_freeform_formats_take_the_generic_strategy() {
        for config in builtin_configs() {
            let brief = game_format_brief(&config, AiDifficulty::Medium);
            let generic = brief.contains(GENERIC_STRATEGY);
            let expects_generic = matches!(
                config.format,
                GameFormat::Freeform | GameFormat::FreeformCommander
            );
            assert_eq!(
                generic, expects_generic,
                "{} generic={generic}: {brief}",
                config.format
            );
        }
    }

    #[test]
    fn a_custom_format_takes_generic_guidance_and_its_own_facts() {
        let mut config = FormatConfig::standard();
        config.format = GameFormat::Custom(engine::types::custom_format::CustomFormatId(9));
        config.starting_life = 30;
        let brief = game_format_brief(&config, AiDifficulty::Hard);
        assert!(brief.contains(GENERIC_STRATEGY), "{brief}");
        assert!(brief.contains("30 starting life"), "{brief}");
    }

    #[test]
    fn distinct_formats_do_not_share_a_strategy_unless_the_guide_merges_them() {
        // Brawl and Historic Brawl are one entry in the guide.
        let shared = [(GameFormat::Brawl, GameFormat::HistoricBrawl)];
        for config in builtin_configs() {
            for other in builtin_configs() {
                if config.format == other.format {
                    continue;
                }
                let a = format_strategy(config.format);
                let b = format_strategy(other.format);
                if a.is_none() || b.is_none() || a != b {
                    continue;
                }
                assert!(
                    shared.contains(&(config.format, other.format))
                        || shared.contains(&(other.format, config.format)),
                    "{} and {} share a strategy",
                    config.format,
                    other.format
                );
            }
        }
    }

    #[test]
    fn facts_come_from_the_engine_config() {
        let commander = game_format_brief(&FormatConfig::commander(), AiDifficulty::Medium);
        assert!(commander.contains("40 starting life"), "{commander}");
        assert!(commander.contains("singleton"), "{commander}");
        assert!(
            commander.contains("21 commander damage eliminates a player"),
            "{commander}"
        );
        assert!(commander.contains("exactly 100"), "{commander}");

        let modern = game_format_brief(&FormatConfig::modern(), AiDifficulty::Medium);
        assert!(modern.contains("exactly 2 players"), "{modern}");
        assert!(modern.contains("20 starting life"), "{modern}");
        assert!(!modern.contains("singleton"), "{modern}");
    }

    /// A custom format may fix its card pool and deck rules (the Old School
    /// preset does), so the generic text must not deny rules that the facts beside
    /// it state.
    #[test]
    fn generic_guidance_does_not_deny_the_rules_the_facts_state() {
        assert!(!GENERIC_STRATEGY.contains("no fixed"), "{GENERIC_STRATEGY}");
        assert!(
            !GENERIC_STRATEGY.contains("deck rule"),
            "{GENERIC_STRATEGY}"
        );
    }

    /// Commander Draft is not singleton (CR 903.13f(2)) and Historic Brawl is 100
    /// cards, so neither may inherit unconditional singleton or 60-card claims.
    #[test]
    fn shared_commander_and_brawl_text_does_not_contradict_the_variants() {
        let commander_draft =
            game_format_brief(&FormatConfig::commander_draft(), AiDifficulty::Hard);
        assert!(commander_draft.contains("If the format facts above say the deck is singleton"));
        assert!(
            !commander_draft.contains("Singleton means"),
            "{commander_draft}"
        );

        let historic = game_format_brief(&FormatConfig::historic_brawl(), AiDifficulty::Hard);
        assert!(!historic.contains("60-card"), "{historic}");
        assert!(historic.contains("exactly 100"), "{historic}");
    }

    /// Commander permits two players, so multiplayer politics is conditional.
    #[test]
    fn commander_politics_is_conditional_on_more_than_two_players() {
        let brief = game_format_brief(&FormatConfig::commander(), AiDifficulty::Hard);
        assert!(brief.contains("more than two players"), "{brief}");
        assert!(brief.contains("allows 2-6 players"), "{brief}");
        assert!(!brief.contains("social multiplayer game"), "{brief}");
    }

    #[test]
    fn the_lowest_difficulty_gets_facts_but_no_strategy() {
        let brief = game_format_brief(&FormatConfig::modern(), AiDifficulty::VeryEasy);
        assert!(brief.contains("Modern"), "{brief}");
        assert!(!brief.contains(MODERN), "{brief}");
        assert!(!brief.contains(UNIVERSAL_PRINCIPLES), "{brief}");
        for difficulty in DIFFICULTIES.into_iter().skip(1) {
            let brief = game_format_brief(&FormatConfig::modern(), difficulty);
            assert!(brief.contains(MODERN), "{difficulty:?}: {brief}");
        }
    }

    #[test]
    fn the_brief_yields_to_the_difficulty_brief_on_pace() {
        let brief = game_format_brief(&FormatConfig::commander(), AiDifficulty::CEDH);
        assert!(
            brief.contains("follow your playing-strength description"),
            "{brief}"
        );
    }

    #[test]
    fn commander_draft_teaches_commander_play_on_top_of_the_draft_framing() {
        let brief = game_format_brief(&FormatConfig::commander_draft(), AiDifficulty::Medium);
        assert!(brief.contains(COMMANDER_DRAFT), "{brief}");
        assert!(brief.contains(COMMANDER), "{brief}");
    }

    /// The brief is static, engine-authored text living outside the data fence, so
    /// it must not itself contain a fence marker.
    #[test]
    fn no_brief_contains_a_fence_marker() {
        use crate::prompt::{UNTRUSTED_DATA_BEGIN, UNTRUSTED_DATA_END};
        for config in builtin_configs() {
            for difficulty in DIFFICULTIES {
                let brief = game_format_brief(&config, difficulty);
                assert!(!brief.contains(UNTRUSTED_DATA_BEGIN), "{brief}");
                assert!(!brief.contains(UNTRUSTED_DATA_END), "{brief}");
            }
        }
    }
}
