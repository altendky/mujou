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

  var svg = function (body) {
    return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' + body + "</svg>";
  };
  var icons = {
    // Half-circle: system / auto
    system: svg('<circle cx="12" cy="12" r="9" fill="currentColor" stroke="none"/><path d="M12 3a9 9 0 0 1 0 18z" fill="var(--bg)" stroke="none"/>'),
    // Sun: light mode
    light: svg('<circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/>'),
    // Moon: dark mode
    dark: svg('<path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/>')
  };
  var labels = { system: "System theme", light: "Light theme", dark: "Dark theme" };

  var mode = localStorage.getItem("theme") || "system";

  // Resolve the effective data-theme value for a given mode.
  function resolve(m) {
    if (m === "system") {
      return darkQuery.matches ? "dark" : "light";
    }
    return m;
  }

  function apply(m) {
    mode = m;
    document.documentElement.setAttribute("data-theme", resolve(m));
    localStorage.setItem("theme", m);

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

  // Wire up toggle buttons once they exist in the DOM.
  // We track which buttons have already been wired to avoid duplicate listeners.
  var wired = new WeakSet();

  function wireButtons() {
    var buttons = document.querySelectorAll(".theme-toggle");
    var found = false;
    for (var i = 0; i < buttons.length; i++) {
      found = true;
      if (!wired.has(buttons[i])) {
        wired.add(buttons[i]);
        buttons[i].addEventListener("click", function () {
          apply(modes[(modes.indexOf(mode) + 1) % modes.length]);
        });
      }
    }
    if (found) {
      apply(mode);
    }
    return found;
  }

  // Try immediately, then observe for dynamically-rendered buttons
  // (e.g. Dioxus WASM mounts after DOMContentLoaded).
  function init() {
    if (wireButtons()) return;

    var observer = new MutationObserver(function () {
      if (wireButtons()) {
        observer.disconnect();
      }
    });
    observer.observe(document.body || document.documentElement, {
      childList: true,
      subtree: true,
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
