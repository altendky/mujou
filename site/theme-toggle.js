/*
 * Shared theme toggle for mujou.
 *
 * Cycles: system → light → dark → system.
 * Persists choice to localStorage.
 * Looks for a button with class "theme-toggle" in the DOM.
 *
 * Early flash prevention is handled by theme-detect.js which must
 * run in <head> before any CSS is applied.  This script handles the
 * interactive toggle button and live OS preference tracking.
 */

(function () {
  var modes = ["system", "light", "dark"];
  var darkQuery = matchMedia("(prefers-color-scheme: dark)");

  // Lucide icons (ISC license) — consistent stroke-based set.
  // https://lucide.dev
  var svg = function (body) {
    return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' + body + "</svg>";
  };
  var icons = {
    // Lucide sun-moon: system / auto theme
    system: svg('<path d="M12 8a2.83 2.83 0 0 0 4 4 4 4 0 1 1-4-4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.9 4.9 1.4 1.4"/><path d="m17.7 17.7 1.4 1.4"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.3 17.7-1.4 1.4"/><path d="m19.1 4.9-1.4 1.4"/>'),
    // Lucide sun: light mode
    light: svg('<circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/>'),
    // Lucide moon: dark mode
    dark: svg('<path d="M20.985 12.486a9 9 0 1 1-9.473-9.472c.405-.022.617.46.402.803a6 6 0 0 0 8.268 8.268c.344-.215.825-.004.803.401"/>')
  };
  var labels = { system: "System theme", light: "Light theme", dark: "Dark theme" };

  var stored = null;
  try { stored = localStorage.getItem("theme"); } catch (e) {}
  var mode = (stored && modes.indexOf(stored) !== -1) ? stored : "system";

  // Resolve the effective data-theme value for a given mode.
  function resolve(m) {
    if (m === "system") {
      return darkQuery.matches ? "dark" : "light";
    }
    return m;
  }

  function apply(m) {
    mode = m;
    var resolved = resolve(m);
    document.documentElement.setAttribute("data-theme", resolved);
    try { localStorage.setItem("theme", m); } catch (e) {}
    if (window.__mujou_theme_changed) window.__mujou_theme_changed(resolved);

    // Update all toggle buttons on the page.
    var buttons = document.querySelectorAll(".theme-toggle");
    for (var i = 0; i < buttons.length; i++) {
      buttons[i].innerHTML = icons[m];
      buttons[i].title = labels[m];
      buttons[i].setAttribute("aria-label", labels[m]);
    }
  }

  // When the OS preference changes and the user is in "system" mode,
  // update the resolved theme live.
  darkQuery.addEventListener("change", function () {
    if (mode === "system") {
      apply("system");
    }
  });

  // Delegate click on the document so dynamically-rendered buttons
  // (e.g. Dioxus WASM re-renders) always work without re-wiring.
  document.addEventListener("click", function (e) {
    if (e.target.closest(".theme-toggle")) {
      apply(modes[(modes.indexOf(mode) + 1) % modes.length]);
    }
  });

  // Apply current mode to any existing toggle buttons.
  function init() {
    apply(mode);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
