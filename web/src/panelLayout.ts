export const MIN_PANEL_WIDTH=230;
export const MIN_CHAT_WIDTH=480;
export const ACTIVITY_WIDTH=52;
export const RESIZER_WIDTH=6;
export const OVERLAY_BREAKPOINT=800;

export function clampPanelWidth(width:number,viewportWidth:number):number{
  if(viewportWidth<=OVERLAY_BREAKPOINT)return Math.min(width,Math.max(0,viewportWidth-48));
  return Math.max(MIN_PANEL_WIDTH,Math.min(width,viewportWidth-ACTIVITY_WIDTH-RESIZER_WIDTH-MIN_CHAT_WIDTH));
}
