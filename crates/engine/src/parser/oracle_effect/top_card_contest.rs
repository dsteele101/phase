//! CR 701.13a + CR 202.3 + CR 608.2c: the top-card exile contest — "each player
//! exiles the top card of their library. The player who exiled the card with
//! the greatest mana value <instruction>. If two or more players' cards are
//! tied for greatest, the tied players repeat this process until the tie is
//! broken." (Timesifter).
//!
//! The three sentences are one instruction: the second names its subject by
//! the first's outcome and the third loops the first two (CR 608.2c — read the
//! whole text), so the chain lowers to a single `Effect::ExileTopCardContest`
//! whose nested `winner_effect` is the winner's instruction.

use nom::bytes::complete::tag;
use nom::combinator::{eof, opt};
use nom::Parser;

use super::parse_effect_chain_with_context;
use crate::parser::oracle_ir::ast::parsed_clause;
use crate::parser::oracle_ir::context::ParseContext;
use crate::parser::oracle_ir::effect_chain::EffectChainIr;
use crate::parser::oracle_nom::bridge::{nom_on_lower, split_once_on_lower};
use crate::parser::oracle_nom::error::OracleResult;
use crate::types::ability::{AbilityKind, Effect};

/// Separator between the winner's instruction and the tie sentence.
const TIE_SENTENCE_HEAD: &str = ". if two or more ";

/// "each player exiles the top card of their library. the player who exiled
/// the card with the greatest mana value " — the contest and its winner subject.
fn parse_contest_head(input: &str) -> OracleResult<'_, ()> {
    let (input, _) = tag("each player exiles the top card of their library. ").parse(input)?;
    let (input, _) =
        tag("the player who exiled the card with the greatest mana value ").parse(input)?;
    Ok((input, ()))
}

/// "players' cards are tied for greatest, the tied players repeat this process
/// until the tie is broken." — the repeat-among-the-tied tie rule.
fn parse_repeat_among_tied(input: &str) -> OracleResult<'_, ()> {
    let (input, _) = tag("players' cards are tied for greatest, ").parse(input)?;
    let (input, _) =
        tag("the tied players repeat this process until the tie is broken").parse(input)?;
    let (input, _) = opt(tag(".")).parse(input)?;
    let (input, _) = eof.parse(input)?;
    Ok((input, ()))
}

/// Parse the whole contest chain, or `None` (fall through to the generic chain
/// parser) when the text is not exactly this shape or the winner's instruction
/// does not parse.
pub(super) fn parse_exile_top_card_contest_ir(
    text: &str,
    kind: AbilityKind,
    ctx: &ParseContext,
) -> Option<EffectChainIr> {
    let text = text.trim();
    let lower = text.to_lowercase();
    let ((), rest) = nom_on_lower(text, &lower, parse_contest_head)?;
    let rest_lower = rest.to_lowercase();
    let (instruction, tie_sentence) = split_once_on_lower(rest, &rest_lower, TIE_SENTENCE_HEAD)?;
    let tie_lower = tie_sentence.to_lowercase();
    nom_on_lower(tie_sentence, &tie_lower, parse_repeat_among_tied)?;

    // The winner's instruction is a predicate whose subject is the winner
    // ("takes an extra turn after this one"); it parses like any "each player"
    // predicate, acting for the player the resolver binds.
    let winner_effect = parse_effect_chain_with_context(
        instruction,
        kind,
        &mut ParseContext {
            card_name: ctx.card_name.clone(),
            in_trigger: ctx.in_trigger,
            ..Default::default()
        },
    );
    if matches!(*winner_effect.effect, Effect::Unimplemented { .. }) {
        return None;
    }

    Some(EffectChainIr::single_clause(
        text,
        kind,
        parsed_clause(Effect::ExileTopCardContest {
            winner_effect: Box::new(winner_effect),
        }),
        None,
        ctx.actor.clone(),
        ctx.in_trigger,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::{QuantityExpr, TargetFilter};

    const TIMESIFTER: &str = "each player exiles the top card of their library. The player \
        who exiled the card with the greatest mana value takes an extra turn after this one. \
        If two or more players' cards are tied for greatest, the tied players repeat this \
        process until the tie is broken.";

    fn contest_winner_effect(text: &str) -> Option<Effect> {
        let ir =
            parse_exile_top_card_contest_ir(text, AbilityKind::Spell, &ParseContext::default())?;
        let def = super::super::lower::lower_effect_chain_ir(&ir);
        let Effect::ExileTopCardContest { winner_effect } = *def.effect else {
            panic!("expected ExileTopCardContest, got {:?}", def.effect);
        };
        Some(*winner_effect.effect)
    }

    #[test]
    fn contest_lowers_winner_instruction_as_nested_effect() {
        let winner = contest_winner_effect(TIMESIFTER).expect("Timesifter contest must parse");
        assert_eq!(
            winner,
            Effect::ExtraTurn {
                target: TargetFilter::Controller,
                count: QuantityExpr::Fixed { value: 1 },
            }
        );
    }

    #[test]
    fn contest_requires_the_repeat_tie_rule() {
        // A different tie rule is a different contest; decline it.
        let no_repeat = "each player exiles the top card of their library. The player who \
            exiled the card with the greatest mana value takes an extra turn after this one. \
            If two or more players' cards are tied for greatest, nothing happens.";
        assert!(contest_winner_effect(no_repeat).is_none());
        // Reach-guard: the same chain with the repeat rule does parse.
        assert!(contest_winner_effect(TIMESIFTER).is_some());
    }

    #[test]
    fn contest_declines_an_unparsed_winner_instruction() {
        let unparsed = "each player exiles the top card of their library. The player who \
            exiled the card with the greatest mana value frobnicates wildly. If two or more \
            players' cards are tied for greatest, the tied players repeat this process until \
            the tie is broken.";
        assert!(contest_winner_effect(unparsed).is_none());
    }
}
