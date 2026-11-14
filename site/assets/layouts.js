// The "Binary layout" page: each <figure class="anatomy layout" data-example="…"> shows one
// message encoded by gravenc at site build time (layouts.json, made by site/layouts.py),
// with every byte range explained.
(function () {
  const url = new URL("layouts.json", document.currentScript.src);
  const figures = document.querySelectorAll("figure.layout[data-example]");
  if (!figures.length) return;

  fetch(url)
    .then((r) => r.json())
    .then((all) => figures.forEach((fig) => render(fig, all[fig.dataset.example])))
    .catch(() => figures.forEach((fig) => (fig.querySelector(".explain").textContent = "The example could not be loaded.")));

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined) e.textContent = text;
    return e;
  }

  function render(fig, ex) {
    const bytes = ex.hex.split(" ");
    const ranges = ex.ranges.map((r, i) => ({ id: String(i), first: r[0], last: r[1], group: r[2], label: r[3], desc: r[4] }));
    const intro = `${bytes.length} bytes, as written by the reference implementation. Point at a byte or a label to see what it encodes.`;

    const json = el("pre", "schema", JSON.stringify(ex.json, null, 1).replace(/\n\s*/g, " "));
    json.setAttribute("aria-label", "The message as JSON");
    const hex = el("div", "hex");
    hex.setAttribute("role", "group");
    hex.setAttribute("aria-label", `The ${bytes.length} bytes of the message`);
    const explain = el("p", "explain");
    explain.setAttribute("aria-live", "polite");
    const legend = el("ul", "legend");
    legend.setAttribute("aria-label", "Byte ranges");

    bytes.forEach((b, i) => {
      const f = ranges.find((r) => i >= r.first && i <= r.last);
      const s = el("span", "byte g-" + f.group, b);
      s.dataset.f = f.id;
      s.title = `byte ${i}`;
      if (i === f.first) s.classList.add("first");
      if (i === f.last) s.classList.add("last");
      hex.appendChild(s);
    });

    ranges.forEach((f) => {
      const li = el("li");
      const btn = el("button", "g-" + f.group);
      btn.type = "button";
      btn.dataset.f = f.id;
      btn.append(el("span", "sw"), el("span", "nm", f.label), el("span", "rg", f.first === f.last ? String(f.first) : f.first + "–" + f.last));
      li.appendChild(btn);
      legend.appendChild(li);
    });

    fig.querySelectorAll(".explain").forEach((e) => e.remove());
    fig.append(json, hex, explain, legend);

    function show(id) {
      fig.classList.toggle("focus", id !== null);
      fig.querySelectorAll("[data-f]").forEach((e) => e.classList.toggle("on", e.dataset.f === id));
      const f = id !== null && ranges[Number(id)];
      explain.textContent = f ? `${f.label} (${f.first === f.last ? "byte " + f.first : "bytes " + f.first + "–" + f.last}): ${f.desc}` : intro;
    }
    const pick = (e) => {
      const t = e.target.closest("[data-f]");
      if (t) show(t.dataset.f);
    };
    fig.addEventListener("mouseover", pick);
    fig.addEventListener("mouseleave", () => show(null));
    legend.addEventListener("focusin", pick);
    legend.addEventListener("focusout", () => show(null));
    legend.addEventListener("click", pick);
    show(null);
  }
})();
