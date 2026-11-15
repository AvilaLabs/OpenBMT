// The Android system chrome as the emulator draws it at 1080 by 2400 px and density 2.625 (412 by 915 dp): the
// status bar (clock, the screenshot icon, wifi, signal, battery) and the 3-button navigation bar. The sizes and
// colours are measured from real emulator screenshots: the status bar is 128 px (48.76 dp) high, the navigation
// bar 126 px (48 dp); the icons are #666666 on a light bar and white on a dark one, the navigation icons
// #44464f (light) and #c5c6d0 (dark).
"use strict";
(function (root) {
  const U = root.OpenBmtUi || require("./ui.js");
  const { css } = U;

  const DENSITY = 2.625;
  const WIDTH_DP = 412;
  const HEIGHT_DP = 915;
  const STATUS_DP = 128 / DENSITY;
  const NAV_DP = 126 / DENSITY;

  /** The status bar's icons: [light] is true for dark icons on a light bar (the platform's "light status bar"). */
  function statusBar(light, width = WIDTH_DP) {
    const r = width - WIDTH_DP;
    const ink = light ? "#666666" : "#ffffff";
    const el = document.createElement("div");
    el.className = "statusbar";
    el.style.height = STATUS_DP + "px";
    el.style.color = ink;
    el.innerHTML = `<div class="time" style="height:${STATUS_DP}px;line-height:${STATUS_DP}px">3:54</div>`
      + `<svg class="left-icon" style="position:absolute;left:64px;top:${STATUS_DP / 2 - 6.5}px" width="13" height="13" viewBox="0 0 13 13"><rect x="3.5" y="0.75" width="8.5" height="8.5" rx="1.2" fill="none" stroke="${ink}" stroke-width="1.5"/><path d="M1 3.5V11A1.2 1.2 0 0 0 2.2 12.2H9.7" fill="none" stroke="${ink}" stroke-width="1.5"/></svg>`
      + `<svg style="position:absolute;left:${340 + r}px;top:${STATUS_DP / 2 - 6.5}px" width="15" height="13" viewBox="0 0 15 13"><path d="M7.5 12.6L0.2 3.4A11.5 11.5 0 0 1 14.8 3.4Z" fill="${ink}"/></svg>`
      + `<svg style="position:absolute;left:${356 + r}px;top:${STATUS_DP / 2 - 5.5}px" width="12" height="11" viewBox="0 0 12 11"><path d="M12 0V11H0Z" fill="${ink}"/></svg>`
      + `<svg style="position:absolute;left:${375 + r}px;top:${STATUS_DP / 2 - 6.5}px" width="8" height="13" viewBox="0 0 8 13"><rect x="0.5" y="1.6" width="7" height="11" rx="1.2" fill="${ink}"/><rect x="2.4" y="0" width="3.2" height="1.7" rx="0.5" fill="${ink}"/><path d="M4.6 3.8L2.6 7.4H4.1L3.4 10.8L5.6 6.8H4.1Z" fill="${light ? "#f4f5f7" : "#171a1f"}"/></svg>`;
    return el;
  }

  /** The 3-button navigation bar: back, home and recents. */
  function navBar(light, width = WIDTH_DP) {
    const x = (width - WIDTH_DP) / 2;
    const ink = light ? "#44464f" : "#c5c6d0";
    const el = document.createElement("div");
    el.className = "navbar";
    el.style.height = NAV_DP + "px";
    const cy = 24;
    el.innerHTML = `<svg style="left:0;top:0" width="${width}" height="${NAV_DP}" viewBox="${-x} 0 ${width} ${NAV_DP}">`
      + `<path d="M104.4 ${cy - 8.6}Q104.4 ${cy - 9.4} 103.7 ${cy - 9}L92.2 ${cy - 1.2}Q91.2 ${cy} 92.2 ${cy + 1.2}L103.7 ${cy + 9}Q104.4 ${cy + 9.4} 104.4 ${cy + 8.6}Z" fill="${ink}" stroke="${ink}" stroke-width="1.4" stroke-linejoin="round"/>`
      + `<circle cx="205.7" cy="${cy}" r="8.4" fill="${ink}"/>`
      + `<rect x="304.4" y="${cy - 7.6}" width="15.2" height="15.2" rx="2.4" fill="${ink}"/></svg>`;
    return el;
  }

  /**
   * A phone's screen of 412 by 915 dp: the page fills it and the bars are drawn over it. [ui] is the Ui that made
   * the page: its window says whether the bars' icons are light or dark.
   */
  function Device(ui, page) {
    const dev = document.createElement("div");
    dev.className = "device";
    const win = document.createElement("div");
    win.className = "window";
    win.style.backgroundColor = css(U.applyNight(ui));
    page.style.flex = "1 1 0";
    win.appendChild(page);
    dev.appendChild(win);
    const w = ui.context.width || WIDTH_DP, h = ui.context.height || HEIGHT_DP;
    if (w !== WIDTH_DP || h !== HEIGHT_DP) { dev.style.width = w + "px"; dev.style.height = h + "px"; }
    dev.appendChild(statusBar(ui.window.statusLight, w));
    dev.appendChild(navBar(ui.window.navLight, w));
    return dev;
  }

  /**
   * The Ui of a device in light or dark, over [palette]: the emulator's phone, or [size] {width, height, density}
   * in dp, such as an 8-inch tablet of 800 by 1280 px at density 1.33 (601 by 962 dp).
   */
  function deviceUi(palette, night, logoUrl, size) {
    const z = size || { width: WIDTH_DP, height: HEIGHT_DP, density: DENSITY };
    return new U.Ui({
      palette, night, density: z.density, insets: [0, STATUS_DP, 0, NAV_DP], window: { statusLight: false, navLight: false },
      width: z.width, height: z.height, logoUrl,
    });
  }

  /** The two counter devices of the demo. */
  const PHONE = { width: WIDTH_DP, height: HEIGHT_DP, density: DENSITY };
  const TABLET = { width: 601, height: 962, density: 4 / 3 };

  const api = { DENSITY, WIDTH_DP, HEIGHT_DP, STATUS_DP, NAV_DP, PHONE, TABLET, statusBar, navBar, Device, deviceUi };
  root.OpenBmtChrome = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(typeof globalThis !== "undefined" ? globalThis : this);
