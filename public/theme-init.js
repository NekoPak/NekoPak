// Applies saved theme + accent before first paint (no inline script, CSP-friendly).
try {
  var t = localStorage.getItem("nekopak.theme");
  if (t === "light" || t === "dark") document.documentElement.setAttribute("data-theme", t);
  var a = localStorage.getItem("nekopak.accent");
  document.documentElement.setAttribute("data-accent", a && /^[a-z]+$/.test(a) ? a : "mist");
} catch (e) {}
