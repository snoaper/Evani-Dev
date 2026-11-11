//! WP6 (docs/studio/WORK_PACKAGES.md), wasm only: the studio page. The
//! mockup's layout round the 3D view: the style bar and the materials bar
//! along the top, the asset browser on the left, the inspector (material
//! settings, the style's controls, global lighting) and the side column
//! (current combination, export, save, presets) on the right, the
//! variations strip under the view, and one notice line over it.
//!
//! - Every control pushes what [`page_view::action`] says: a
//!   `StudioCommand` for the kit's studio system (this system runs just
//!   before it, so a click lands the same frame), a material switch, or a
//!   page-only change. The markup is [`page_view`]'s; each part is written
//!   only when what it shows changes, and an input being typed in or
//!   dragged keeps its focus and value.
//! - The view in the middle is the 3D scene's (WP4: its own tool rail,
//!   combination card and view bar sit in the gap these panels leave,
//!   measured through the page's covers in web.rs). A build without the
//!   studio scene shows a clear "3D view coming" card there instead.
//! - Sized by the room the page really has: the frame measures the window
//!   and picks a wide (four columns), mid (three) or narrow (the view on
//!   top, a tabbed dock under it) layout; inside, each panel sizes by its
//!   own box (CSS container queries). Long names end in "…" with the full
//!   name as a tooltip.
//! - The record is kept: `Studio::storage_due` is written to IndexedDB and
//!   to localStorage (the fallback); on start the stored state is restored
//!   (`StudioCommand::Restore`), with its settings and saved combinations.
//! - The mode switch at the top ("Materials" | "Procedural") is
//!   proc_page.rs's: in procedural mode the page shows its effect cards and
//!   effect panel instead of these parts (the hooks here are marked PROC).
//! - No pointer lock, keyboard lock or fullscreen here (fullscreen is the
//!   scene overlay's, on a click).

use std::cell::RefCell;
use std::rc::Rc;

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::plugin::PluginContext;
use evani_core::tiers::Tiers;
use evani_kit_material_view::catalogue::LibraryData;
use evani_kit_material_view::studio::bridge::Bridge;
use evani_kit_material_view::studio::params::StudioParams;
use evani_kit_material_view::studio::{Studio, StudioCommand, StudioData, Tweak, variations_for};
use evani_kit_material_view::{Command, Viewer, ViewerData};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, Event, HtmlElement, HtmlInputElement};

use super::loader::{SourceState, StudioSource};
use super::page_view::{self as pv, Act, PageUi, UiAct};

/// The scene the studio draws in (WP4's `scene::SCENE`).
const SCENE: &str = "studio";
/// localStorage key of the stored state (IndexedDB keeps the same).
const STORE_KEY: &str = "evani.studio.state";
/// Frames a page message ("Saved to presets") stays in the notice line.
const MESSAGE_FRAMES: u32 = 240;

const CSS: &str = r#"
body.studio-on #mv-hud,body.studio-on #mv-bar,body.studio-on #mv-scenes,body.studio-on #mv-drawer,body.studio-on #mv-panel,
body.studio-on #mv-styles,body.studio-on #mv-refs,body.studio-on #evani-touch{display:none!important}
/* No 3D studio scene in this build: the legacy scenes' test shapes never peek between the panels. */
body.studio-coming #evani-canvas{visibility:hidden}
body.studio-coming{background:#cdb79c}
#studio-page{color-scheme:dark;--pa:rgba(24,23,28,.92);--ln:rgba(255,255,255,.11);--ink:#f3eee6;--dim:#a8a39b;--acc:#e9b949;
 position:fixed;inset:0;z-index:5;pointer-events:none;display:grid;gap:8px;box-sizing:border-box;
 padding:8px 8px calc(8px + var(--dev-bar,0px));color:var(--ink);font:13px/1.3 system-ui,-apple-system,"Segoe UI",sans-serif;
 grid-template-columns:clamp(270px,25vw,560px) minmax(0,1fr) clamp(232px,15.5vw,360px) clamp(206px,14.5vw,340px);
 grid-template-rows:auto auto minmax(0,1fr) auto;
 grid-template-areas:"top top top top" "left note insp side" "left view insp side" "left vars vars side"}
#studio-page[hidden]{display:none}
@media (min-width:1900px){#studio-page{font-size:15px;gap:10px}#studio-page h3{font-size:12.5px}}
body:has(#studio-preview-library) #studio-page{padding-bottom:calc(46px + var(--dev-bar,0px))}
#studio-page *{box-sizing:border-box}
#studio-page button,#studio-page select,#studio-page input{font:inherit;color:inherit}
#studio-page button{background:rgba(255,255,255,.06);border:1px solid var(--ln);border-radius:9px;cursor:pointer;min-height:32px;padding:0 10px;touch-action:manipulation}
#studio-page button:hover{background:rgba(255,255,255,.13)}
#studio-page button.on{border-color:var(--acc);background:rgba(233,185,73,.16);box-shadow:0 0 0 1px var(--acc) inset}
#studio-page .ic{flex:none;width:18px;text-align:center;font-size:15px;line-height:1}
#studio-page .tx,#studio-page .nm{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
#studio-page .sr{position:absolute;width:1px;height:1px;overflow:hidden;clip:rect(0 0 0 0)}
#studio-page .st-none{color:var(--dim);margin:8px 4px;font-size:12px}
#studio-page h3{margin:0 0 8px;font-size:11.5px;font-weight:650;letter-spacing:.06em;text-transform:uppercase;color:var(--ink);display:flex;align-items:center;justify-content:space-between;gap:8px;min-height:26px}
.st-panel{pointer-events:auto;background:var(--pa);border:1px solid var(--ln);border-radius:12px;min-width:0;min-height:0;overflow:hidden;backdrop-filter:blur(6px)}
#st-top{grid-area:top;display:flex;flex-direction:column;gap:6px;min-width:0;pointer-events:none}
#st-styles,#st-mats{display:flex;align-items:center;gap:6px;padding:5px;container-type:inline-size}
.st-styles-row,.st-chips{flex:1;min-width:0;display:flex;gap:6px;overflow-x:auto;scrollbar-width:none;scroll-behavior:smooth}
.st-styles-row::-webkit-scrollbar,.st-chips::-webkit-scrollbar,.st-vars-row::-webkit-scrollbar{display:none}
.st-arrow{flex:none;width:32px;padding:0!important;font-size:18px}
.st-style{flex:none;display:flex;align-items:center;gap:6px;min-height:38px!important;max-width:150px}
@container (max-width:640px){.st-style{min-height:34px!important;padding:0 8px!important}.st-style .ic{display:none}}
.st-matsel select{height:34px;border-radius:9px;border:1px solid var(--ln);background:rgba(255,255,255,.06);padding:0 8px;max-width:150px}
.st-matsel option{background:#222}
.st-search{flex:none;display:flex;align-items:center;gap:6px;height:34px;padding:0 8px;border:1px solid var(--ln);border-radius:9px;background:rgba(0,0,0,.22);width:clamp(130px,16cqw,240px)}
.st-search input{flex:1;min-width:0;border:0;background:none;outline:none;height:30px}
.st-all,.st-more{flex:none}
.st-all{border-radius:999px!important;padding:0 16px!important}
#studio-page .st-all.on{background:#ebe6de;color:#1d1b18;border-color:transparent;box-shadow:none}
.st-chip{flex:none;display:flex;align-items:center;gap:7px;border-radius:10px!important;padding:0 10px 0 3px!important;max-width:170px;min-height:36px!important}
.st-chip i{flex:none;width:28px;height:28px;border-radius:50%;overflow:hidden;background:radial-gradient(circle at 45% 40%,color-mix(in srgb,var(--sw) 70%,#555) 0,color-mix(in srgb,var(--sw) 35%,#111) 100%)}
.st-chip span{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.st-chip.foreign{opacity:.55}
@container (max-width:560px){.st-matsel{display:none}.st-search{width:110px}}
#st-morepop{position:fixed;z-index:60;left:50%;transform:translateX(-50%);top:var(--st-top-b,120px);width:min(980px,calc(100vw - 16px));max-height:min(62vh,640px);overflow:auto;padding:10px 12px}
#st-morepop[hidden]{display:none}
.st-more-head{display:flex;justify-content:space-between;align-items:center;margin-bottom:6px;position:sticky;top:-10px;background:var(--pa);padding:4px 0}
#st-morepop h4{margin:10px 0 6px;font-size:12px;color:var(--dim);font-weight:600}
#st-morepop h4 small{opacity:.7}
.st-more-row{display:flex;flex-wrap:wrap;gap:6px}
#st-note{grid-area:note;pointer-events:auto;justify-self:center;max-width:100%;padding:6px 12px;font-size:13px;border-color:rgba(233,185,73,.5);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
#st-note[hidden]{display:none}
#st-note .err{color:#f0a48a}
#st-note button{min-height:26px;margin-left:8px}
#st-view{grid-area:view;min-width:0;min-height:0;display:flex;align-items:center;justify-content:center;pointer-events:none;container-type:size}
.st-coming{pointer-events:auto;width:100%;height:100%;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:4px;text-align:center;padding:16px;
 background:radial-gradient(ellipse at 50% 40%,#e9d8c2 0,#cdb79c 70%,#b9a084 100%);border-radius:12px;color:#2b241d}
#studio-page .st-coming .pic{flex:none;width:min(26cqh,140px);height:auto;font-size:min(12cqh,60px)}
@container (max-height:300px){.st-coming>span{display:none}.st-coming p{margin-top:4px}}
.st-coming b{font-size:18px;max-width:100%;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.st-coming span{font-size:13px;opacity:.8;max-width:100%;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.st-coming p{max-width:420px;margin:8px 0 0;font-size:13px;background:rgba(24,23,28,.86);color:var(--ink);border:1px solid rgba(233,185,73,.55);border-radius:10px;padding:8px 12px}
#st-left{grid-area:left;display:flex;flex-direction:column;container-type:inline-size}
.st-tabs{display:flex;gap:3px;padding:8px 8px 0}
.st-tab{flex:1;min-width:0;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:3px;padding:4px 1px!important;min-height:52px!important;font-size:11px}
.st-tab.st-tab-all{flex:0 0 28px}
.st-tab .ic{font-size:17px}
.st-tab .tx{max-width:100%}
.st-tab .tx{font-size:10.5px}
@container (max-width:330px){.st-tab .tx{font-size:9px;letter-spacing:-.03em}}
@container (max-width:250px){.st-tab .tx{display:none}.st-tab{min-height:38px!important}.st-tab .ic{font-size:18px}}
.st-find{display:flex;gap:6px;padding:8px}
.st-find .st-search{flex:1;width:auto}
.st-filter{flex:none;width:36px;padding:0!important}
.st-cats{flex:1;min-height:0;overflow-y:auto;padding:0 8px 8px;overscroll-behavior:contain}
.st-cat{width:100%;display:flex;align-items:center;gap:8px;text-align:left;margin-bottom:4px;background:transparent!important;border-color:transparent!important}
.st-cat:hover{background:rgba(255,255,255,.07)!important}
.st-cat.on{background:rgba(255,255,255,.08)!important;box-shadow:none!important}
.st-cat .nm{flex:1}
.st-cat .n{color:var(--dim);font-variant-numeric:tabular-nums}
.st-cat.on .n{color:var(--acc)}
.st-grid{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:6px;margin:2px 0 8px}
@container (min-width:370px){.st-grid{grid-template-columns:repeat(4,minmax(0,1fr))}}
@container (min-width:500px){.st-grid{grid-template-columns:repeat(5,minmax(0,1fr))}}
@container (min-width:640px){.st-grid{grid-template-columns:repeat(6,minmax(0,1fr))}}
@container (max-width:200px){.st-grid{grid-template-columns:repeat(2,minmax(0,1fr))}}
.st-thumb,.st-var,.st-preset,.st-light{display:flex;flex-direction:column;align-items:center;gap:3px;padding:5px!important;position:relative;min-width:0}
.st-thumb .nm,.st-var .nm,.st-preset .nm,.st-light .tx{max-width:100%;font-size:12px}
.st-var .nm,.st-preset .nm{white-space:normal!important;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;line-height:1.15;text-align:center;overflow:hidden;font-size:11px}
#studio-page .pic{display:flex;align-items:center;justify-content:center;width:100%;aspect-ratio:1;border-radius:8px;font-size:30px;color:rgba(255,255,255,.92);
 text-shadow:0 1px 3px rgba(0,0,0,.6);background:radial-gradient(circle at 50% 38%,color-mix(in srgb,var(--sw,#888) 75%,white) 0,var(--sw,#888) 45%,color-mix(in srgb,var(--sw,#888) 55%,black) 100%)}
#studio-page .pic.img{position:relative;overflow:hidden;background:radial-gradient(circle at 50% 35%,#4c4a52 0,#313036 68%,#28272c 100%)}
#studio-page .pic.img img{width:88%;height:88%;object-fit:contain;display:block;pointer-events:none;user-select:none}
#studio-page .pic.tinted img{filter:grayscale(1) brightness(1.55) contrast(1.05)}
#studio-page .pic>.tc{position:absolute;inset:0;width:100%;height:100%;border-radius:inherit;opacity:0;pointer-events:none;user-select:none}#studio-page .pic>.tc[data-k]{opacity:1}#studio-page .pic:has(>.tc){position:relative;overflow:hidden}
#studio-page .pic .tint{position:absolute;inset:6%;background:var(--sw);-webkit-mask-size:contain;mask-size:contain;-webkit-mask-repeat:no-repeat;mask-repeat:no-repeat;-webkit-mask-position:center;mask-position:center;mix-blend-mode:multiply;pointer-events:none}
.st-thumb.unbuilt{border-style:dashed}
.st-thumb.unbuilt .pic{filter:saturate(.25) brightness(.8)}
#studio-page em{position:absolute;left:4px;right:4px;top:6px;font-style:normal;font-size:10px;line-height:1.2;padding:2px 4px;border-radius:6px;background:rgba(20,20,24,.85);color:#f1d9a0;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
#st-insp,#st-side{overflow-y:auto;overscroll-behavior:contain;padding:10px 12px;container-type:inline-size}
#st-insp{grid-area:insp;display:flex;flex-direction:column;overflow:hidden;padding:0}
.st-insp-main{flex:1;min-height:0;overflow-y:auto;overscroll-behavior:contain;padding:10px 12px 0;scrollbar-width:thin}
#studio-page .st-lighting{flex:none;margin:0;padding:10px 12px 12px;border-top:1px solid var(--ln);border-bottom:0}
#st-side{grid-area:side}
#st-right{display:contents}
.st-sec{padding-bottom:10px;margin-bottom:10px;border-bottom:1px solid var(--ln)}
.st-hint{font-size:10.5px;font-weight:500;letter-spacing:0;text-transform:none;color:var(--dim);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
.st-plist{display:flex;flex-direction:column;gap:4px}
.st-part{display:flex;align-items:center;gap:8px;text-align:left;min-height:40px!important;padding:3px 8px 3px 4px!important;min-width:0}
.st-part .tx{display:flex;flex-direction:column;min-width:0;flex:1}
.st-part .tx b{font-weight:600;font-size:12.5px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.st-part .tx small{font-size:11px;color:var(--dim);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.st-part .pb{flex:none;width:30px;height:30px;border-radius:50%;overflow:hidden;background:radial-gradient(circle at 45% 40%,color-mix(in srgb,var(--sw,#888) 70%,#555) 0,color-mix(in srgb,var(--sw,#888) 35%,#111) 100%)}
.st-part .pb.all{display:flex;align-items:center;justify-content:center;background:rgba(255,255,255,.1);font-size:14px}
.st-part .pb.own{background:radial-gradient(circle at 40% 35%,#555 0,#111 70%)}
.st-part-all{min-height:34px!important}
.st-psum{list-style:none;margin:0 0 10px;padding:0;display:flex;flex-direction:column;gap:3px;font-size:11.5px}
.st-psum li{display:grid;grid-template-columns:12px minmax(0,auto) minmax(0,1fr);gap:6px;align-items:center;min-width:0}
.st-psum li i{width:12px;height:12px;border-radius:50%;background:var(--sw)}
.st-psum li span{color:var(--dim);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.st-psum li b{font-weight:500;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;text-align:right}
.st-psum li.own b{color:var(--acc)}
.st-editing{margin:-4px 0 6px;font-size:11.5px;color:var(--dim);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.st-editing b{color:var(--ink);font-weight:550}
.st-sec:last-child{border-bottom:0;margin-bottom:0}
.st-small{min-height:26px!important;font-size:12px;text-transform:none;letter-spacing:0}
.st-ball{position:relative;width:min(clamp(64px,12vh,150px),52cqw);aspect-ratio:1;margin:2px auto 10px;border-radius:50%;background:radial-gradient(circle at 45% 40%,color-mix(in srgb,var(--sw) 70%,#555) 0,color-mix(in srgb,var(--sw) 35%,#111) 100%);box-shadow:0 8px 18px rgba(0,0,0,.4)}
/* The engine's own picture of the material (studio/balls.rs); empty, so the soft colour shows, only while it is made. */
.bc{display:block;width:100%;height:100%;border-radius:50%;pointer-events:none;user-select:none}
.st-row{margin:6px 0 8px}
.st-row .lab{display:flex;justify-content:space-between;gap:8px;font-size:12.5px}
.st-row .lab span{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.st-row .lab b{font-weight:500;font-variant-numeric:tabular-nums}
.st-row input[type=range]{width:100%;height:24px;margin:2px 0 0;accent-color:var(--acc)}
.st-hex{display:flex;align-items:center;margin-top:4px;border:1px solid var(--ln);border-radius:8px;overflow:hidden;height:30px}
.st-hex i{flex:1;align-self:stretch}
.st-hex b{flex:none;padding:0 10px;font-weight:500;font-variant-numeric:tabular-nums;background:rgba(0,0,0,.3);align-self:stretch;display:flex;align-items:center}
.st-lights{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:6px}
@container (max-width:170px){.st-lights{grid-template-columns:repeat(2,minmax(0,1fr))}}
.st-light .tx{font-size:11px!important}
@container (max-width:320px){.st-lights{gap:3px}.st-light{padding:4px 0!important}.st-light .tx{font-size:10px!important;letter-spacing:-.02em}}
.lt-studio{background:radial-gradient(circle at 50% 55%,#ffd7a0 0,#c9783c 30%,#3a2418 75%)!important}
.lt-sunset{background:linear-gradient(#f6a94c 0,#e0602e 48%,#5a2a1f 52%,#2b1a18 100%)!important}
.lt-daylight{background:linear-gradient(#5fa6e8 0,#bfe0f7 55%,#8a9a6c 58%,#5b6b48 100%)!important}
.lt-night{background:radial-gradient(circle at 70% 30%,#e8eefc 0 4%,#2a3552 6%,#0c111d 80%)!important}
.st-quality{display:flex;align-items:center;justify-content:space-between;gap:8px;margin-top:10px;font-size:12.5px}
.st-quality select{height:30px;border-radius:8px;border:1px solid var(--ln);background:rgba(255,255,255,.06);padding:0 6px;min-width:0}
.st-quality option{background:#222}
.st-cur{display:grid;grid-template-columns:auto minmax(0,1fr);grid-template-areas:"pic tx" "pic tags";column-gap:10px;row-gap:4px;align-items:center;padding:8px;border-radius:10px;background:rgba(255,255,255,.04);margin-bottom:10px}
.st-cur>.pic{grid-area:pic}.st-cur-tx{grid-area:tx;align-self:end}.st-cur>.st-tags{grid-area:tags;align-self:start}
@container (max-width:240px){.st-cur{grid-template-areas:"pic tx" "tags tags"}}
#studio-page .st-cur .pic{flex:none;width:72px;font-size:26px}
.st-cur-tx{min-width:0;flex:1;display:flex;flex-direction:column;gap:2px}
.st-cur-tx b{font-size:15px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.st-cur-tx>span{color:var(--dim);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.st-tags{display:flex;gap:4px;flex-wrap:wrap;min-width:0}
.st-tags span{font-size:10.5px;padding:1px 5px;border:1px solid var(--ln);border-radius:6px;max-width:100%;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
@container (max-width:240px){#studio-page .st-cur .pic{width:52px}}
.st-export{display:flex;gap:2px;margin-bottom:8px}
.st-primary{flex:1;min-height:40px!important;background:var(--acc)!important;color:#241b08!important;font-weight:650;border:0!important}
.st-primary:hover{filter:brightness(1.08)}
.st-caret{flex:none;width:40px}
.st-menu{display:flex;flex-direction:column;gap:4px;margin:-2px 0 8px}
.st-save{width:100%;min-height:40px!important}
.st-presets{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:6px}
@container (max-width:200px){.st-presets{grid-template-columns:repeat(2,minmax(0,1fr))}}
.st-preset{padding:4px!important}
.st-preset .nm{font-size:11px!important}
#st-vars{grid-area:vars;padding:8px 10px;container-type:inline-size}
#st-vars h3{margin-bottom:6px}
.st-vars-row{display:flex;gap:6px;overflow-x:auto;scrollbar-width:none;padding-bottom:2px}
.st-var{flex:none;width:clamp(80px,10.5cqw,112px)}
.st-var .nm{font-size:11.5px!important;-webkit-line-clamp:3}
.st-var.foreign .pic{filter:grayscale(.85) brightness(.7)}
.st-var.foreign{opacity:.75}
.st-var em.dflt{right:auto;left:4px;top:4px;font-size:9px;padding:1px 5px;background:rgba(20,20,24,.78);color:#e9dcc0}
.st-var em.tick{left:auto;right:4px;width:20px;height:20px;padding:0;border-radius:50%;background:var(--acc);color:#241b08;font-size:13px;text-align:center;line-height:20px}
.st-add .pic{background:rgba(255,255,255,.05)!important;border:1px dashed var(--ln);font-size:26px}
#st-tabs{grid-area:tabs;display:none;gap:6px;padding:5px}
#st-tabs button{flex:1}
/* Mid: three columns; the side column under the inspector. */
#studio-page.mid{grid-template-columns:clamp(210px,24vw,300px) minmax(0,1fr) clamp(240px,28vw,330px);
 grid-template-areas:"top top top" "left note right" "left view right" "left vars right"}
#studio-page.mid #st-right{display:flex;flex-direction:column;grid-area:right;min-height:0;overflow-y:auto;gap:8px;pointer-events:auto}
#studio-page.mid #st-insp,#studio-page.mid #st-side{overflow:visible;flex:none}
#studio-page.mid .st-insp-main{overflow:visible}
#studio-page.mid #st-side{order:-1}
/* Narrow: the view on top, a tabbed dock under it. */
#studio-page.narrow{grid-template-columns:minmax(0,1fr);grid-template-rows:auto auto minmax(160px,1fr) auto auto minmax(0,40%);
 grid-template-areas:"top" "note" "view" "vars" "tabs" "dock"}
#studio-page.narrow #st-tabs{display:flex}
#studio-page.narrow #st-left{grid-area:dock}
#studio-page.narrow #st-right{display:block;grid-area:dock;min-height:0}
#studio-page.narrow #st-insp,#studio-page.narrow #st-side{height:100%}
#studio-page.narrow:not(.dock-assets) #st-left,#studio-page.narrow.dock-assets #st-right,
#studio-page.narrow.dock-material #st-side,#studio-page.narrow.dock-combo #st-insp{display:none}
#studio-page.narrow .st-var{width:68px}
#studio-page.narrow #st-vars{padding:6px 8px}
#studio-page.short .st-style,#studio-page.short .st-chip{min-height:30px!important}
#studio-page.short .st-tab{min-height:34px!important}
#studio-page.short .st-ball{width:72px;margin-bottom:6px}
#studio-page.short .st-tab .tx{display:none}
#studio-page.short #st-vars h3{display:none}
#studio-page.short .st-var{width:62px}
"#;

const HTML: &str = r#"
<div id="st-top"><nav id="st-styles" class="st-panel" aria-label="Art style"></nav><div id="st-mats" class="st-panel" aria-label="Materials"></div></div>
<div id="st-morepop" class="st-panel" hidden></div>
<div id="st-note" class="st-panel" role="status" aria-live="polite" hidden></div>
<aside id="st-left" class="st-panel" aria-label="Assets"></aside>
<div id="st-view"></div>
<div id="st-right"><aside id="st-insp" class="st-panel" aria-label="Material settings"></aside><aside id="st-side" class="st-panel" aria-label="Current combination and presets"></aside></div>
<section id="st-vars" class="st-panel" aria-label="Variations"></section>
<div id="st-tabs" class="st-panel" role="tablist" aria-label="Panels"><button data-act="dock" data-v="assets">Assets</button><button data-act="dock" data-v="material">Material</button><button data-act="dock" data-v="combo">Combination</button></div>
"#;

/// What the listeners collected since the last frame: (data-act, data-v,
/// input value).
type Queue = Rc<RefCell<Vec<(String, String, String)>>>;
/// IndexedDB's stored state, once read (`Some(None)`: read, nothing there).
type Stored = Rc<RefCell<Option<Option<String>>>>;

struct Dom {
    doc: Document,
    root: HtmlElement,
    styles: Element,
    mats: Element,
    more: HtmlElement,
    note: HtmlElement,
    left: Element,
    view: Element,
    insp: Element,
    side: Element,
    vars: Element,
    tabs: Element,
    // PROC: the procedural mode's parts.
    #[cfg(all(feature = "material-nature", feature = "material-playground"))]
    proc: super::proc_page::ProcDom,
}

/// What each part last showed (its key), and the page's own state.
#[derive(Default)]
struct Page {
    started: bool,
    ui: PageUi,
    keys: [String; 9],
    layout: String,
    values_at: (u64, u64),
    /// A message the page shows in the notice line, and its frames left.
    message: Option<(String, u32)>,
    /// The user did something: a late IndexedDB read no longer restores.
    touched: bool,
    local: Option<String>,
    focus_search: bool,
    #[cfg(all(feature = "material-nature", feature = "material-playground"))]
    proc: super::proc_page::ProcPage,
}

fn get(root: &Element, sel: &str) -> Result<Element, String> {
    root.query_selector(sel).ok().flatten().ok_or_else(|| format!("studio page: no {sel}"))
}

fn make(doc: &Document, queue: &Queue) -> Result<Dom, String> {
    let body = doc.body().ok_or("studio page: no body")?;
    let style = doc.create_element("style").map_err(|_| "studio page: style")?;
    style.set_text_content(Some(CSS));
    body.append_child(&style).map_err(|_| "studio page: append style")?;
    let root: HtmlElement = doc.create_element("div").map_err(|_| "studio page: div")?.dyn_into().map_err(|_| "studio page: div")?;
    root.set_id("studio-page");
    root.set_hidden(true);
    root.set_inner_html(HTML);
    body.append_child(&root).map_err(|_| "studio page: append")?;
    listen(&root, queue)?;
    let html = |e: Element| e.dyn_into::<HtmlElement>().map_err(|_| "studio page: element".to_string());
    Ok(Dom {
        doc: doc.clone(),
        styles: get(&root, "#st-styles")?,
        mats: get(&root, "#st-mats")?,
        more: html(get(&root, "#st-morepop")?)?,
        note: html(get(&root, "#st-note")?)?,
        left: get(&root, "#st-left")?,
        view: get(&root, "#st-view")?,
        insp: get(&root, "#st-insp")?,
        side: get(&root, "#st-side")?,
        vars: get(&root, "#st-vars")?,
        tabs: get(&root, "#st-tabs")?,
        #[cfg(all(feature = "material-nature", feature = "material-playground"))]
        proc: super::proc_page::mount(doc, &root)?,
        root,
    })
}

/// One delegated listener per event kind: a click on a button, an input
/// moving (sliders, search boxes) and a select changing.
fn listen(root: &HtmlElement, queue: &Queue) -> Result<(), String> {
    for kind in ["click", "input", "change"] {
        let q = queue.clone();
        let cb = Closure::<dyn FnMut(Event)>::new(move |e: Event| {
            let Some(t) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) else { return };
            let Some(el) = t.closest("[data-act]").ok().flatten() else { return };
            let tag = el.tag_name().to_ascii_lowercase();
            let is_input = tag == "input" || tag == "select";
            // Clicks are for buttons; inputs report on input, selects on change.
            let wanted = match kind {
                "click" => !is_input,
                "input" => tag == "input",
                _ => tag == "select",
            };
            if !wanted {
                return;
            }
            let value = if is_input { js_sys::Reflect::get(&el, &JsValue::from_str("value")).ok().and_then(|v| v.as_string()).unwrap_or_default() } else { String::new() };
            let act = el.get_attribute("data-act").unwrap_or_default();
            let v = el.get_attribute("data-v").unwrap_or_default();
            q.borrow_mut().push((act, v, value));
        });
        root.add_event_listener_with_callback(kind, cb.as_ref().unchecked_ref()).map_err(|_| "studio page: listen")?;
        cb.forget();
    }
    // The live preview's "what's new" bell (tools/studio/preview-overlay.js):
    // an entry clicked selects that material, asset, style or light.
    let q = queue.clone();
    let cb = Closure::<dyn FnMut(Event)>::new(move |e: Event| {
        let detail = js_sys::Reflect::get(&e, &JsValue::from_str("detail")).unwrap_or(JsValue::UNDEFINED);
        let field = |k: &str| js_sys::Reflect::get(&detail, &JsValue::from_str(k)).ok().and_then(|v| v.as_string()).unwrap_or_default();
        let act = match field("kind").as_str() {
            "material" => "mat",
            "asset" => "asset",
            "style" => "style",
            "lighting" => "light",
            _ => return,
        };
        q.borrow_mut().push((act.into(), field("id"), String::new()));
    });
    if let Some(win) = web_sys::window() {
        win.add_event_listener_with_callback("studio-preview-select", cb.as_ref().unchecked_ref()).map_err(|_| "studio page: listen")?;
    }
    cb.forget();
    Ok(())
}

/// Write `html` into `el` if `key` moved; an input focused inside keeps its
/// focus, text and caret.
fn put(doc: &Document, el: &Element, last: &mut String, key: String, html: impl FnOnce() -> String) {
    if *last == key {
        return;
    }
    *last = key;
    let focused = doc.active_element().filter(|a| el.contains(Some(a)));
    let held = focused.as_ref().and_then(|a| {
        let i = a.dyn_ref::<HtmlInputElement>()?;
        Some((a.get_attribute("data-act")?, i.value(), i.selection_start().ok().flatten()))
    });
    el.set_inner_html(&html());
    if let Some((act, value, caret)) = held
        && let Some(n) = el.query_selector(&format!("input[data-act=\"{act}\"]")).ok().flatten()
        && let Ok(i) = n.dyn_into::<HtmlInputElement>()
    {
        i.set_value(&value);
        let _ = i.focus();
        if let Some(c) = caret {
            let _ = i.set_selection_range(c, c);
        }
    }
}

/// `localStorage` through plain JS calls; a browser that refuses storage
/// just forgets.
fn local(method: &str, args: &[&str]) -> Option<JsValue> {
    let win = web_sys::window()?;
    let ls = js_sys::Reflect::get(&win, &JsValue::from_str("localStorage")).ok().filter(JsValue::is_object)?;
    let f: js_sys::Function = js_sys::Reflect::get(&ls, &JsValue::from_str(method)).ok()?.dyn_into().ok()?;
    let a = js_sys::Array::new();
    for x in args {
        a.push(&JsValue::from_str(x));
    }
    f.apply(&ls, &a).ok()
}

/// IndexedDB `evani-studio`, store `kv`, key `state`: `get` resolves to the
/// stored string or null, `put` stores `value`. Never throws: a refusal
/// resolves null.
const IDB_JS: &str = "return new Promise(function(res){try{var r=indexedDB.open('evani-studio',1);\
r.onupgradeneeded=function(){r.result.createObjectStore('kv')};r.onerror=function(){res(null)};\
r.onsuccess=function(){try{var db=r.result;var tx=db.transaction('kv',mode==='get'?'readonly':'readwrite');var st=tx.objectStore('kv');\
if(mode==='get'){var g=st.get('state');g.onsuccess=function(){res(typeof g.result==='string'?g.result:null)};g.onerror=function(){res(null)}}\
else{st.put(value,'state');tx.oncomplete=function(){res(true)};tx.onerror=function(){res(null)}}}catch(e){res(null)}}}catch(e){res(null)}});";

fn idb(mode: &str, value: &str) -> Option<js_sys::Promise> {
    let f = js_sys::Function::new_with_args("mode, value", IDB_JS);
    f.call2(&JsValue::NULL, &JsValue::from_str(mode), &JsValue::from_str(value)).ok()?.dyn_into().ok()
}

fn read_idb(stored: &Stored) {
    let Some(p) = idb("get", "") else {
        *stored.borrow_mut() = Some(None);
        return;
    };
    let s = stored.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let v = wasm_bindgen_futures::JsFuture::from(p).await.ok().and_then(|v| v.as_string());
        *s.borrow_mut() = Some(v);
    });
}

fn store(json: &str) {
    let _ = local("setItem", &[STORE_KEY, json]);
    let _ = idb("put", json);
}

/// Download `text` as file `name` (a data: link clicked once).
fn download(doc: &Document, name: &str, text: &str) -> bool {
    let Ok(a) = doc.create_element("a") else { return false };
    let href = format!("data:application/json;charset=utf-8,{}", String::from(js_sys::encode_uri_component(text)));
    let (Ok(()), Ok(())) = (a.set_attribute("href", &href), a.set_attribute("download", name)) else { return false };
    let Some(body) = doc.body() else { return false };
    if body.append_child(&a).is_err() {
        return false;
    }
    if let Some(h) = a.dyn_ref::<HtmlElement>() {
        h.click();
    }
    a.remove();
    true
}

/// Copy `text` to the clipboard (`navigator.clipboard.writeText`).
fn copy(text: &str) -> bool {
    let Some(win) = web_sys::window() else { return false };
    let get = |o: &JsValue, k: &str| js_sys::Reflect::get(o, &JsValue::from_str(k)).ok().filter(|v| !v.is_undefined() && !v.is_null());
    let Some(clip) = get(&win, "navigator").and_then(|n| get(&n, "clipboard")) else { return false };
    let Some(f) = get(&clip, "writeText").and_then(|f| f.dyn_into::<js_sys::Function>().ok()) else { return false };
    f.call1(&clip, &JsValue::from_str(text)).is_ok()
}

/// The record's material's switch `name` as shown: this combination's
/// edit, else the param while the bridge has the material open.
fn switch_value(world: &World, name: &str) -> Option<f32> {
    let studio = world.res::<Studio>();
    if let Some(Tweak::Num(x)) = studio.state.settings().and_then(|s| s.edits.get(name)).copied() {
        return Some(x);
    }
    let open = world.try_res::<Bridge>().is_some_and(|b| b.open_material() == Some(studio.state.selection.material.as_str()));
    if !open {
        return None;
    }
    let reg = world.res::<ParamRegistry>();
    match reg.id(name).and_then(|id| reg.value(id)) {
        Some(ParamValue::F32(x)) => Some(x),
        _ => None,
    }
}

/// Carry out one control.
fn run(world: &mut World, dom: &Dom, page: &mut Page, act: Act, data: &StudioData) {
    let say = |page: &mut Page, m: String| page.message = Some((m, MESSAGE_FRAMES));
    match act {
        Act::Cmd(c) => {
            page.touched = true;
            let mut s = world.res_mut::<Studio>();
            s.error = None;
            s.push(c);
        }
        Act::Switch { name, value } => {
            page.touched = true;
            let material = world.res::<Studio>().state.selection.material.clone();
            let open = world.res::<Studio>().active && world.try_res::<Bridge>().is_some_and(|b| b.open_material() == Some(material.as_str()));
            // While the material is open the param moves (the bridge keeps
            // it as this combination's edit); else the edit is stored.
            let set = open && {
                let mut reg = world.res_mut::<ParamRegistry>();
                reg.id(&name).is_some_and(|id| reg.set(id, ParamValue::F32(value)).is_ok())
            };
            if !set {
                world.res_mut::<Studio>().push(StudioCommand::Edit { name, value: Tweak::Num(value) });
            }
        }
        Act::Save => {
            page.touched = true;
            let name = pv::saved_name(&data.0, &world.res::<Studio>().state.selection);
            world.res_mut::<Studio>().push(StudioCommand::Save(name.clone()));
            say(page, format!("Saved to your presets: {name}"));
        }
        a @ (Act::Export | Act::CopyRecipe) => {
            let (json, file) = {
                let s = world.res::<Studio>();
                let keys = s.current.as_ref().map(|r| [format!("{:?}", r.geometry_key), format!("{:?}", r.variant_key), format!("{:?}", r.pass_key)]);
                (pv::recipe_json(&data.0, &s.state, keys), pv::recipe_file_name(&data.0, &s.state.selection))
            };
            page.ui.export_menu = false;
            let m = if matches!(a, Act::Export) {
                if download(&dom.doc, &file, &json) { format!("Recipe file downloaded: {file}") } else { "This browser refused the download".into() }
            } else if copy(&json) {
                "Recipe copied".into()
            } else {
                "This browser refused the clipboard".into()
            };
            say(page, m);
        }
        Act::Ui(UiAct::ScrollStyles(dir)) => {
            if let Some(row) = dom.styles.query_selector(".st-styles-row").ok().flatten() {
                let w = row.client_width();
                row.set_scroll_left(row.scroll_left() + f64::from(dir * (w * 7 / 10).max(80)));
            }
        }
        Act::Ui(u) => {
            if matches!(u, UiAct::AddMaterial) {
                page.focus_search = true;
            }
            pv::apply_ui(&mut page.ui, u);
        }
    }
}

/// The window's size, CSS px.
fn win_size() -> (f64, f64) {
    let Some(win) = web_sys::window() else { return (1.0, 1.0) };
    let num = |v: Result<JsValue, JsValue>| v.ok().and_then(|x| x.as_f64()).unwrap_or(1.0).max(1.0);
    (num(win.inner_width()), num(win.inner_height()))
}

fn frame(world: &mut World, dom: &Dom, page: &mut Page, queue: &Queue, stored: &Stored) {
    let Some(data) = world.try_res::<StudioData>().map(|d| d.clone()) else { return };
    if world.try_res::<Studio>().is_none() {
        return;
    }
    let c = data.0.clone();
    let (has_scene, scene) = world.try_res::<Viewer>().map_or((false, String::new()), |v| (v.families_on.iter().any(|f| f == SCENE), v.scene.clone()));
    if !page.started {
        page.started = true;
        // The stored record first (localStorage now, IndexedDB when it answers).
        if let Some(json) = local("getItem", &[STORE_KEY]).and_then(|v| v.as_string()) {
            world.res_mut::<Studio>().push(StudioCommand::Restore(json.clone()));
            page.local = Some(json);
        }
        read_idb(stored);
        if has_scene && let Some(mut v) = world.try_res_mut::<Viewer>() {
            v.push(Command::Scene(SCENE.into()));
        }
    }
    if let Some(found) = stored.borrow_mut().take().flatten()
        && !page.touched
        && page.local.as_deref() != Some(found.as_str())
    {
        world.res_mut::<Studio>().push(StudioCommand::Restore(found));
    }
    // The page shows while the studio scene is on (or always in a build
    // without it, where the view says the 3D scene is coming).
    let shown = !has_scene || scene == SCENE || scene.is_empty() || is_proc(&scene);
    if dom.root.hidden() == shown {
        dom.root.set_hidden(!shown);
        if let Some(b) = dom.doc.body() {
            let _ = if shown { b.class_list().add_1("studio-on") } else { b.class_list().remove_1("studio-on") };
        }
    }
    if !shown {
        return;
    }
    for (kind, v, input) in std::mem::take(&mut *queue.borrow_mut()) {
        // PROC: the procedural mode's own controls first.
        #[cfg(all(feature = "material-nature", feature = "material-playground"))]
        if let Some(m) = super::proc_page::handle(world, &mut page.proc, &kind, &v, &input) {
            page.touched = true;
            if let Some(m) = m {
                page.message = Some((m, MESSAGE_FRAMES));
            }
            continue;
        }
        if let Some(a) = pv::action(&kind, &v, &input) {
            run(world, dom, page, a, &data);
        }
    }
    // The stored state follows every change of the record.
    if let Some(json) = world.res_mut::<Studio>().storage_due() {
        store(&json);
    }

    // The layout from the room the page has.
    let (w, h) = win_size();
    let size = if w < 900.0 { "narrow" } else if w < 1220.0 { "mid" } else { "wide" };
    let proc = is_proc(&scene);
    let layout = format!("{size}{} dock-{}{}", if h < 640.0 { " short" } else { "" }, page.ui.dock, if proc { " proc" } else { "" });
    if layout != page.layout {
        dom.root.set_class_name(&layout);
        page.layout = layout;
    }
    for d in ["assets", "material", "combo"] {
        if let Some(e) = dom.tabs.query_selector(&format!("[data-v=\"{d}\"]")).ok().flatten() {
            let _ = e.class_list().toggle_with_force("on", page.ui.dock == d);
        }
    }

    // PROC: the procedural mode draws its own parts; the materials parts
    // are hidden and redrawn in full when the mode comes back.
    #[cfg(all(feature = "material-nature", feature = "material-playground"))]
    if super::proc_page::frame(world, &dom.proc, &mut page.proc) {
        page.keys = Default::default();
        page.values_at = (0, 0);
        proc_note(dom, page);
        return;
    }
    let (state, error) = {
        let s = world.res::<Studio>();
        (s.state.clone(), s.error.clone())
    };
    let sel = state.selection.clone();
    let lib = world.try_res::<LibraryData>().map(|l| l.0.clone());
    let ui = page.ui.clone();
    let k = &mut page.keys;

    put(&dom.doc, &dom.styles, &mut k[0], sel.style.clone(), || pv::styles_html(&c, &sel.style));
    put(&dom.doc, &dom.mats, &mut k[1], format!("{}|{}|{}|{}|{:?}", ui.mat_q, ui.mat_cat, ui.more, sel.key(), state.part), || {
        let chips = pv::chip_materials(&c, lib.as_deref(), &ui, &sel);
        pv::mats_html_for(&c, &ui, &sel, state.part.as_deref(), &chips)
    });
    put(&dom.doc, &dom.more, &mut k[2], format!("{}|{}|{}|{}", ui.more, ui.mat_cat, sel.material, sel.asset), || if ui.more { pv::more_html(&c, &ui, &sel) } else { String::new() });
    if dom.more.hidden() == ui.more {
        dom.more.set_hidden(!ui.more);
        if ui.more {
            let b = dom.mats.get_bounding_client_rect().bottom();
            let _ = dom.root.style().set_property("--st-top-b", &format!("{}px", (b + 6.0).round()));
        }
    }
    if std::mem::take(&mut page.focus_search)
        && let Some(i) = dom.mats.query_selector("input[data-act=\"mat-q\"]").ok().flatten().and_then(|e| e.dyn_into::<HtmlElement>().ok())
    {
        let _ = i.focus();
    }
    // The loader's word on the record's asset (WP2): loading, or why not.
    let source = world.try_res::<StudioSource>().filter(|s| s.asset == sel.asset).map_or_else(String::new, |s| match &s.state {
        SourceState::Loading => "Loading…".to_string(),
        SourceState::Failed(_) => "Didn't load".to_string(),
        SourceState::BeingBuilt { .. } | SourceState::Ready { .. } => String::new(),
    });
    put(&dom.doc, &dom.left, &mut k[3], format!("{:?}|{}|{source}", ui, sel.asset), || pv::assets_html(&c, &ui, &sel, &source));
    let coming = !has_scene;
    if let Some(b) = dom.doc.body()
        && b.class_list().contains("studio-coming") != coming
    {
        let _ = b.class_list().toggle_with_force("studio-coming", coming);
    }
    put(&dom.doc, &dom.view, &mut k[4], format!("{coming}|{}|{source}", sel.key()), || if coming { pv::coming_html(&c, &sel, &source) } else { String::new() });

    // The inspector: rebuilt when its rows change; values written each time
    // the record or the params move (never into the input in use).
    let mats = lib.as_deref().map(|l| pv::material_sliders(l, &sel.material, &|n| switch_value(world, n))).unwrap_or_default();
    let styles = pv::style_sliders(&c, &state);
    let lights: Vec<(String, String)> = world.try_res::<ViewerData>().map(|d| d.0.lights.presets.iter().map(|p| (p.id.clone(), p.name.clone())).collect()).unwrap_or_default();
    let tiers: Vec<(String, String)> = world.try_res::<Tiers>().map(|t| t.selectable().map(|(_, p)| (p.id.clone(), p.label.clone())).collect()).unwrap_or_default();
    let rows: Vec<&str> = mats.iter().chain(&styles).map(|s| s.key.as_str()).collect();
    put(&dom.doc, &dom.insp, &mut k[5], format!("{}|{:?}|{rows:?}|{}|{}|{}", sel.key(), state.part, state.view.light, state.view.quality, tiers.len()), || pv::inspector_html(&c, &state, &mats, &styles, &lights, &tiers));
    let at = (state.revision, world.res::<ParamRegistry>().revision());
    if at != page.values_at {
        page.values_at = at;
        let focused = dom.doc.active_element();
        for s in mats.iter().chain(&styles) {
            let txt = format!("{:.2}", s.value);
            if let Some(o) = dom.insp.query_selector(&format!("[data-out=\"{}\"]", s.key)).ok().flatten()
                && o.text_content().as_deref() != Some(txt.as_str())
            {
                o.set_text_content(Some(&txt));
            }
            if let Some(i) = dom.insp.query_selector(&format!("input[data-v=\"{}\"]", s.key)).ok().flatten()
                && focused.as_ref() != Some(&i)
                && let Ok(i) = i.dyn_into::<HtmlInputElement>()
                && (i.value_as_number() as f32 - s.value).abs() > 1e-4
            {
                i.set_value_as_number(f64::from(s.value));
            }
        }
    }
    let saved: Vec<&str> = state.saved.iter().map(|s| s.name.as_str()).collect();
    let (count, marked) = world.try_res::<StudioParams>().map_or((12, 3), |p| p.strip(&world.res::<ParamRegistry>()));
    let vkey = format!("{}|{:?}|{count}|{marked}", sel.key(), state.part);
    let skey = format!("{vkey}|{saved:?}|{}", ui.export_menu);
    // Worked out only when the side column or the strip is redrawn.
    let tiles = if k[6] == skey && k[7] == vkey { Vec::new() } else { variations_for(&c, &sel, state.part.as_deref(), count, marked) };
    put(&dom.doc, &dom.side, &mut k[6], skey, || pv::side_html(&c, &state, ui.export_menu, &tiles));
    put(&dom.doc, &dom.vars, &mut k[7], vkey, || pv::vars_html(&c, &sel, &tiles));

    // The notice line: the page's own message, else the record's notice.
    let note = match page.message.as_mut() {
        Some((m, left)) if *left > 0 => {
            *left -= 1;
            pv::esc(m)
        }
        _ => {
            page.message = None;
            pv::note_html(&c, &state, error.as_deref())
        }
    };
    if k[8] != note {
        dom.note.set_hidden(note.is_empty());
        dom.note.set_inner_html(&note);
        let tip = dom.note.text_content().unwrap_or_default();
        let _ = dom.note.set_attribute("title", &tip);
        k[8] = note;
    }
}

/// PROC: the procedural scene is on (the page's procedural mode).
fn is_proc(scene: &str) -> bool {
    #[cfg(all(feature = "material-nature", feature = "material-playground"))]
    {
        scene == super::proc_view::PROC_SCENE
    }
    #[cfg(not(all(feature = "material-nature", feature = "material-playground")))]
    {
        let _ = scene;
        false
    }
}

/// PROC: the page's own message (a copy) over the procedural view, for its
/// frames; the notice line is otherwise hidden in procedural mode.
#[cfg(all(feature = "material-nature", feature = "material-playground"))]
fn proc_note(dom: &Dom, page: &mut Page) {
    let note = match page.message.as_mut() {
        Some((m, left)) if *left > 0 => {
            *left -= 1;
            pv::esc(m)
        }
        _ => {
            page.message = None;
            String::new()
        }
    };
    if dom.note.inner_html() != note {
        dom.note.set_hidden(note.is_empty());
        dom.note.set_inner_html(&note);
    }
}

/// Mount the page and add its frame system (just before the kit's studio
/// system, so what the page asks for is applied the same frame).
pub fn install(ctx: &mut PluginContext) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    let queue: Queue = Rc::new(RefCell::new(Vec::new()));
    let dom = match make(&doc, &queue) {
        Ok(d) => d,
        Err(e) => {
            web_sys::console::error_1(&JsValue::from_str(&format!("material-view: {e}")));
            return;
        }
    };
    let stored: Stored = Rc::new(RefCell::new(None));
    let mut page = Page::default();
    ctx.system(SystemDesc::new("material_view.studio_page", Stage::Frame, move |world: &mut World| frame(world, &dom, &mut page, &queue, &stored)).order(-9));
}
