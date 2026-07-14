//! The procedural arcade stylesheet: every bevel, chrome gradient, glow,
//! scanline, sweep, and squash animation as one injected `<style>` constant.
//! No image files anywhere — gradients, clip-paths, shadows, and keyframes
//! only. Palette values arrive as CSS variables set from the typed theme.

/// The complete menu stylesheet.
pub const MENU_CSS: &str = r#"
#end-zone-menu{position:fixed;inset:0;overflow:hidden;z-index:40;pointer-events:none;
  font-family:'Arial Narrow',Impact,'Franklin Gothic Medium',Arial,sans-serif;
  color:var(--ez-text);transform-origin:0 0;}
#end-zone-menu *{box-sizing:border-box;margin:0;padding:0;}
.ez-widget{position:absolute;display:flex;align-items:center;justify-content:center;}
.ez-widgetfill{width:100%;height:100%;display:flex;flex-direction:column;
  align-items:center;justify-content:center;}
.ez-distinct{border-style:double;border-width:5px;}
.ez-dim{position:absolute;inset:0;background:
  radial-gradient(ellipse at 50% 30%,rgba(10,13,18,0.0) 0%,rgba(10,13,18,0.55) 100%),
  var(--ez-backdrop-dim);}
.ez-tint{position:absolute;inset:0;}
.ez-scan{position:absolute;inset:0;opacity:.16;
  background:repeating-linear-gradient(0deg,rgba(0,0,0,.55) 0 1px,transparent 1px 3px);}
.ez-vig{position:absolute;inset:0;box-shadow:inset 0 0 160px 40px rgba(0,0,0,.65);}
.ez-sweepbar{position:absolute;top:0;bottom:0;width:22%;pointer-events:none;opacity:.05;
  background:linear-gradient(100deg,transparent,rgba(255,255,255,.9),transparent);}
.ez-anim .ez-sweepbar{animation:ezsweep 7s linear infinite;}
@keyframes ezsweep{0%{left:-30%}100%{left:120%}}

/* --- title logo ------------------------------------------------------- */
.ez-logo{flex-direction:column;gap:14px;}
.ez-logo .ez-mark{font-style:italic;font-weight:900;letter-spacing:.02em;line-height:.9;
  transform:skewX(-8deg);text-transform:uppercase;
  background:linear-gradient(180deg,#f4f7fb 0%,#c7cfda 34%,#5f6c7d 50%,#e6ebf2 62%,#8a97a8 100%);
  -webkit-background-clip:text;background-clip:text;color:transparent;
  filter:drop-shadow(0 4px 0 #0b1524) drop-shadow(0 10px 22px rgba(0,0,0,.8))
         drop-shadow(0 0 26px var(--ez-electric));}
.ez-logo.ez-big .ez-mark{font-size:min(15vw,128px);}
.ez-logo.ez-small .ez-mark{font-size:44px;}
.ez-logo .ez-sub{font-size:14px;font-weight:700;letter-spacing:.55em;color:var(--ez-chrome);
  text-shadow:0 2px 6px #000;}
.ez-press{font-size:22px;font-weight:900;font-style:italic;letter-spacing:.2em;
  color:var(--ez-volt);text-shadow:0 0 14px var(--ez-volt),0 2px 4px #000;}
.ez-anim .ez-press{animation:ezblink 1.1s step-end infinite;}
@keyframes ezblink{0%,60%{opacity:1}61%,100%{opacity:.12}}

/* --- labels ------------------------------------------------------------ */
.ez-label{font-weight:900;text-transform:uppercase;text-align:center;
  text-shadow:0 2px 4px rgba(0,0,0,.85);}
.ez-label.ez-italic{font-style:italic;transform:skewX(-6deg);}
.ez-huge{font-size:calc(40px*var(--ez-ts));letter-spacing:.06em;}
.ez-heading{font-size:calc(26px*var(--ez-ts));letter-spacing:.12em;}
.ez-body{font-size:calc(17px*var(--ez-ts));letter-spacing:.08em;}
.ez-smalltext{font-size:calc(12px*var(--ez-ts));letter-spacing:.14em;color:var(--ez-textdim);}

/* --- buttons ------------------------------------------------------------ */
.ez-btn{width:100%;height:100%;display:flex;align-items:center;justify-content:center;
  font-weight:900;font-style:italic;text-transform:uppercase;
  font-size:calc(19px*var(--ez-ts));letter-spacing:.1em;color:var(--ez-text);
  border:3px solid #06090e;border-radius:6px;position:relative;
  background:linear-gradient(180deg,var(--ez-steel-l) 0%,var(--ez-steel-d) 55%,#0d1118 100%);
  box-shadow:inset 0 2px 0 rgba(255,255,255,.28),inset 0 -3px 0 rgba(0,0,0,.55),
    0 6px 14px rgba(0,0,0,.6);text-shadow:0 2px 3px rgba(0,0,0,.9);}
.ez-btn.ez-angled{clip-path:polygon(3% 0,100% 0,97% 100%,0 100%);border-radius:0;}
.ez-btn.ez-primary{background:linear-gradient(180deg,#3f8dff 0%,var(--ez-electric) 45%,#0c2f63 100%);}
.ez-btn.ez-danger{background:linear-gradient(180deg,#ff6a55 0%,var(--ez-hot) 45%,#5c150d 100%);}
.ez-focus .ez-btn,.ez-btn.ez-focused{outline:0;
  box-shadow:0 0 0 3px var(--ez-focus),0 0 26px var(--ez-focus),
    inset 0 2px 0 rgba(255,255,255,.35),0 6px 14px rgba(0,0,0,.6);}
.ez-disabled{opacity:.38;filter:saturate(.4);}
.ez-pressanim{animation:ezpress .14s ease-out;}
@keyframes ezpress{0%{transform:scale(1)}35%{transform:scale(.94,.88)}100%{transform:scale(1)}}

/* --- panels / cards ------------------------------------------------------ */
.ez-plate{background:linear-gradient(180deg,var(--ez-steel-l),var(--ez-steel-d));
  border:3px solid #06090e;border-radius:8px;
  box-shadow:inset 0 2px 0 rgba(255,255,255,.22),0 10px 24px rgba(0,0,0,.65);}
.ez-card{flex-direction:column;justify-content:flex-start;gap:6px;padding:12px 10px;
  background:linear-gradient(168deg,var(--ez-steel-l) 0%,var(--ez-steel-d) 60%,#0b0f16 100%);
  border:3px solid #06090e;border-radius:10px;overflow:hidden;
  box-shadow:inset 0 2px 0 rgba(255,255,255,.22),inset 0 0 0 2px var(--card-accent,#39424f),
    0 12px 26px rgba(0,0,0,.7);}
.ez-card .ez-cardtop{width:100%;height:8px;flex:none;border-radius:3px;
  background:linear-gradient(90deg,var(--card-primary),var(--card-secondary));}
.ez-card.ez-focused{box-shadow:0 0 0 3px var(--ez-focus),0 0 30px var(--ez-focus),
  inset 0 2px 0 rgba(255,255,255,.3),0 12px 26px rgba(0,0,0,.7);}
.ez-card.ez-preview{opacity:.62;filter:saturate(.75);}
.ez-card .ez-city{font-size:calc(13px*var(--ez-ts));font-weight:700;letter-spacing:.3em;
  color:var(--ez-textdim);}
.ez-card .ez-name{font-size:calc(27px*var(--ez-ts));font-weight:900;font-style:italic;
  letter-spacing:.05em;color:var(--ez-text);text-shadow:0 2px 4px #000;}
.ez-card.ez-compact .ez-name{font-size:calc(17px*var(--ez-ts));}
.ez-card .ez-abbr{font-size:calc(12px*var(--ez-ts));font-weight:900;letter-spacing:.4em;
  color:var(--card-accent);}
.ez-card .ez-emblem{width:38%;max-width:110px;flex:none;filter:drop-shadow(0 4px 8px rgba(0,0,0,.7));}
.ez-card.ez-compact .ez-emblem{width:52%;}
.ez-sidechip{position:absolute;top:8px;right:8px;font-size:11px;font-weight:900;
  letter-spacing:.2em;padding:3px 8px;border-radius:3px;background:#06090e;
  color:var(--ez-chrome);border:1px solid var(--card-accent);}
.ez-lockchip{position:absolute;top:8px;left:8px;font-size:11px;font-weight:900;
  letter-spacing:.2em;padding:3px 8px;border-radius:3px;background:var(--ez-volt);
  color:#0a0d12;}
.ez-bars{width:100%;display:flex;flex-direction:column;gap:4px;}
.ez-bar{display:flex;align-items:center;gap:6px;font-size:calc(11px*var(--ez-ts));
  font-weight:700;letter-spacing:.12em;color:var(--ez-textdim);}
.ez-bar b{width:64px;flex:none;text-align:right;color:var(--ez-text);}
.ez-barcells{display:flex;gap:2px;flex:1;}
.ez-cell{height:9px;flex:1;background:#10151d;border:1px solid #05070b;transform:skewX(-14deg);}
.ez-cell.on{background:linear-gradient(180deg,#fff6,var(--card-accent) 40%,var(--card-accent));
  box-shadow:0 0 6px var(--card-accent);}
.ez-lineup{display:flex;gap:4px;margin-top:2px;}
.ez-jersey{width:16px;height:20px;border-radius:3px 3px 5px 5px;border:1px solid #05070b;
  background:linear-gradient(180deg,var(--card-primary) 62%,var(--card-secondary) 62%);}

/* --- settings rows / tabs / selectors ------------------------------------ */
.ez-tabs{display:flex;gap:6px;width:100%;height:100%;align-items:stretch;}
.ez-tab{flex:1;display:flex;align-items:center;justify-content:center;
  font-size:calc(13px*var(--ez-ts));font-weight:900;font-style:italic;letter-spacing:.12em;
  color:var(--ez-textdim);background:linear-gradient(180deg,#161b23,#0d1118);
  border:2px solid #06090e;clip-path:polygon(6% 0,100% 0,94% 100%,0 100%);}
.ez-tab.on{color:#0a0d12;background:linear-gradient(180deg,#e8edf4,var(--ez-chrome));
  text-shadow:none;}
.ez-row{width:100%;height:100%;display:flex;align-items:center;gap:12px;padding:0 16px;
  background:linear-gradient(180deg,var(--ez-steel-l),var(--ez-steel-d));
  border:2px solid #06090e;border-radius:6px;
  box-shadow:inset 0 1px 0 rgba(255,255,255,.18),0 4px 10px rgba(0,0,0,.5);}
.ez-row.ez-focused{box-shadow:0 0 0 3px var(--ez-focus),0 0 20px var(--ez-focus),
  inset 0 1px 0 rgba(255,255,255,.2);}
.ez-row .ez-rowlabel{flex:1.1;font-weight:900;font-style:italic;letter-spacing:.08em;
  font-size:calc(15px*var(--ez-ts));}
.ez-row .ez-rowdetail{flex:1.4;font-size:calc(10px*var(--ez-ts));letter-spacing:.05em;
  color:var(--ez-textdim);line-height:1.25;}
.ez-row .ez-rowvalue{flex:1;display:flex;align-items:center;justify-content:flex-end;gap:8px;
  font-weight:900;font-size:calc(15px*var(--ez-ts));letter-spacing:.1em;color:var(--ez-volt);}
.ez-arrow{color:var(--ez-chrome);font-size:14px;}
.ez-arrow.off{opacity:.2;}
.ez-toggle{width:46px;height:20px;border-radius:12px;border:2px solid #06090e;
  background:#10151d;position:relative;flex:none;}
.ez-toggle i{position:absolute;top:1px;width:14px;height:14px;border-radius:50%;
  background:var(--ez-chrome);left:2px;}
.ez-toggle.on{background:var(--ez-electric);}
.ez-toggle.on i{left:auto;right:2px;background:#fff;}
.ez-vol{display:flex;gap:2px;width:130px;flex:none;}
.ez-vol i{flex:1;height:12px;background:#10151d;border:1px solid #05070b;transform:skewX(-14deg);}
.ez-vol i.on{background:linear-gradient(180deg,#fff5,var(--ez-electric));
  box-shadow:0 0 5px var(--ez-electric);}
.ez-bindchips{display:flex;gap:5px;}
.ez-chip{font-size:11px;font-weight:900;letter-spacing:.08em;padding:3px 8px;border-radius:4px;
  background:#0a0e14;color:var(--ez-chrome);border:1px solid #2c3542;
  box-shadow:inset 0 -2px 0 rgba(0,0,0,.6);}
.ez-chip.warn{border-color:var(--ez-hot);color:var(--ez-hot);}
.ez-capture{color:var(--ez-volt);}
.ez-anim .ez-capture{animation:ezblink .7s step-end infinite;}
.ez-selector{width:100%;height:100%;flex-direction:column;gap:6px;
  background:linear-gradient(180deg,var(--ez-steel-l),var(--ez-steel-d));
  border:3px solid #06090e;clip-path:polygon(2% 0,100% 0,98% 100%,0 100%);
  box-shadow:inset 0 2px 0 rgba(255,255,255,.2);}
.ez-selector.ez-focused{box-shadow:0 0 0 3px var(--ez-focus),0 0 22px var(--ez-focus);}
.ez-selector .ez-sellabel{font-size:calc(11px*var(--ez-ts));font-weight:700;
  letter-spacing:.3em;color:var(--ez-textdim);}
.ez-selector .ez-selvalue{display:flex;gap:14px;align-items:center;font-weight:900;
  font-style:italic;font-size:calc(21px*var(--ez-ts));letter-spacing:.1em;color:var(--ez-text);}

/* --- hints / modal / transitions ----------------------------------------- */
.ez-hints{position:absolute;left:0;right:0;bottom:8px;display:flex;gap:14px;
  justify-content:center;pointer-events:none;}
.ez-hint{display:flex;gap:7px;align-items:center;font-size:calc(11px*var(--ez-ts));
  font-weight:700;letter-spacing:.12em;color:var(--ez-textdim);}
.ez-hint b{padding:2px 7px;border-radius:4px;background:#0a0e14;color:var(--ez-chrome);
  border:1px solid #2c3542;font-weight:900;}
.ez-modalveil{position:absolute;inset:0;background:rgba(4,6,10,.72);}
.ez-modal{position:absolute;left:50%;top:30%;transform:translateX(-50%);width:min(62%,560px);
  padding:22px 26px 84px;
  background:linear-gradient(180deg,var(--ez-steel-l),var(--ez-steel-d));
  border:3px solid #06090e;border-radius:10px;clip-path:polygon(1.5% 0,100% 0,98.5% 100%,0 100%);
  box-shadow:inset 0 2px 0 rgba(255,255,255,.25),0 18px 44px rgba(0,0,0,.8);}
.ez-modal h3{font-size:calc(24px*var(--ez-ts));font-weight:900;font-style:italic;
  letter-spacing:.08em;margin-bottom:8px;color:var(--ez-text);}
.ez-modal p{font-size:calc(13px*var(--ez-ts));letter-spacing:.04em;color:var(--ez-textdim);}
.ez-tr{position:absolute;inset:0;pointer-events:none;}
.ez-tr .ez-wipeslab{position:absolute;top:0;bottom:0;width:60%;
  background:linear-gradient(100deg,#0b0f16 70%,var(--ez-electric));transform:skewX(-10deg);
  box-shadow:0 0 60px rgba(0,0,0,.9);}
.ez-reduced .ez-scan{opacity:.10;}
.ez-hc .ez-btn,.ez-hc .ez-row,.ez-hc .ez-card{border-width:4px;}
"#;
