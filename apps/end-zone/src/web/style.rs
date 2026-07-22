//! The procedural arcade stylesheet: every bevel, chrome gradient, glow,
//! and squash animation as one injected `<style>` constant.
//! No image files anywhere — gradients, clip-paths, shadows, and keyframes
//! only. Palette values arrive as CSS variables set from the typed theme.

/// The complete menu + HUD stylesheet.
pub const MENU_CSS: &str = r#"
#end-zone-menu,#end-zone-hud{position:fixed;inset:0;overflow:hidden;z-index:40;
  pointer-events:none;color:var(--ez-text);
  font-family:system-ui,'Segoe UI','Helvetica Neue',Arial,sans-serif;
  -webkit-font-smoothing:antialiased;text-rendering:optimizeLegibility;}
#end-zone-hud{z-index:38;}
#end-zone-menu *,#end-zone-hud *{box-sizing:border-box;margin:0;padding:0;}
.ez-widget{position:absolute;display:flex;align-items:center;justify-content:center;}
.ez-widgetfill{width:100%;height:100%;display:flex;flex-direction:column;
  align-items:center;justify-content:center;}
.ez-dim{position:absolute;inset:0;}
.ez-vig{position:absolute;inset:0;box-shadow:inset 0 0 160px 40px rgba(0,0,0,.65);}

/* --- title logo ------------------------------------------------------- */
.ez-logo{flex-direction:column;gap:14px;}
.ez-logo .ez-mark{font-weight:900;letter-spacing:.04em;line-height:.9;
  text-align:center;text-transform:uppercase;
  background:linear-gradient(180deg,#f4f7fb 0%,#c7cfda 34%,#5f6c7d 50%,#e6ebf2 62%,#8a97a8 100%);
  -webkit-background-clip:text;background-clip:text;color:transparent;
  filter:drop-shadow(0 4px 0 #0b1524) drop-shadow(0 10px 22px rgba(0,0,0,.8))
         drop-shadow(0 0 26px var(--ez-electric));}
.ez-logo.ez-big .ez-mark{font-size:min(15vw,128px);}
.ez-logo.ez-small .ez-mark{font-size:44px;}
.ez-logo .ez-sub{font-size:14px;font-weight:700;letter-spacing:.55em;color:var(--ez-chrome);
  text-shadow:0 2px 6px #000;}
.ez-press{font-size:22px;font-weight:800;letter-spacing:.36em;
  color:var(--ez-volt);text-shadow:0 0 14px var(--ez-volt),0 2px 4px #000;}
.ez-anim .ez-press{animation:ezblink 1.1s step-end infinite;}
@keyframes ezblink{0%,60%{opacity:1}61%,100%{opacity:.12}}

/* --- labels ------------------------------------------------------------ */
.ez-label{font-weight:800;text-transform:uppercase;text-align:center;
  text-shadow:0 2px 4px rgba(0,0,0,.85);}
/* headings: upright, generously tracked (no slant) */
.ez-label.ez-italic{letter-spacing:.16em;}
.ez-huge{font-size:48px;font-weight:900;letter-spacing:.08em;}
.ez-heading{font-size:26px;letter-spacing:.18em;}
.ez-body{font-size:17px;letter-spacing:.06em;}
.ez-smalltext{font-size:12px;letter-spacing:.16em;color:var(--ez-textdim);}

/* --- buttons ------------------------------------------------------------ */
.ez-btn{width:100%;height:100%;display:flex;align-items:center;justify-content:center;
  font-weight:800;text-transform:uppercase;
  font-size:20px;letter-spacing:.14em;color:var(--ez-text);
  border:3px solid #06090e;border-radius:6px;position:relative;
  background:linear-gradient(180deg,var(--ez-steel-l) 0%,var(--ez-steel-d) 55%,#0d1118 100%);
  box-shadow:inset 0 2px 0 rgba(255,255,255,.28),inset 0 -3px 0 rgba(0,0,0,.55),
    0 6px 14px rgba(0,0,0,.6);text-shadow:0 2px 3px rgba(0,0,0,.9);}
/* .ez-angled: kept as a rectangular no-op (buttons are no longer slanted). */
.ez-btn.ez-primary{background:linear-gradient(180deg,#3f8dff 0%,var(--ez-electric) 45%,#0c2f63 100%);}
.ez-btn.ez-danger{background:linear-gradient(180deg,#ff6a55 0%,var(--ez-hot) 45%,#5c150d 100%);}
.ez-btn.ez-focused{outline:0;
  box-shadow:0 0 0 3px var(--ez-focus),0 0 26px var(--ez-focus),
    inset 0 2px 0 rgba(255,255,255,.35),0 6px 14px rgba(0,0,0,.6);}
.ez-disabled{opacity:.38;filter:saturate(.4);}
.ez-pressanim{animation:ezpress .14s ease-out;}
@keyframes ezpress{0%{transform:scale(1)}35%{transform:scale(.94,.88)}100%{transform:scale(1)}}

/* --- panels ------------------------------------------------------------- */
.ez-plate{background:linear-gradient(180deg,var(--ez-steel-l),var(--ez-steel-d));
  border:3px solid #06090e;border-radius:8px;
  box-shadow:inset 0 2px 0 rgba(255,255,255,.22),0 10px 24px rgba(0,0,0,.65);}

/* --- settings / control rows -------------------------------------------- */
.ez-row{width:100%;height:100%;display:flex;align-items:center;gap:12px;padding:0 18px;
  background:linear-gradient(180deg,var(--ez-steel-l),var(--ez-steel-d));
  border:2px solid #06090e;border-radius:6px;
  box-shadow:inset 0 1px 0 rgba(255,255,255,.18),0 4px 10px rgba(0,0,0,.5);}
.ez-row.ez-focused{box-shadow:0 0 0 3px var(--ez-focus),0 0 20px var(--ez-focus),
  inset 0 1px 0 rgba(255,255,255,.2);}
.ez-row .ez-rowlabel{flex:1.1;font-weight:800;letter-spacing:.1em;
  font-size:16px;}
.ez-row .ez-rowvalue{flex:1;display:flex;align-items:center;justify-content:flex-end;gap:10px;
  font-weight:900;font-size:16px;letter-spacing:.1em;color:var(--ez-volt);}
.ez-rowval{min-width:52px;text-align:right;}
.ez-vol{width:150px;height:14px;flex:none;background:#10151d;border:1px solid #05070b;
  border-radius:3px;overflow:hidden;}
.ez-vol i{display:block;height:100%;background:linear-gradient(180deg,#fff5,var(--ez-electric));
  box-shadow:0 0 8px var(--ez-electric);}

/* --- hints -------------------------------------------------------------- */
.ez-hints{position:absolute;left:0;right:0;bottom:8px;display:flex;gap:14px;
  justify-content:center;pointer-events:none;flex-wrap:wrap;}
.ez-hint{display:flex;gap:7px;align-items:center;font-size:12px;
  font-weight:700;letter-spacing:.12em;color:var(--ez-textdim);}
.ez-hint b{padding:2px 7px;border-radius:4px;background:#0a0e14;color:var(--ez-chrome);
  border:1px solid #2c3542;font-weight:900;}

/* --- transitions -------------------------------------------------------- */
.ez-tr{position:absolute;inset:0;pointer-events:none;}
.ez-tr .ez-wipeslab{position:absolute;top:0;bottom:0;width:60%;
  background:linear-gradient(100deg,#0b0f16 70%,var(--ez-electric));
  box-shadow:0 0 60px rgba(0,0,0,.9);}

/* --- gameplay HUD ------------------------------------------------------- */
.ez-hud{position:absolute;top:0;left:0;right:0;display:flex;align-items:flex-start;
  justify-content:space-between;padding:16px 22px;gap:16px;}
.ez-hud>div{background:linear-gradient(180deg,rgba(20,26,34,.92),rgba(10,14,20,.92));
  border:3px solid #06090e;padding:8px 16px;border-radius:6px;
  box-shadow:inset 0 2px 0 rgba(255,255,255,.22),0 6px 14px rgba(0,0,0,.6);}
.ez-hud-score{font-weight:900;font-size:30px;letter-spacing:.12em;
  color:var(--ez-volt);text-shadow:0 0 12px rgba(180,240,60,.5),0 2px 3px #000;}
.ez-hud-center{text-align:center;}
.ez-hud-down{font-weight:900;font-size:34px;letter-spacing:.08em;
  color:var(--ez-text);text-shadow:0 2px 4px #000;line-height:1;}
.ez-hud-togain{font-weight:700;font-size:14px;letter-spacing:.22em;color:var(--ez-chrome);
  margin-top:2px;}
.ez-hud-heat{font-weight:900;font-size:28px;letter-spacing:.14em;
  color:var(--ez-hot);text-shadow:0 0 14px rgba(227,62,48,.6),0 2px 3px #000;}

/* --- huddle play cards (a clickable chalkboard per play) ----------------- */
.ez-diagram{position:relative;border:3px solid #06090e;border-radius:10px;overflow:hidden;
  cursor:pointer;background:#0d241a;
  box-shadow:inset 0 2px 0 rgba(255,255,255,.14),0 8px 20px rgba(0,0,0,.55);
  transition:transform .08s ease,box-shadow .12s ease,border-color .12s ease;}
.ez-anim .ez-diagram:hover{transform:translateY(-3px);}
.ez-diagram.ez-focused{border-color:var(--ez-focus);
  box-shadow:inset 0 2px 0 rgba(255,255,255,.2),0 0 0 3px var(--ez-focus),
  0 0 26px rgba(57,192,255,.55),0 10px 22px rgba(0,0,0,.6);}
.ez-diagram-name{position:absolute;left:0;right:0;bottom:0;z-index:2;padding:7px 6px;
  text-align:center;font-weight:900;font-size:15px;letter-spacing:.16em;color:var(--ez-text);
  background:linear-gradient(180deg,rgba(6,12,10,0),rgba(6,12,10,.92));
  text-shadow:0 2px 4px #000;pointer-events:none;}
.ez-diagram.ez-focused .ez-diagram-name{color:var(--ez-focus);}
.ez-chalk{position:absolute;inset:0;width:100%;height:100%;display:block;}
.ez-chalk .ez-board{fill:#123023;}
.ez-chalk .ez-los{stroke:rgba(255,255,255,.5);stroke-width:.9;stroke-dasharray:2.4 2;}
.ez-chalk .ez-route{fill:none;stroke:rgba(233,241,245,.9);stroke-width:1.8;
  stroke-linejoin:round;stroke-linecap:round;}
.ez-chalk .ez-route.ez-decoy{stroke:rgba(233,241,245,.42);stroke-dasharray:3.4 2.4;}
.ez-chalk .ez-route.ez-primary{stroke:var(--ez-volt);stroke-width:2.6;}
.ez-chalk .ez-mark{fill:#e9f1f5;stroke:#06090e;stroke-width:.7;}
.ez-chalk .ez-mark.ez-primary{fill:var(--ez-volt);}
.ez-chalk .ez-qbdot{fill:#123023;}
"#;
