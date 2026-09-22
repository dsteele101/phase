// CR 606.3 — planeswalker loyalty activation statics.
// CR 602.5e + CR 702.6a — equip activation timing statics.

#[allow(unused_imports)]
use super::prelude::*;
#[allow(unused_imports)]
use super::support::*;
use crate::types::ability::{AbilityTag, ControllerRef, TypedFilter};

pub(crate) fn parse_self_loyalty_activation_permission(input: &str) -> OracleResult<'_, ()> {
    value(
        (),
        (
            tag("you may activate "),
            opt(alt((
                tag("her "),
                tag("his "),
                tag("its "),
                tag("their "),
                tag("~'s "),
            ))),
            tag("loyalty abilities any time you could cast an instant"),
        ),
    )
    .parse(input)
}

pub(crate) fn parse_loyalty_activation_timing_permission(
    tp: &TextPair<'_>,
    text: &str,
) -> Option<StaticDefinition> {
    let condition = nom_on_lower(tp.original, tp.lower, |i| {
        let (i, condition_text) =
            preceded(tag("as long as "), terminated(take_until(", "), tag(", "))).parse(i)?;
        let (i, _) = parse_self_loyalty_activation_permission(i)?;
        let (i, _) = opt(tag(".")).parse(i)?;
        let (i, _) = all_consuming(value((), tag(""))).parse(i)?;
        Ok((i, condition_text.to_string()))
    })
    .map(|(condition_text, _)| {
        parse_static_condition(&condition_text).unwrap_or(StaticCondition::Unrecognized {
            text: condition_text,
        })
    })?;

    Some(
        StaticDefinition::new(StaticMode::ActivateAsInstant {
            cost_category: CostCategory::PaysLoyalty,
            keyword: None,
        })
        .affected(TargetFilter::SelfRef)
        .condition(condition)
        .description(text.to_string()),
    )
}

/// CR 602.5e + CR 702.6a: "You may activate equip abilities any time you could
/// cast an instant." (Leonin Shikari). Unlike the loyalty form above, this
/// permission isn't scoped to the source's own abilities — it applies to
/// every equip ability its controller could activate, on any permanent they
/// control (`affected` matches by controller, not by identity). Equip's cost
/// shape is `CostCategory::ManaOnly`, which alone would also cover mana
/// abilities; `keyword: Some("equip")` narrows the match to the `AbilityTag`
/// equip abilities carry (CR 702.6a).
pub(crate) fn parse_equip_activation_timing_permission(
    tp: &TextPair<'_>,
    text: &str,
) -> Option<StaticDefinition> {
    nom_on_lower(tp.original, tp.lower, |i| {
        let (i, _) =
            tag("you may activate equip abilities any time you could cast an instant").parse(i)?;
        let (i, _) = opt(tag(".")).parse(i)?;
        all_consuming(value((), tag(""))).parse(i)
    })?;

    Some(
        StaticDefinition::new(StaticMode::ActivateAsInstant {
            cost_category: CostCategory::ManaOnly,
            keyword: Some(AbilityTag::Equip.keyword_str().to_string()),
        })
        .affected(TargetFilter::Typed(
            TypedFilter::permanent().controller(ControllerRef::You),
        ))
        .description(text.to_string()),
    )
}
