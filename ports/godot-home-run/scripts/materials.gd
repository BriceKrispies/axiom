# materials.gd — the declared material palette, ported from game.ts's MATERIALS.
# Each is a StandardMaterial3D tuned for the original's flat, Lambert-ish toy look:
# unshaded-adjacent (roughness 1, zero specular/metallic), emissive "glow" markings
# self-lit so they read at grazing angles, and a couple of translucent effect mats.

static func _base(color: Color) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.albedo_color = color
	m.roughness = 1.0
	m.metallic = 0.0
	m.specular_mode = BaseMaterial3D.SPECULAR_DISABLED
	m.diffuse_mode = BaseMaterial3D.DIFFUSE_LAMBERT
	return m

static func _flat(r: float, g: float, b: float) -> StandardMaterial3D:
	return _base(Color(r, g, b))

static func _glow(r: float, g: float, b: float, er: float, eg: float, eb: float) -> StandardMaterial3D:
	var m := _base(Color(r, g, b))
	m.emission_enabled = true
	m.emission = Color(er, eg, eb)
	m.emission_energy_multiplier = 1.0
	return m

static func _trans(r: float, g: float, b: float, opacity: float) -> StandardMaterial3D:
	var m := _base(Color(r, g, b, opacity))
	m.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	return m

static func _glow_trans(r: float, g: float, b: float, er: float, eg: float, eb: float, opacity: float) -> StandardMaterial3D:
	var m := _glow(r, g, b, er, eg, eb)
	m.albedo_color = Color(r, g, b, opacity)
	m.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	return m

static func build() -> Dictionary:
	return {
		"BallWhite": _flat(1, 1, 0.98),
		"BaseWhite": _glow(1, 1, 0.98, 0.3, 0.3, 0.28),
		"BatKnob": _flat(0.55, 0.4, 0.16),
		"BatterBlue": _flat(0.22, 0.46, 1),
		"BatterHelmet": _flat(0.14, 0.3, 0.85),
		"BatterPuck": _flat(0.55, 0.85, 1),
		"CornerBlue": _flat(0.24, 0.3, 0.8),
		"DeckBrown": _flat(0.72, 0.5, 0.3),
		"Dirt": _flat(0.82, 0.58, 0.34),
		"DirtLight": _flat(0.95, 0.72, 0.44),
		"DotBlue": _flat(0.2, 0.35, 0.95),
		"DotRed": _flat(0.9, 0.15, 0.12),
		"DotYellow": _flat(0.95, 0.8, 0.15),
		"FielderBase": _flat(1, 0.6, 0.3),
		"FielderCap": _flat(1, 0.22, 0.18),
		"FielderWhite": _flat(1, 0.98, 0.95),
		"GrassDark": _flat(0.4, 0.82, 0.24),
		"GrassLight": _flat(0.55, 1, 0.34),
		"GroundGreen": _flat(0.38, 0.7, 0.24),
		"Line": _glow(1, 1, 0.96, 0.3, 0.3, 0.28),
		"MachineDark": _flat(0.3, 0.3, 0.36),
		"MachineOrange": _flat(1, 0.6, 0.34),
		"PanelNavy": _flat(0.1, 0.13, 0.38),
		"PatrolDirt": _flat(0.68, 0.46, 0.26),
		"PatrolGreen": _flat(0.36, 0.72, 0.22),
		"SeatBlue": _flat(0.42, 0.54, 1),
		"SeatBlueDark": _flat(0.3, 0.39, 0.92),
		"SkyBowl": _glow(0.72, 0.76, 1, 0.5, 0.56, 0.8),
		"WallBlue": _flat(0.32, 0.44, 1),
		"WallTrim": _flat(1, 0.68, 0.16),
		"bat": _glow(1, 0.88, 0.25, 0.45, 0.36, 0.08),
		"digit": _glow(1, 0.3, 0.15, 0.9, 0.2, 0.08),
		"flash": _glow(1, 0.95, 0.6, 1, 0.85, 0.4),
		"impact": _glow_trans(1, 0.9, 0.5, 1, 0.8, 0.35, 0.55),
		"shadow": _trans(0.05, 0.12, 0.05, 0.35),
		"trail": _glow(1, 0.9, 0.6, 1, 0.75, 0.35),
	}
