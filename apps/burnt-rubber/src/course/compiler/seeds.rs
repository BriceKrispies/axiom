//! **Seed partitioning**: one course seed, six independent streams, and a
//! per-section stream inside each.
//!
//! A single mutable random stream shared by the whole compiler is the classic
//! procedural-generation trap, and it is worth naming precisely. If geometry,
//! scenery and traffic all draw from one stream in sequence, then adding a
//! single scenery prop in section 2 shifts every draw after it — the traffic in
//! section 3 changes lanes, the bends in section 7 change radius, and a change
//! the author believes is cosmetic re-rolls the entire rest of the course. Every
//! tuning pass then has to re-verify the whole course.
//!
//! So: the course seed is split by **subsystem** ([`SeedDomain`]) and again by
//! **stable section id**. Two consequences the tests pin directly:
//!
//! * changing the scenery seed cannot move the road or the traffic;
//! * adding a vehicle in one section cannot change any earlier section.
//!
//! Every derivation is a pure function of `(course_seed, domain, section id)`,
//! via `Draw::fork` (splitmix64 avalanche) and the kernel's
//! [`StableHash`](axiom_kernel::StableHash) over the section name. Nothing is
//! positional, so inserting a section before another one leaves the second one's
//! stream exactly where it was.

use axiom_kernel::StableHash;

use crate::course::specification::SectionId;
use crate::draw::Draw;

/// An independent generator stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SeedDomain {
    /// The road's shape: bends, hills, banking, width.
    Geometry,
    /// Motif expansion: how many sections, how long, which way round.
    Motif,
    /// Ambient traffic placement.
    TrafficFlow,
    /// Authored encounter variation.
    TrafficEncounter,
    /// Roadside props.
    Scenery,
    /// Which shape a traffic car is drawn as.
    Cosmetic,
}

impl SeedDomain {
    /// Every domain, in a stable order.
    pub const ALL: [SeedDomain; 6] = [
        SeedDomain::Geometry,
        SeedDomain::Motif,
        SeedDomain::TrafficFlow,
        SeedDomain::TrafficEncounter,
        SeedDomain::Scenery,
        SeedDomain::Cosmetic,
    ];

    /// The domain's name, used in dumps and as the hashed salt.
    pub const fn name(self) -> &'static str {
        match self {
            SeedDomain::Geometry => "geometry",
            SeedDomain::Motif => "motif",
            SeedDomain::TrafficFlow => "traffic-flow",
            SeedDomain::TrafficEncounter => "traffic-encounter",
            SeedDomain::Scenery => "scenery",
            SeedDomain::Cosmetic => "cosmetic",
        }
    }

    /// The domain's fixed salt: a stable digest of its name, so the constant is
    /// derived from something meaningful rather than being a magic number
    /// somebody has to keep unique by hand.
    pub fn salt(self) -> u64 {
        StableHash::of_bytes(self.name().as_bytes()).raw()
    }
}

/// The seed for a whole subsystem on this course.
pub fn domain_seed(course_seed: u64, domain: SeedDomain) -> u64 {
    Draw::seeded(course_seed).fork(domain.salt()).seed()
}

/// The seed for one section's slice of one subsystem.
///
/// Derived from the section's **stable name**, never its index: inserting a
/// section ahead of this one must not change what this one generates.
pub fn section_seed(course_seed: u64, section: &SectionId, domain: SeedDomain) -> u64 {
    let name = StableHash::of_bytes(section.as_str().as_bytes()).raw();
    Draw::seeded(domain_seed(course_seed, domain))
        .fork(name)
        .seed()
}

/// A drawer on a whole subsystem's stream.
pub fn domain_draw(course_seed: u64, domain: SeedDomain) -> Draw {
    Draw::seeded(domain_seed(course_seed, domain))
}

/// A drawer on one section's slice of one subsystem's stream.
pub fn section_draw(course_seed: u64, section: &SectionId, domain: SeedDomain) -> Draw {
    Draw::seeded(section_seed(course_seed, section, domain))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(mut draw: Draw) -> Vec<u64> {
        (0..16).map(|_| draw.next_u64()).collect()
    }

    #[test]
    fn every_domain_has_a_distinct_name_and_salt() {
        let mut names: Vec<&str> = SeedDomain::ALL.iter().map(|d| d.name()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count);

        let mut salts: Vec<u64> = SeedDomain::ALL.iter().map(|d| d.salt()).collect();
        salts.sort_unstable();
        let count = salts.len();
        salts.dedup();
        assert_eq!(salts.len(), count, "two domains share a salt");
    }

    #[test]
    fn the_domains_of_one_course_are_independent_streams() {
        let seed = 0x0B17_4E7A_5C09_1D33u64;
        let streams: Vec<Vec<u64>> = SeedDomain::ALL
            .iter()
            .map(|d| take(domain_draw(seed, *d)))
            .collect();
        for (i, a) in streams.iter().enumerate() {
            for b in streams.iter().skip(i + 1) {
                assert_ne!(a, b, "two domains produced the same stream");
            }
        }
    }

    #[test]
    fn a_domain_stream_is_a_pure_function_of_the_course_seed() {
        for domain in SeedDomain::ALL {
            assert_eq!(domain_seed(7, domain), domain_seed(7, domain));
            assert_ne!(domain_seed(7, domain), domain_seed(8, domain));
        }
    }

    #[test]
    fn a_section_stream_is_anchored_on_the_name_not_the_position() {
        let seed = 99;
        let a = SectionId::new("coastal_sweeps/2");
        let b = SectionId::new("coastal_sweeps/3");
        assert_eq!(
            section_seed(seed, &a, SeedDomain::Geometry),
            section_seed(seed, &a, SeedDomain::Geometry),
            "the same name derives the same stream"
        );
        assert_ne!(
            section_seed(seed, &a, SeedDomain::Geometry),
            section_seed(seed, &b, SeedDomain::Geometry),
            "different names derive different streams"
        );
        assert_ne!(
            section_seed(seed, &a, SeedDomain::Geometry),
            section_seed(seed, &a, SeedDomain::TrafficFlow),
            "and the same name in two domains is two streams"
        );
    }

    /// **The property the whole partition exists for.** Changing what the
    /// scenery is seeded from cannot move the road or the traffic, because they
    /// are not downstream of it.
    #[test]
    fn a_sections_streams_are_independent_of_each_other() {
        let seed = 4242;
        let id = SectionId::new("tunnel_squeeze/0");
        let geometry = take(section_draw(seed, &id, SeedDomain::Geometry));
        let traffic = take(section_draw(seed, &id, SeedDomain::TrafficFlow));
        let scenery = take(section_draw(seed, &id, SeedDomain::Scenery));
        assert_ne!(geometry, traffic);
        assert_ne!(geometry, scenery);
        assert_ne!(traffic, scenery);

        // Advancing one stream a long way cannot reach any other: they are
        // derived from the seed, not from each other's position.
        let mut advanced = section_draw(seed, &id, SeedDomain::Scenery);
        (0..10_000).for_each(|_| {
            advanced.next_u64();
        });
        assert_eq!(
            take(section_draw(seed, &id, SeedDomain::Geometry)),
            geometry
        );
        assert_eq!(take(section_draw(seed, &id, SeedDomain::TrafficFlow)), traffic);
    }
}
