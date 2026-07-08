/*
 * The Signal Runner colour palette — the illustrated, flat-shaded, pale
 * snowy/stone look of the reference. Colours are the SDK's `Rgba` (each channel in
 * [0, 1]); `hex` unpacks a `0xRRGGBB` literal so the constants read like a design
 * swatch sheet. Alpha defaults to 1; `alpha()` makes a translucent variant.
 */

import type { Rgba } from "@axiom/game";

/** An opaque colour from a `0xRRGGBB` literal. */
export const hex = (rgb: number, a = 1): Rgba => [
  ((rgb >> 16) & 0xff) / 255,
  ((rgb >> 8) & 0xff) / 255,
  (rgb & 0xff) / 255,
  a,
];

/** A translucent variant of `color`. */
export const alpha = (color: Rgba, a: number): Rgba => [color[0], color[1], color[2], a];

export const SKY_TOP = hex(0xe9_ea_f1);
export const SKY_BOTTOM = hex(0xdf_e1_ec);
export const GROUND = hex(0xe4_e6_ee);

export const OUTLINE = hex(0x33_36_40);
export const OUTLINE_SOFT = hex(0x54_59_66);

export const MOUNTAIN = [hex(0x9a_a0_b8), hex(0xb2_b7_cc), hex(0xc7_cb_dc)] as const;
export const MOUNTAIN_SNOW = hex(0xef_f1_f7);

export const PATH = hex(0xf4_f0_e6);
export const PATH_BAND = hex(0xe9_e4_d5);
export const PATH_EDGE = hex(0x3a_3d_46);

export const TREE = [hex(0x53_74_5a), hex(0x46_65_4e)] as const;
export const TREE_TRUNK = hex(0x6f_5a_45);
export const ROCK = hex(0x9a_a0_ad);
export const ROCK_DARK = hex(0x80_86_95);
export const RUIN = hex(0xa9_ad_b9);
export const RUIN_DARK = hex(0x8b_90_9e);

export const SHARD = hex(0x54_c4_e6);
export const SHARD_CORE = hex(0xc6_ee_f7);
export const SHARD_EDGE = hex(0x2b_6f_8c);

export const PLATE = hex(0xf1_c5_3c);
export const PLATE_EDGE = hex(0x8f_6c_12);

export const STORM = hex(0x9a_4f_d0);
export const STORM_DARK = hex(0x7a_3a_af);
export const STORM_BOLT = hex(0xe7_c7_f3);

export const DRONE_BODY = hex(0x7c_82_90);
export const DRONE_CORE = hex(0xd0_45_3f);

export const HOOD = hex(0xd0_43_3c);
export const HOOD_DARK = hex(0xa8_32_2c);
export const CAPE = hex(0xd8_4a_42);
export const CAPE_GLYPH = hex(0xe8_b4_3a);
export const SKIN = hex(0x2f_33_3b);
export const SLED = hex(0xd2_d8_e0);
export const SLED_GLOW = hex(0x6f_d3_ec);
export const SKID = hex(0x8f_9a_ad);

export const PANEL = hex(0xff_ff_ff);
export const PANEL_INK = hex(0x2f_33_3b);
export const PANEL_MUTE = hex(0x6b_72_80);
export const PANEL_EDGE = hex(0x2f_33_3b);
export const READY = hex(0x2f_9e_57);
export const SEG_ON = hex(0x6b_72_80);
export const SEG_OFF = hex(0xd7_da_e1);
