//! The course DSL's **recursive-descent parser**.
//!
//! It produces the same [`CourseSpec`] a programmatic builder produces — that
//! equivalence is the point of the whole layer, and
//! `parse_and_the_builder_agree` pins it directly.
//!
//! # What this language deliberately cannot do
//!
//! There is no way to execute anything. There are no variables, no callbacks, no
//! imports, no reflection, no runtime evaluation and no expressions beyond a
//! literal and a range of two literals. The only repetition is `repeat N { … }`
//! and `alternate N { … }`, both of which are bounded at parse time by
//! [`MAX_REPEAT`] — so a source cannot loop, cannot recurse without bound, and
//! cannot make the compiler do unbounded work. A course source is *data*, and a
//! parser that could be argued into being anything else would make deterministic
//! validation impossible.
//!
//! Every field name is matched against a closed set. An unrecognised one is
//! [`CourseErrorCode::UnknownField`] with a line and a column, not a silently
//! ignored key.

use crate::course::error::{CourseError, CourseErrorCode, CourseResult, SourceLocation};
use crate::course::specification::{
    BankingMode, CountRange, CourseDefaults, CourseItem, CourseSpec, EncounterSpec, LaneWeight,
    MotifInvocation, MotifKind, MotifParams, NearMissWindowSpec, PassingSide, RoadModifierSpec,
    RoadPrimitiveSpec, RollingWallSpec, ScalarRange, SectionGroupSpec, SectionId, SectionKind,
    SectionSpec, SlalomSpec, TrafficFlowSpec, TrafficZoneSpec, TurnDirection, ValidationThresholds,
    VehicleArchetype, ZipperSpec,
};

use super::lexer::{tokenise, Token, TokenKind};

/// The most repetitions a `repeat` or `alternate` block may ask for.
///
/// The bound is enforced at **parse** time rather than at expansion time, so a
/// hostile or mistaken source is refused before it has allocated anything.
pub const MAX_REPEAT: u32 = 32;

/// Parse a course source.
pub fn parse(name: &str, source: &str) -> CourseResult<CourseSpec> {
    let tokens = tokenise(name, source)?;
    let mut parser = Parser {
        tokens: &tokens,
        cursor: 0,
        name: name.to_string(),
    };
    let spec = parser.course()?;
    parser.expect_end()?;
    spec.validate()?;
    Ok(spec)
}

struct Parser<'a> {
    tokens: &'a [Token],
    cursor: usize,
    name: String,
}

impl<'a> Parser<'a> {
    // ---- token plumbing -------------------------------------------------

    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.cursor)
    }

    fn here(&self) -> SourceLocation {
        self.tokens
            .get(self.cursor)
            .or_else(|| self.tokens.last())
            .map(|t| t.at.clone())
            .unwrap_or_else(|| SourceLocation::new(&self.name, 1, 1))
    }

    fn fail(&self, code: CourseErrorCode, message: impl Into<String>) -> CourseError {
        CourseError::new(code, message).at(self.here())
    }

    fn syntax(&self, message: impl Into<String>) -> CourseError {
        self.fail(CourseErrorCode::InvalidSyntax, message)
    }

    fn advance(&mut self) -> CourseResult<&'a Token> {
        let token = self
            .peek()
            .ok_or_else(|| self.syntax("the source ended in the middle of a block"))?;
        self.cursor += 1;
        Ok(token)
    }

    fn expect(&mut self, kind: &TokenKind) -> CourseResult<()> {
        let found = self.advance()?;
        (&found.kind == kind).then_some(()).ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::InvalidSyntax,
                format!("expected {}, found {}", kind.describe(), found.kind.describe()),
            )
            .at(found.at.clone())
        })
    }

    fn expect_end(&self) -> CourseResult<()> {
        self.peek()
            .is_none()
            .then_some(())
            .ok_or_else(|| self.syntax("trailing text after the course block"))
    }

    fn ident(&mut self) -> CourseResult<String> {
        let token = self.advance()?;
        match &token.kind {
            TokenKind::Ident(name) => Ok(name.clone()),
            other => Err(CourseError::new(
                CourseErrorCode::InvalidSyntax,
                format!("expected a name, found {}", other.describe()),
            )
            .at(token.at.clone())),
        }
    }

    fn text(&mut self) -> CourseResult<String> {
        let token = self.advance()?;
        match &token.kind {
            TokenKind::Text(text) => Ok(text.clone()),
            other => Err(CourseError::new(
                CourseErrorCode::InvalidSyntax,
                format!("expected a quoted name, found {}", other.describe()),
            )
            .at(token.at.clone())),
        }
    }

    fn number(&mut self) -> CourseResult<f32> {
        let token = self.advance()?;
        match &token.kind {
            TokenKind::Number { value, .. } => Ok(*value),
            other => Err(CourseError::new(
                CourseErrorCode::InvalidSyntax,
                format!("expected a number, found {}", other.describe()),
            )
            .at(token.at.clone())),
        }
    }

    /// `= <number>`
    fn scalar(&mut self) -> CourseResult<f32> {
        self.expect(&TokenKind::Equals)?;
        self.number()
    }

    /// `= <number>` or `= <number>..<number>`
    fn range(&mut self) -> CourseResult<ScalarRange> {
        self.expect(&TokenKind::Equals)?;
        let lo = self.number()?;
        let matched = matches!(self.peek().map(|t| &t.kind), Some(TokenKind::Range));
        matched.then(|| self.cursor += 1);
        let hi = matched.then(|| self.number()).transpose()?.unwrap_or(lo);
        Ok(ScalarRange::new(lo, hi))
    }

    /// `= <count>` or `= <count>..<count>`
    fn count_range(&mut self) -> CourseResult<CountRange> {
        let range = self.range()?;
        Ok(CountRange::new(
            range.lo.max(0.0) as u32,
            range.hi.max(0.0) as u32,
        ))
    }

    /// `= <count>`
    fn count(&mut self) -> CourseResult<u32> {
        Ok(self.scalar()?.max(0.0) as u32)
    }

    /// `= <signed integer>`
    fn integer(&mut self) -> CourseResult<i32> {
        Ok(self.scalar()? as i32)
    }

    /// `= <word>`
    fn word(&mut self) -> CourseResult<String> {
        self.expect(&TokenKind::Equals)?;
        self.ident()
    }

    /// `= true` / `= false`
    fn boolean(&mut self) -> CourseResult<bool> {
        let at = self.here();
        let word = self.word()?;
        match word.as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            other => Err(CourseError::new(
                CourseErrorCode::InvalidSyntax,
                format!("expected `true` or `false`, found `{other}`"),
            )
            .at(at)),
        }
    }

    /// `= [ a, b, c ]`
    fn integer_list(&mut self) -> CourseResult<Vec<i32>> {
        self.expect(&TokenKind::Equals)?;
        self.expect(&TokenKind::OpenBracket)?;
        let mut values = Vec::new();
        loop {
            match self.peek().map(|t| &t.kind) {
                Some(TokenKind::CloseBracket) => {
                    self.cursor += 1;
                    return Ok(values);
                }
                Some(TokenKind::Comma) => {
                    self.cursor += 1;
                }
                Some(_) => values.push(self.number()? as i32),
                None => return Err(self.syntax("a list was opened and never closed")),
            }
        }
    }

    fn unknown_field(&self, at: SourceLocation, block: &str, field: &str) -> CourseError {
        CourseError::new(
            CourseErrorCode::UnknownField,
            format!("`{field}` is not a field of a `{block}` block"),
        )
        .in_field(field)
        .at(at)
    }

    /// Run `body` for each `<name>` inside a `{ … }` block.
    fn block<F>(&mut self, mut body: F) -> CourseResult<()>
    where
        F: FnMut(&mut Self, String, SourceLocation) -> CourseResult<()>,
    {
        self.expect(&TokenKind::OpenBrace)?;
        loop {
            match self.peek().map(|t| &t.kind) {
                Some(TokenKind::CloseBrace) => {
                    self.cursor += 1;
                    return Ok(());
                }
                Some(TokenKind::Ident(_)) => {
                    let at = self.here();
                    let name = self.ident()?;
                    body(self, name, at)?;
                }
                Some(other) => {
                    return Err(self.syntax(format!(
                        "expected a field name or `}}`, found {}",
                        other.describe()
                    )))
                }
                None => return Err(self.syntax("a block was opened and never closed")),
            }
        }
    }

    // ---- the grammar ----------------------------------------------------

    fn course(&mut self) -> CourseResult<CourseSpec> {
        let at = self.here();
        let keyword = self.ident()?;
        (keyword == "course").then_some(()).ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::InvalidSyntax,
                format!("a course source starts with `course`, not `{keyword}`"),
            )
            .at(at)
        })?;
        let name = self.text()?;
        let mut spec = CourseSpec::new(name, 0);
        self.block(|p, field, at| {
            match field.as_str() {
                "seed" => spec.seed = p.scalar()? as u64,
                "defaults" => spec.defaults = p.defaults()?,
                "thresholds" => spec.thresholds = p.thresholds()?,
                _ => {
                    let items = p.item(&field, at)?;
                    spec.items.extend(items);
                }
            }
            Ok(())
        })?;
        Ok(spec)
    }

    fn defaults(&mut self) -> CourseResult<CourseDefaults> {
        let mut defaults = CourseDefaults::DEFAULT;
        self.block(|p, field, at| {
            match field.as_str() {
                "lanes" => defaults.lanes = p.count()?,
                "lane_width" => defaults.lane_width_m = p.scalar()?,
                "shoulder_width" => defaults.shoulder_width_m = p.scalar()?,
                "expected_speed" => defaults.expected_speed_mps = p.scalar()?,
                "environment" => defaults.environment = p.environment()?,
                other => return Err(p.unknown_field(at, "defaults", other)),
            }
            Ok(())
        })?;
        Ok(defaults)
    }

    fn thresholds(&mut self) -> CourseResult<ValidationThresholds> {
        let mut thresholds = ValidationThresholds::DEFAULT;
        self.block(|p, field, at| {
            match field.as_str() {
                "min_turn_radius" => thresholds.min_turn_radius_m = p.scalar()?,
                "max_grade" => thresholds.max_grade = p.scalar()?,
                "max_bank" => thresholds.max_bank_rad = p.scalar()?,
                "traversal_step" => thresholds.traversal_step_m = p.scalar()?,
                "lateral_speed" => thresholds.lateral_speed_mps = p.scalar()?,
                "lateral_margin" => thresholds.lateral_margin_m = p.scalar()?,
                "min_reaction_time" => thresholds.min_reaction_time_s = p.scalar()?,
                "near_miss_conversion" => thresholds.near_miss_conversion = p.scalar()?,
                "target_boost_duty" => thresholds.target_boost_duty = p.scalar()?,
                "starved_ratio" => thresholds.starved_ratio = p.scalar()?,
                "excellent_ratio" => thresholds.excellent_ratio = p.scalar()?,
                "excellent_route_width" => thresholds.excellent_route_width = p.count()?,
                other => return Err(p.unknown_field(at, "thresholds", other)),
            }
            Ok(())
        })?;
        Ok(thresholds)
    }

    fn environment(&mut self) -> CourseResult<SectionKind> {
        let at = self.here();
        let word = self.word()?;
        SectionKind::parse(&word).ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::UnknownField,
                format!("`{word}` is not an environment this game can draw"),
            )
            .in_field("environment")
            .at(at)
        })
    }

    /// One course-level item: a primitive, a `section` group, a `motif`, or a
    /// bounded `repeat`/`alternate` of any of those.
    fn item(&mut self, keyword: &str, at: SourceLocation) -> CourseResult<Vec<CourseItem>> {
        match keyword {
            "section" => Ok(vec![CourseItem::Group(self.group()?)]),
            "motif" => Ok(vec![CourseItem::Motif(self.motif()?)]),
            "repeat" => self.repetition(false),
            "alternate" => self.repetition(true),
            _ => {
                let section = self.primitive_section(keyword, at)?;
                Ok(vec![CourseItem::Section(section)])
            }
        }
    }

    /// `repeat N { … }` / `alternate N { … }`
    fn repetition(&mut self, alternating: bool) -> CourseResult<Vec<CourseItem>> {
        let at = self.here();
        let count = self.number()? as u32;
        ((count >= 1) & (count <= MAX_REPEAT))
            .then_some(())
            .ok_or_else(|| {
                CourseError::new(
                    CourseErrorCode::RepeatLimitExceeded,
                    format!(
                        "a bounded repeat runs 1..{MAX_REPEAT} times, not {count} — this \
                         grammar has no unbounded loop"
                    ),
                )
                .in_field("repeat")
                .at(at)
            })?;
        let mut template: Vec<CourseItem> = Vec::new();
        self.block(|p, field, at| {
            template.extend(p.item(&field, at)?);
            Ok(())
        })?;
        Ok((0..count)
            .flat_map(|k| {
                template.iter().map(move |item| {
                    let renamed = rename(item, k);
                    (alternating & (k % 2 == 1))
                        .then(|| flip(&renamed))
                        .unwrap_or(renamed)
                })
            })
            .collect())
    }

    /// `section "name" { <primitives> traffic { … } }`
    fn group(&mut self) -> CourseResult<SectionGroupSpec> {
        let name = self.text()?;
        let mut group = SectionGroupSpec::new(SectionId::new(name));
        self.block(|p, field, at| {
            match field.as_str() {
                "lanes" => group.lanes = Some(p.count()?),
                "expected_speed" => group.expected_speed_mps = Some(p.scalar()?),
                "environment" => group.environment = Some(p.environment()?),
                "traffic" => group.traffic = Some(p.traffic()?),
                other => group.parts.push(p.primitive_section(other, at)?),
            }
            Ok(())
        })?;
        Ok(group)
    }

    /// `<primitive> { … }` — one section built from one road primitive.
    fn primitive_section(
        &mut self,
        keyword: &str,
        at: SourceLocation,
    ) -> CourseResult<SectionSpec> {
        let mut id: Option<String> = None;
        let mut length_m = 0.0f32;
        let mut radius_m = 0.0f32;
        let mut height_m = 0.0f32;
        let mut from = 0.0f32;
        let mut to = 0.0f32;
        let mut direction = TurnDirection::Right;
        let mut lanes: Option<u32> = None;
        let mut expected: Option<f32> = None;
        let mut environment: Option<SectionKind> = None;
        let mut modifiers: Vec<RoadModifierSpec> = Vec::new();
        let mut traffic: Option<TrafficZoneSpec> = None;
        let keyword_owned = keyword.to_string();

        self.block(|p, field, field_at| {
            match field.as_str() {
                "id" => {
                    p.expect(&TokenKind::Equals)?;
                    id = Some(p.text()?);
                }
                "length" => length_m = p.scalar()?,
                "radius" => radius_m = p.scalar()?,
                "height" | "depth" => height_m = p.scalar()?,
                "from" => from = p.scalar()?,
                "to" => to = p.scalar()?,
                "direction" | "first" => {
                    let word_at = p.here();
                    let word = p.word()?;
                    direction = TurnDirection::parse(&word).ok_or_else(|| {
                        CourseError::new(
                            CourseErrorCode::InvalidSyntax,
                            format!("`{word}` is not a turn direction — use `left` or `right`"),
                        )
                        .in_field("direction")
                        .at(word_at)
                    })?;
                }
                "lanes" => lanes = Some(p.count()?),
                "expected_speed" => expected = Some(p.scalar()?),
                "environment" => environment = Some(p.environment()?),
                "lateral_wave" => modifiers.push(p.wave(true)?),
                "elevation_wave" => modifiers.push(p.wave(false)?),
                "banking" => modifiers.push(p.banking()?),
                "width_profile" => modifiers.push(p.width_profile()?),
                "lane_profile" => modifiers.push(p.lane_profile()?),
                "grade_profile" => modifiers.push(p.grade_profile()?),
                "traffic" => traffic = Some(p.traffic()?),
                other => return Err(p.unknown_field(field_at, &keyword_owned, other)),
            }
            Ok(())
        })?;

        let primitive = match keyword {
            "straight" => RoadPrimitiveSpec::Straight { length_m },
            "turn" => RoadPrimitiveSpec::Turn {
                length_m,
                radius_m,
                direction,
            },
            "s_bend" => RoadPrimitiveSpec::SBend {
                length_m,
                radius_m,
                first: direction,
            },
            "crest" => RoadPrimitiveSpec::Crest {
                length_m,
                height_m,
            },
            "dip" => RoadPrimitiveSpec::Dip {
                length_m,
                depth_m: height_m,
            },
            "bank_transition" => RoadPrimitiveSpec::BankTransition {
                length_m,
                from_rad: from,
                to_rad: to,
            },
            "lane_transition" => RoadPrimitiveSpec::LaneTransition {
                length_m,
                from_lanes: from.max(0.0) as u32,
                to_lanes: to.max(0.0) as u32,
            },
            "width_transition" => RoadPrimitiveSpec::WidthTransition {
                length_m,
                from_half_width_m: from,
                to_half_width_m: to,
            },
            other => {
                return Err(CourseError::new(
                    CourseErrorCode::UnknownField,
                    format!(
                        "`{other}` is not a road primitive — this grammar knows straight, \
                         turn, s_bend, crest, dip, bank_transition, lane_transition and \
                         width_transition"
                    ),
                )
                .at(at))
            }
        };

        Ok(SectionSpec {
            id: SectionId::new(id.unwrap_or_else(|| keyword.to_string())),
            primitive,
            modifiers,
            lanes,
            expected_speed_mps: expected,
            environment,
            traffic,
        })
    }

    fn wave(&mut self, lateral: bool) -> CourseResult<RoadModifierSpec> {
        let mut amplitude_m = 0.0f32;
        let mut wavelength_m = 1.0f32;
        let mut phase_rad = 0.0f32;
        let block = lateral
            .then_some("lateral_wave")
            .unwrap_or("elevation_wave");
        self.block(|p, field, at| {
            match field.as_str() {
                "amplitude" => amplitude_m = p.scalar()?,
                "wavelength" => wavelength_m = p.scalar()?,
                "phase" => phase_rad = p.scalar()?,
                other => return Err(p.unknown_field(at, block, other)),
            }
            Ok(())
        })?;
        Ok(lateral
            .then_some(RoadModifierSpec::LateralWave {
                amplitude_m,
                wavelength_m,
                phase_rad,
            })
            .unwrap_or(RoadModifierSpec::ElevationWave {
                amplitude_m,
                wavelength_m,
                phase_rad,
            }))
    }

    fn banking(&mut self) -> CourseResult<RoadModifierSpec> {
        let mut mode = BankingMode::FollowCurvature;
        let mut strength = 1.0f32;
        let mut maximum_rad = 0.14f32;
        self.block(|p, field, at| {
            match field.as_str() {
                "mode" => {
                    let word_at = p.here();
                    let word = p.word()?;
                    mode = BankingMode::parse(&word).ok_or_else(|| {
                        CourseError::new(
                            CourseErrorCode::InvalidSyntax,
                            format!(
                                "`{word}` is not a banking mode — use follow_curvature, fixed \
                                 or flat"
                            ),
                        )
                        .in_field("mode")
                        .at(word_at)
                    })?;
                }
                "strength" => strength = p.scalar()?,
                "maximum" => maximum_rad = p.scalar()?,
                other => return Err(p.unknown_field(at, "banking", other)),
            }
            Ok(())
        })?;
        Ok(RoadModifierSpec::Banking {
            mode,
            strength,
            maximum_rad,
        })
    }

    fn width_profile(&mut self) -> CourseResult<RoadModifierSpec> {
        let mut start_half_width_m = 6.0f32;
        let mut end_half_width_m = 6.0f32;
        self.block(|p, field, at| {
            match field.as_str() {
                "from" => start_half_width_m = p.scalar()?,
                "to" => end_half_width_m = p.scalar()?,
                other => return Err(p.unknown_field(at, "width_profile", other)),
            }
            Ok(())
        })?;
        Ok(RoadModifierSpec::WidthProfile {
            start_half_width_m,
            end_half_width_m,
        })
    }

    fn grade_profile(&mut self) -> CourseResult<RoadModifierSpec> {
        let mut drop_m = 0.0f32;
        self.block(|p, field, at| {
            match field.as_str() {
                "drop" => drop_m = p.scalar()?,
                other => return Err(p.unknown_field(at, "grade_profile", other)),
            }
            Ok(())
        })?;
        Ok(RoadModifierSpec::GradeProfile { drop_m })
    }

    fn lane_profile(&mut self) -> CourseResult<RoadModifierSpec> {
        let mut start_lanes = 3u32;
        let mut end_lanes = 3u32;
        self.block(|p, field, at| {
            match field.as_str() {
                "from" => start_lanes = p.count()?,
                "to" => end_lanes = p.count()?,
                other => return Err(p.unknown_field(at, "lane_profile", other)),
            }
            Ok(())
        })?;
        Ok(RoadModifierSpec::LaneProfile {
            start_lanes,
            end_lanes,
        })
    }

    /// `motif <kind> { … }`
    fn motif(&mut self) -> CourseResult<MotifInvocation> {
        let at = self.here();
        let kind_name = self.ident()?;
        let kind = MotifKind::parse(&kind_name).map_err(|e| e.at(at))?;
        let mut params = MotifParams::DEFAULT;
        let mut id: Option<String> = None;
        let mut environment: Option<SectionKind> = None;
        let mut expected: Option<f32> = None;
        let mut traffic: Option<TrafficZoneSpec> = None;
        self.block(|p, field, field_at| {
            match field.as_str() {
                "id" => {
                    p.expect(&TokenKind::Equals)?;
                    id = Some(p.text()?);
                }
                "count" => params.count = p.count()?,
                "length" => params.length_m = p.scalar()?,
                "radius" => params.radius_m = p.range()?,
                "bank" => params.bank_rad = p.range()?,
                "elevation_amplitude" => params.elevation_amplitude_m = p.scalar()?,
                "lateral_amplitude" => params.lateral_amplitude_m = p.scalar()?,
                "wavelength" => params.wavelength_m = p.scalar()?,
                "height" => params.height_m = p.scalar()?,
                "lanes" => params.lanes = p.count_range()?,
                "narrow_lanes" => params.narrow_lanes = p.count()?,
                "environment" => environment = Some(p.environment()?),
                "expected_speed" => expected = Some(p.scalar()?),
                "traffic" => traffic = Some(p.traffic()?),
                other => return Err(p.unknown_field(field_at, "motif", other)),
            }
            Ok(())
        })?;
        Ok(MotifInvocation {
            id: SectionId::new(id.unwrap_or(kind_name)),
            kind,
            params,
            environment,
            expected_speed_mps: expected,
            traffic,
        })
    }

    /// `traffic { flow { … } encounter <kind> { … } near_miss { … } }`
    fn traffic(&mut self) -> CourseResult<TrafficZoneSpec> {
        let mut zone = TrafficZoneSpec::default();
        self.block(|p, field, at| {
            match field.as_str() {
                "flow" => zone.flow = Some(p.flow()?),
                "encounter" => zone.encounters.push(p.encounter()?),
                "near_miss" => zone.near_miss_windows.push(p.near_miss()?),
                other => return Err(p.unknown_field(at, "traffic", other)),
            }
            Ok(())
        })?;
        Ok(zone)
    }

    fn flow(&mut self) -> CourseResult<TrafficFlowSpec> {
        let mut density = 12.0f32;
        let mut headway: Option<ScalarRange> = None;
        let mut min: Option<f32> = None;
        let mut preferred: Option<f32> = None;
        let mut max: Option<f32> = None;
        let mut spec = TrafficFlowSpec::at_density(density);
        let mut lanes: Vec<LaneWeight> = Vec::new();
        let mut archetypes: Vec<(VehicleArchetype, f32)> = Vec::new();
        self.block(|p, field, at| {
            match field.as_str() {
                "vehicles_per_km" => density = p.scalar()?,
                "headway" => headway = Some(p.range()?),
                "min_headway" => min = Some(p.scalar()?),
                "preferred_headway" => preferred = Some(p.scalar()?),
                "max_headway" => max = Some(p.scalar()?),
                "speed" => spec.speed_mps = p.range()?,
                "speed_relative_to_expected" => spec.speed_relative_to_expected = p.scalar()?,
                "platoon_probability" => spec.platoon_probability = p.scalar()?,
                "platoon_size" => spec.platoon_size = p.count_range()?,
                "platoon_gap" => spec.platoon_gap_m = p.scalar()?,
                "burst_length" => spec.burst_length_m = p.scalar()?,
                "recovery_length" => spec.recovery_length_m = p.scalar()?,
                "open_corridor_every" => spec.open_corridor_every_m = p.range()?,
                "open_corridor_length" => spec.open_corridor_length_m = p.scalar()?,
                "lane" => {
                    let lane = p.number()? as i32;
                    let weight = p.scalar()?;
                    lanes.push(LaneWeight { lane, weight });
                }
                "archetype" => {
                    let word_at = p.here();
                    let word = p.ident()?;
                    let archetype = VehicleArchetype::parse(&word).ok_or_else(|| {
                        CourseError::new(
                            CourseErrorCode::UnknownField,
                            format!("`{word}` is not a vehicle archetype"),
                        )
                        .in_field("archetype")
                        .at(word_at)
                    })?;
                    let weight = p.scalar()?;
                    archetypes.push((archetype, weight));
                }
                other => return Err(p.unknown_field(at, "flow", other)),
            }
            Ok(())
        })?;

        let derived = TrafficFlowSpec::at_density(density);
        Ok(TrafficFlowSpec {
            vehicles_per_km: density,
            min_headway_m: min
                .or(headway.map(|h| h.lo))
                .unwrap_or(derived.min_headway_m),
            preferred_headway_m: preferred
                .or(headway.map(|h| h.midpoint()))
                .unwrap_or(derived.preferred_headway_m),
            max_headway_m: max
                .or(headway.map(|h| h.hi))
                .unwrap_or(derived.max_headway_m),
            lane_weights: lanes,
            archetype_weights: archetypes,
            ..spec
        })
    }

    fn encounter(&mut self) -> CourseResult<EncounterSpec> {
        let at = self.here();
        let kind = self.ident()?;
        match kind.as_str() {
            "zipper" => Ok(EncounterSpec::Zipper(self.zipper()?)),
            "rolling_wall" => Ok(EncounterSpec::RollingWall(self.rolling_wall()?)),
            "slalom" => Ok(EncounterSpec::Slalom(self.slalom()?)),
            other => Err(CourseError::new(
                CourseErrorCode::UnknownField,
                format!(
                    "`{other}` is not an encounter — this grammar knows zipper, rolling_wall \
                     and slalom"
                ),
            )
            .in_field("encounter")
            .at(at)),
        }
    }

    fn zipper(&mut self) -> CourseResult<ZipperSpec> {
        let mut spec = ZipperSpec::of_length(240.0);
        self.block(|p, field, at| {
            match field.as_str() {
                "at" => spec.start_offset_m = p.scalar()?,
                "length" => spec.length_m = p.scalar()?,
                "spacing" => spec.spacing_m = p.scalar()?,
                "speed" => spec.speed_mps = p.scalar()?,
                "first_open_lane" => spec.first_open_lane = p.integer()?,
                "alternation" => {
                    let word_at = p.here();
                    let word = p.word()?;
                    spec.alternation = TurnDirection::parse(&word).ok_or_else(|| {
                        CourseError::new(
                            CourseErrorCode::InvalidSyntax,
                            format!("`{word}` is not a direction — use `left` or `right`"),
                        )
                        .in_field("alternation")
                        .at(word_at)
                    })?;
                }
                "minimum_clearance" => spec.lateral_clearance_m = p.scalar()?,
                "target_near_misses" => spec.target_near_misses = p.count()?,
                "minimum_reaction_time" => spec.minimum_reaction_time_s = p.scalar()?,
                "require_continuous_route" => spec.require_continuous_route = p.boolean()?,
                other => return Err(p.unknown_field(at, "zipper", other)),
            }
            Ok(())
        })?;
        Ok(spec)
    }

    fn rolling_wall(&mut self) -> CourseResult<RollingWallSpec> {
        let mut spec = RollingWallSpec::of_phases(3);
        self.block(|p, field, at| {
            match field.as_str() {
                "at" => spec.start_offset_m = p.scalar()?,
                "wall_width" => spec.wall_width_lanes = p.count()?,
                "open_lane" => spec.open_lane = p.integer()?,
                "opening_step" => spec.opening_step_lanes = p.integer()?,
                "phase_length" => spec.phase_length_m = p.scalar()?,
                "phases" => spec.phases = p.count()?,
                "speed" => spec.speed_mps = p.scalar()?,
                "group_spacing" => spec.group_spacing_m = p.scalar()?,
                "reaction_distance" => spec.reaction_distance_m = p.scalar()?,
                other => return Err(p.unknown_field(at, "rolling_wall", other)),
            }
            Ok(())
        })?;
        Ok(spec)
    }

    fn slalom(&mut self) -> CourseResult<SlalomSpec> {
        let mut spec = SlalomSpec::of_blockers(5);
        self.block(|p, field, at| {
            match field.as_str() {
                "at" => spec.start_offset_m = p.scalar()?,
                "blockers" => spec.blockers = p.count()?,
                "spacing" => spec.spacing_m = p.scalar()?,
                "lane_sequence" => spec.lane_sequence = p.integer_list()?,
                "speed" => spec.speed_mps = p.scalar()?,
                "clearance" => spec.clearance_m = p.scalar()?,
                "recovery_gap" => spec.recovery_gap_m = p.scalar()?,
                other => return Err(p.unknown_field(at, "slalom", other)),
            }
            Ok(())
        })?;
        Ok(spec)
    }

    fn near_miss(&mut self) -> CourseResult<NearMissWindowSpec> {
        let mut spec = NearMissWindowSpec {
            start_offset_m: 0.0,
            length_m: 200.0,
            clearance_m: ScalarRange::new(0.4, 1.6),
            side: PassingSide::Either,
            minimum_relative_speed_mps: 8.0,
            intended_opportunities: 1,
            difficulty_weight: 0.6,
        };
        self.block(|p, field, at| {
            match field.as_str() {
                "at" => spec.start_offset_m = p.scalar()?,
                "length" => spec.length_m = p.scalar()?,
                "clearance" => spec.clearance_m = p.range()?,
                "side" => {
                    let word_at = p.here();
                    let word = p.word()?;
                    spec.side = PassingSide::parse(&word).ok_or_else(|| {
                        CourseError::new(
                            CourseErrorCode::InvalidSyntax,
                            format!("`{word}` is not a passing side"),
                        )
                        .in_field("side")
                        .at(word_at)
                    })?;
                }
                "minimum_relative_speed" => spec.minimum_relative_speed_mps = p.scalar()?,
                "opportunities" => spec.intended_opportunities = p.count()?,
                "difficulty" => spec.difficulty_weight = p.scalar()?,
                other => return Err(p.unknown_field(at, "near_miss", other)),
            }
            Ok(())
        })?;
        Ok(spec)
    }
}

/// Give a repeated item its own stable id.
fn rename(item: &CourseItem, repetition: u32) -> CourseItem {
    match item {
        CourseItem::Section(section) => CourseItem::Section(SectionSpec {
            id: section.id.child(repetition),
            ..section.clone()
        }),
        CourseItem::Group(group) => CourseItem::Group(SectionGroupSpec {
            id: group.id.child(repetition),
            ..group.clone()
        }),
        CourseItem::Motif(motif) => CourseItem::Motif(MotifInvocation {
            id: motif.id.child(repetition),
            ..motif.clone()
        }),
    }
}

/// Mirror every turn in an item — what `alternate` does to its odd copies.
fn flip(item: &CourseItem) -> CourseItem {
    match item {
        CourseItem::Section(section) => CourseItem::Section(SectionSpec {
            primitive: flip_primitive(section.primitive),
            ..section.clone()
        }),
        CourseItem::Group(group) => CourseItem::Group(SectionGroupSpec {
            parts: group
                .parts
                .iter()
                .map(|part| SectionSpec {
                    primitive: flip_primitive(part.primitive),
                    ..part.clone()
                })
                .collect(),
            ..group.clone()
        }),
        // A motif already alternates internally; flipping it would undo its own
        // figure, so it is left alone.
        CourseItem::Motif(motif) => CourseItem::Motif(motif.clone()),
    }
}

fn flip_primitive(primitive: RoadPrimitiveSpec) -> RoadPrimitiveSpec {
    match primitive {
        RoadPrimitiveSpec::Turn {
            length_m,
            radius_m,
            direction,
        } => RoadPrimitiveSpec::Turn {
            length_m,
            radius_m,
            direction: direction.flipped(),
        },
        RoadPrimitiveSpec::SBend {
            length_m,
            radius_m,
            first,
        } => RoadPrimitiveSpec::SBend {
            length_m,
            radius_m,
            first: first.flipped(),
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minimal_course_parses() {
        let spec = parse(
            "test.brc",
            r#"
            course "tiny" {
                seed = 7
                straight { id = "opening" length = 500m }
            }
            "#,
        )
        .expect("parses");
        assert_eq!(spec.name, "tiny");
        assert_eq!(spec.seed, 7);
        assert_eq!(spec.items.len(), 1);
        match &spec.items[0] {
            CourseItem::Section(s) => {
                assert_eq!(s.id.as_str(), "opening");
                assert_eq!(s.primitive, RoadPrimitiveSpec::Straight { length_m: 500.0 });
            }
            other => panic!("expected a section, got {other:?}"),
        }
    }

    #[test]
    fn defaults_and_thresholds_parse_with_their_units() {
        let spec = parse(
            "test.brc",
            r#"
            course "units" {
                seed = 1
                defaults {
                    lanes = 3
                    lane_width = 3.8m
                    shoulder_width = 1.5m
                    expected_speed = 180mph
                    environment = tunnel
                }
                thresholds {
                    min_turn_radius = 120m
                    max_grade = 0.16
                    max_bank = 20deg
                    starved_ratio = 1.2
                    excellent_ratio = 2.0
                    excellent_route_width = 3
                    traversal_step = 25m
                    lateral_speed = 11mps
                    lateral_margin = 0.4m
                    min_reaction_time = 0.5s
                    near_miss_conversion = 0.8
                    target_boost_duty = 0.6
                }
                straight { length = 400m }
            }
            "#,
        )
        .expect("parses");
        assert_eq!(spec.defaults.lanes, 3);
        assert!((spec.defaults.lane_width_m - 3.8).abs() < 1.0e-5);
        assert!((spec.defaults.shoulder_width_m - 1.5).abs() < 1.0e-5);
        assert!((spec.defaults.expected_speed_mps - 80.467_2).abs() < 1.0e-2);
        assert_eq!(spec.defaults.environment, SectionKind::Tunnel);
        assert_eq!(spec.thresholds.min_turn_radius_m, 120.0);
        assert!((spec.thresholds.max_grade - 0.16).abs() < 1.0e-6);
        assert!(
            (spec.thresholds.max_bank_rad - 20.0f32.to_radians()).abs() < 1.0e-6,
            "a course authors its own lean: {}",
            spec.thresholds.max_bank_rad
        );
        assert_eq!(spec.thresholds.excellent_route_width, 3);
        assert!((spec.thresholds.min_reaction_time_s - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn every_primitive_and_modifier_has_a_syntax() {
        let spec = parse(
            "test.brc",
            r#"
            course "all" {
                seed = 3
                straight {
                    id = "wavy"
                    length = 700m
                    lateral_wave { amplitude = 8m wavelength = 260m phase = 0rad }
                    elevation_wave { amplitude = 3m wavelength = 180m }
                    banking { mode = follow_curvature strength = 0.8 maximum = 18deg }
                    grade_profile { drop = 12m }
                }
                turn { id = "right" length = 400m radius = 200m direction = right }
                s_bend { id = "ess" length = 400m radius = 220m first = left }
                crest { id = "up" length = 180m height = 6m }
                dip { id = "down" length = 180m depth = 5m }
                bank_transition { id = "roll" length = 200m from = 0deg to = 8deg }
                lane_transition { id = "narrow" length = 140m from = 5 to = 3 }
                width_transition { id = "wide" length = 200m from = 6m to = 9m }
                straight { id = "profiled" length = 200m width_profile { from = 6m to = 8m } }
                straight { id = "laned" length = 200m lane_profile { from = 3 to = 5 } }
            }
            "#,
        )
        .expect("parses");
        assert_eq!(spec.items.len(), 10);
        let primitive = |i: usize| match &spec.items[i] {
            CourseItem::Section(s) => s.primitive,
            other => panic!("expected a section, got {other:?}"),
        };
        assert_eq!(primitive(0).token(), "straight");
        assert_eq!(primitive(1).token(), "turn");
        assert_eq!(primitive(2).token(), "s_bend");
        assert_eq!(primitive(3).token(), "crest");
        assert_eq!(primitive(4).token(), "dip");
        assert_eq!(primitive(5).token(), "bank_transition");
        assert_eq!(primitive(6).token(), "lane_transition");
        assert_eq!(primitive(7).token(), "width_transition");
        match &spec.items[0] {
            CourseItem::Section(s) => {
                assert_eq!(s.modifiers.len(), 4);
                assert!(s
                    .modifiers
                    .iter()
                    .any(|m| matches!(m, RoadModifierSpec::GradeProfile { drop_m } if *drop_m == 12.0)));
                assert!(matches!(
                    s.modifiers[2],
                    RoadModifierSpec::Banking {
                        mode: BankingMode::FollowCurvature,
                        ..
                    }
                ));
            }
            other => panic!("expected a section, got {other:?}"),
        }
        assert_eq!(
            primitive(6),
            RoadPrimitiveSpec::LaneTransition {
                length_m: 140.0,
                from_lanes: 5,
                to_lanes: 3
            }
        );
    }

    #[test]
    fn a_section_group_collects_its_primitives_and_one_traffic_zone() {
        let spec = parse(
            "test.brc",
            r#"
            course "grouped" {
                seed = 2
                section "tunnel_squeeze" {
                    lanes = 3
                    environment = tunnel
                    straight { length = 620m }
                    lane_transition { length = 140m from = 3 to = 3 }
                    traffic {
                        flow { vehicles_per_km = 24 headway = 20m..48m open_corridor_every = 180m..300m }
                        encounter zipper {
                            length = 280m
                            target_near_misses = 6
                            minimum_clearance = 0.55m
                            minimum_reaction_time = 0.75s
                            require_continuous_route = true
                        }
                    }
                }
            }
            "#,
        )
        .expect("parses");
        match &spec.items[0] {
            CourseItem::Group(g) => {
                assert_eq!(g.id.as_str(), "tunnel_squeeze");
                assert_eq!(g.parts.len(), 2);
                assert_eq!(g.lanes, Some(3));
                assert_eq!(g.environment, Some(SectionKind::Tunnel));
                let zone = g.traffic.as_ref().expect("a traffic zone");
                let flow = zone.flow.as_ref().expect("a flow");
                assert_eq!(flow.vehicles_per_km, 24.0);
                assert_eq!(flow.min_headway_m, 20.0);
                assert_eq!(flow.max_headway_m, 48.0);
                assert_eq!(flow.preferred_headway_m, 34.0);
                assert_eq!(flow.open_corridor_every_m, ScalarRange::new(180.0, 300.0));
                assert_eq!(zone.encounters.len(), 1);
                match &zone.encounters[0] {
                    EncounterSpec::Zipper(z) => {
                        assert_eq!(z.length_m, 280.0);
                        assert_eq!(z.target_near_misses, 6);
                        assert!((z.lateral_clearance_m - 0.55).abs() < 1.0e-6);
                        assert!((z.minimum_reaction_time_s - 0.75).abs() < 1.0e-6);
                        assert!(z.require_continuous_route);
                    }
                    other => panic!("expected a zipper, got {other:?}"),
                }
            }
            other => panic!("expected a group, got {other:?}"),
        }
    }

    #[test]
    fn every_encounter_and_the_near_miss_window_have_a_syntax() {
        let spec = parse(
            "test.brc",
            r#"
            course "figures" {
                seed = 4
                straight {
                    id = "run"
                    length = 2000m
                    lanes = 3
                    traffic {
                        encounter rolling_wall {
                            at = 200m
                            wall_width = 2
                            open_lane = 1
                            opening_step = -1
                            phase_length = 160m
                            phases = 3
                            speed = 30mps
                            group_spacing = 160m
                            reaction_distance = 140m
                        }
                        encounter slalom {
                            at = 900m
                            blockers = 4
                            spacing = 70m
                            lane_sequence = [ -1, 1 ]
                            speed = 28mps
                            clearance = 0.9m
                            recovery_gap = 150m
                        }
                        near_miss {
                            at = 300m
                            length = 400m
                            clearance = 0.4m..1.4m
                            side = right
                            minimum_relative_speed = 12mps
                            opportunities = 3
                            difficulty = 0.8
                        }
                    }
                }
            }
            "#,
        )
        .expect("parses");
        let zone = match &spec.items[0] {
            CourseItem::Section(s) => s.traffic.as_ref().expect("a zone"),
            other => panic!("expected a section, got {other:?}"),
        };
        assert_eq!(zone.encounters.len(), 2);
        match &zone.encounters[0] {
            EncounterSpec::RollingWall(w) => {
                assert_eq!(w.phases, 3);
                assert_eq!(w.opening_step_lanes, -1);
                assert_eq!(w.wall_width_lanes, 2);
                assert_eq!(w.start_offset_m, 200.0);
            }
            other => panic!("expected a wall, got {other:?}"),
        }
        match &zone.encounters[1] {
            EncounterSpec::Slalom(s) => {
                assert_eq!(s.lane_sequence, vec![-1, 1]);
                assert_eq!(s.blockers, 4);
            }
            other => panic!("expected a slalom, got {other:?}"),
        }
        let window = &zone.near_miss_windows[0];
        assert_eq!(window.side, PassingSide::Right);
        assert_eq!(window.intended_opportunities, 3);
        assert_eq!(window.clearance_m, ScalarRange::new(0.4, 1.4));
    }

    #[test]
    fn a_motif_parses_with_ranges_and_its_own_traffic() {
        let spec = parse(
            "test.brc",
            r#"
            course "motifs" {
                seed = 84192
                motif high_speed_sweeps {
                    id = "coastal_sweeps"
                    count = 4
                    length = 1400m
                    radius = 90m..150m
                    bank = 8deg..16deg
                    lanes = 5
                    environment = sweeping_bends
                    expected_speed = 180mph
                    traffic { flow { vehicles_per_km = 14 } }
                }
            }
            "#,
        )
        .expect("parses");
        match &spec.items[0] {
            CourseItem::Motif(m) => {
                assert_eq!(m.id.as_str(), "coastal_sweeps");
                assert_eq!(m.kind, MotifKind::HighSpeedSweeps);
                assert_eq!(m.params.count, 4);
                assert_eq!(m.params.radius_m, ScalarRange::new(90.0, 150.0));
                assert!((m.params.bank_rad.lo - 8.0f32.to_radians()).abs() < 1.0e-6);
                assert_eq!(m.environment, Some(SectionKind::SweepingBends));
                assert!(m.traffic.is_some());
            }
            other => panic!("expected a motif, got {other:?}"),
        }
    }

    #[test]
    fn a_bounded_repeat_produces_distinct_stable_ids() {
        let spec = parse(
            "test.brc",
            r#"
            course "repeated" {
                seed = 1
                repeat 3 {
                    turn { id = "sweep" length = 300m radius = 200m direction = right }
                    straight { id = "link" length = 100m }
                }
            }
            "#,
        )
        .expect("parses");
        let names: Vec<String> = spec
            .items
            .iter()
            .map(|item| match item {
                CourseItem::Section(s) => s.id.to_string(),
                other => panic!("expected sections, got {other:?}"),
            })
            .collect();
        assert_eq!(
            names,
            vec!["sweep/0", "link/0", "sweep/1", "link/1", "sweep/2", "link/2"]
        );
        assert!(spec.validate().is_ok());
    }

    #[test]
    fn alternation_mirrors_every_other_copy() {
        let spec = parse(
            "test.brc",
            r#"
            course "alternating" {
                seed = 1
                alternate 4 {
                    turn { id = "bend" length = 300m radius = 200m direction = right }
                }
            }
            "#,
        )
        .expect("parses");
        let directions: Vec<TurnDirection> = spec
            .items
            .iter()
            .map(|item| match item {
                CourseItem::Section(s) => match s.primitive {
                    RoadPrimitiveSpec::Turn { direction, .. } => direction,
                    ref other => panic!("expected a turn, got {other:?}"),
                },
                other => panic!("expected a section, got {other:?}"),
            })
            .collect();
        assert_eq!(
            directions,
            vec![
                TurnDirection::Right,
                TurnDirection::Left,
                TurnDirection::Right,
                TurnDirection::Left
            ]
        );
    }

    #[test]
    fn a_repeat_above_the_bound_is_refused_at_parse_time() {
        let err = parse(
            "test.brc",
            &format!(
                "course \"x\" {{ seed = 1 repeat {} {{ straight {{ length = 10m }} }} }}",
                MAX_REPEAT + 1
            ),
        )
        .unwrap_err();
        assert_eq!(err.code, CourseErrorCode::RepeatLimitExceeded);
        assert!(err.at.is_some(), "and it points at the source");
        assert!(parse(
            "test.brc",
            "course \"x\" { seed = 1 repeat 0 { straight { length = 10m } } }"
        )
        .is_err());
    }

    #[test]
    fn an_unknown_field_is_rejected_with_a_line_and_a_column() {
        let err = parse(
            "course.brc",
            "course \"x\" {\n  seed = 1\n  straight { length = 100m wobbliness = 3 }\n}",
        )
        .unwrap_err();
        assert_eq!(err.code, CourseErrorCode::UnknownField);
        assert_eq!(err.field.as_deref(), Some("wobbliness"));
        let at = err.at.expect("a location");
        assert_eq!(at.line, 3);
        assert_eq!(at.column, 28);
        assert_eq!(at.source, "course.brc");
    }

    #[test]
    fn an_unknown_motif_primitive_encounter_or_environment_is_rejected() {
        let cases = [
            (
                "course \"x\" { seed = 1 motif figure_eight { } straight { length = 10m } }",
                CourseErrorCode::UnknownMotif,
            ),
            (
                "course \"x\" { seed = 1 loop_the_loop { length = 10m } }",
                CourseErrorCode::UnknownField,
            ),
            (
                "course \"x\" { seed = 1 straight { length = 10m traffic { encounter chicane { } } } }",
                CourseErrorCode::UnknownField,
            ),
            (
                "course \"x\" { seed = 1 straight { length = 10m environment = swamp } }",
                CourseErrorCode::UnknownField,
            ),
        ];
        for (source, code) in cases {
            let err = parse("test.brc", source).unwrap_err();
            assert_eq!(err.code, code, "{source}");
            assert!(err.at.is_some(), "{source} produced no location");
        }
    }

    #[test]
    fn syntax_errors_carry_a_line_and_a_column() {
        for source in [
            "course \"x\" { seed = 1 straight { length = }",
            "course \"x\" { seed = 1 straight { length 100m } }",
            "cursed \"x\" { }",
            "course x { }",
            "course \"x\" { seed = 1 straight { length = 10m } } extra",
            "course \"x\" { seed = 1 straight { length = 10m }",
            "course \"x\" { seed = 1 straight { 5 } }",
        ] {
            let err = parse("test.brc", source).unwrap_err();
            assert_eq!(err.code, CourseErrorCode::InvalidSyntax, "{source}");
            assert!(err.at.is_some(), "{source} produced no location");
        }
    }

    #[test]
    fn invalid_units_and_enum_words_are_rejected() {
        assert_eq!(
            parse("test.brc", "course \"x\" { seed = 1 straight { length = 4parsecs } }")
                .unwrap_err()
                .code,
            CourseErrorCode::InvalidUnit
        );
        for source in [
            "course \"x\" { seed = 1 turn { length = 10m radius = 5m direction = sideways } }",
            "course \"x\" { seed = 1 straight { length = 10m banking { mode = wobble } } }",
            "course \"x\" { seed = 1 straight { length = 10m traffic { near_miss { side = upward } } } }",
            "course \"x\" { seed = 1 straight { length = 10m traffic { encounter zipper { require_continuous_route = maybe } } } }",
        ] {
            assert!(parse("test.brc", source).is_err(), "{source}");
        }
    }

    #[test]
    fn a_duplicate_identifier_is_rejected_by_the_specification_it_produces() {
        // The parser hands back a spec; `CourseSpec::validate` and the compiler's
        // expansion are what own uniqueness, and `parse` runs the first of them.
        let spec = parse(
            "test.brc",
            r#"
            course "clash" {
                seed = 1
                straight { id = "same" length = 200m }
                straight { id = "same" length = 200m }
            }
            "#,
        )
        .expect("parses — duplicate ids are an expansion failure, not a syntax one");
        let err = crate::course::compiler::expand(&spec).unwrap_err();
        assert_eq!(err.code, CourseErrorCode::DuplicateIdentifier);
    }

    #[test]
    fn lane_and_archetype_weights_parse() {
        let spec = parse(
            "test.brc",
            r#"
            course "weighted" {
                seed = 1
                straight {
                    length = 1000m
                    traffic {
                        flow {
                            vehicles_per_km = 20
                            lane -1 = 1.0
                            lane 0 = 3.0
                            lane 1 = 1.0
                            archetype van = 2.0
                            archetype saloon = 1.0
                        }
                    }
                }
            }
            "#,
        )
        .expect("parses");
        let flow = match &spec.items[0] {
            CourseItem::Section(s) => s.traffic.as_ref().unwrap().flow.as_ref().unwrap(),
            other => panic!("expected a section, got {other:?}"),
        };
        assert_eq!(flow.lane_weights.len(), 3);
        assert_eq!(flow.lane_weights[1].lane, 0);
        assert_eq!(flow.lane_weights[1].weight, 3.0);
        assert_eq!(flow.archetype_weights.len(), 2);
        assert_eq!(flow.archetype_weights[0].0, VehicleArchetype::Van);
    }

    #[test]
    fn the_grammar_offers_no_way_to_run_anything() {
        // Every shape a "real" language would need is a syntax error here.
        for hostile in [
            "course \"x\" { seed = 1 let a = 5 }",
            "course \"x\" { seed = 1 import \"other.brc\" }",
            "course \"x\" { seed = 1 while true { } }",
            "course \"x\" { seed = 1 fn go() { } }",
            "course \"x\" { seed = 1 straight { length = 2 * 3 } }",
            "course \"x\" { seed = 1 straight { length = eval(\"9\") } }",
        ] {
            assert!(parse("test.brc", hostile).is_err(), "{hostile} was accepted");
        }
    }
}
