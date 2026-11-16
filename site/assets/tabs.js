// Synchronize Rust / JS-TS code tabs across the landing page and persist choice.
(function () {
  const KEY = "graven-lang";
  const get = () => {
    try { return localStorage.getItem(KEY); } catch (e) { return null; }
  };
  const set = (v) => {
    try { localStorage.setItem(KEY, v); } catch (e) {}
  };

  const groups = [...document.querySelectorAll(".code-tabs")];
  if (!groups.length) return;

  const activateAll = (lang) => {
    groups.forEach((g) => {
      const tabs = [...g.querySelectorAll(".code-tabbar [role=tab]")];
      const target = tabs.find((t) => t.dataset.tab === lang) || tabs[0];
      if (!target) return;
      const active = target.dataset.tab;
      tabs.forEach((t) => t.setAttribute("aria-selected", String(t.dataset.tab === active)));
      g.querySelectorAll(".code-panel").forEach((p) => (p.hidden = p.dataset.tab !== active));
    });
  };

  groups.forEach((g) => {
    g.classList.add("js");
    const bar = g.querySelector(".code-tabbar");
    if (bar) bar.hidden = false;
    g.querySelectorAll("[role=tab]").forEach((btn) => {
      btn.addEventListener("click", () => {
        set(btn.dataset.tab);
        activateAll(btn.dataset.tab);
      });
    });
  });

  activateAll(get());
})();
