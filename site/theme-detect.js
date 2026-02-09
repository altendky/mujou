/*
 * Early theme detection for mujou.
 *
 * Must run in <head> before any CSS is applied to prevent a flash of
 * the wrong theme.  Sets data-theme="light" or data-theme="dark" on
 * <html> immediately based on localStorage + system preference.
 */
(function () {
  var mode = localStorage.getItem("theme") || "system";
  var dark =
    mode === "dark" ||
    (mode === "system" &&
      matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.setAttribute("data-theme", dark ? "dark" : "light");
})();
