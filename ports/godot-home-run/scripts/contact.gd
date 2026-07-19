# Contact — the resolved outcome of one bat-ball touch. A pure value object
# produced by Swing.swept_contact / Swing.resolve_contact and consumed by the
# swing-outcome prediction. Behaviour lives on Swing; this is just the record.
extends RefCounted

var r: float          # contact radius along the bat (grip -> tip), world units from pivot
var u: float          # normalized position along the hittable segment (0 handle .. 1 tip)
var sweet_q: float
var timing_q: float
var vert_q: float
var quality: float    # blended contact quality, 0..1
var exit_vel: Vector3 # world units per TICK
var exit_speed: float # u/s (pre-loft horizontal reference)
var spray: float      # horizontal spray angle (0 = dead center, |>45deg| = foul)
var loft: float       # launch loft, radians
var point: Vector3    # world contact point
