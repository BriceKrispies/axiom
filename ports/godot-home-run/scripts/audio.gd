# Audio — a tiny WebAudio-style cue synth: a pool of AudioStreamPlayers fed
# runtime-generated AudioStreamWAV tones. play() maps a gameplay feedback kind to a
# short chord/blip, mirroring the original game's tone table.
extends Node

var _players: Array[AudioStreamPlayer] = []
var _next := 0

func _ready() -> void:
	for i in range(12):
		var p := AudioStreamPlayer.new()
		add_child(p)
		_players.append(p)

func _free_player() -> AudioStreamPlayer:
	var p := _players[_next]
	_next = (_next + 1) % _players.size()
	return p

func _make_wav(freq: float, dur: float, vol: float, wave: String) -> AudioStreamWAV:
	var sr := 22050
	var n := int(dur * sr)
	var bytes := PackedByteArray()
	bytes.resize(n * 2)
	for i in range(n):
		var phase := freq * (float(i) / sr)
		var frac: float = phase - floor(phase)
		var s := 0.0
		match wave:
			"square":
				s = 1.0 if frac < 0.5 else -1.0
			"triangle":
				s = 4.0 * absf(frac - 0.5) - 1.0
			"sawtooth":
				s = 2.0 * frac - 1.0
			_:
				s = sin(TAU * phase)
		var env := minf(1.0, minf(float(i) / 220.0, float(n - i) / 220.0))
		bytes.encode_s16(i * 2, int(clampf(s * vol * env, -1.0, 1.0) * 32767.0))
	var w := AudioStreamWAV.new()
	w.format = AudioStreamWAV.FORMAT_16_BITS
	w.mix_rate = sr
	w.stereo = false
	w.data = bytes
	return w

func _tone(freq: float, dur: float, vol: float, wave: String, delay: float) -> void:
	if delay > 0.0:
		await get_tree().create_timer(delay).timeout
	var p := _free_player()
	p.stream = _make_wav(freq, dur, vol, wave)
	p.play()

func play(kind: String, big: bool) -> void:
	match kind:
		"release":
			_tone(660, 0.05, 0.12, "square", 0.0)
		"contact":
			_tone(220 if big else 180, 0.07, 0.5, "square", 0.0)
			_tone(1400 if big else 900, 0.05, 0.25, "triangle", 0.0)
		"homer":
			var arp := [523, 659, 784, 1047]
			for i in range(arp.size()):
				_tone(arp[i], 0.16, 0.3, "triangle", i * 0.05)
		"clean":
			_tone(587, 0.12, 0.22, "triangle", 0.0)
		"miss":
			_tone(110, 0.12, 0.18, "sawtooth", 0.0)
		"ball":
			_tone(300, 0.1, 0.12, "sine", 0.0)
		"foul":
			_tone(240, 0.08, 0.18, "square", 0.0)
		"caught", "fielded", "weak", "grounder", "popup":
			_tone(160, 0.08, 0.2, "sine", 0.0)
		"cinematicAnticipation":
			_tone(200, 0.1, 0.14, "sine", 0.0)
			_tone(320, 0.14, 0.16, "sine", 0.06)
		"crowdErupt":
			_tone(140, 0.22, 0.2, "sawtooth", 0.0)
			_tone(210, 0.18, 0.16, "triangle", 0.05)

func ready_click() -> void:
	_tone(880, 0.05, 0.14, "sine", 0.0)
