// The hero diagram: a real enveloped `bank.Entry` message (produced by the Rust
// runtime) with every byte range explained. Texts follow the page's `lang`.
(function () {
  const hex =
    "47 52 56 4e 01 00 00 00 65 54 62 9e f3 a2 ab f3 23 00 00 00 15 00 03 e9 03 00 00 00 00 00 00 " +
    "3c f6 ff ff ff ff ff ff 04 00 00 00 08 00 00 00 74 72 61 6e 73 66 65 72 2d 69 0b 71";
  const bytes = hex.split(" ");

  // [id, first byte, last byte, group]
  const ranges = [
    ["magic", 0, 3, "env"], ["version", 4, 4, "env"], ["flags", 5, 7, "env"], ["sid", 8, 15, "env"],
    ["plen", 16, 19, "env"], ["flen", 20, 21, "tbl"], ["bits", 22, 22, "tbl"], ["account", 23, 30, "val"],
    ["amount", 31, 38, "val"], ["memo", 39, 42, "ptr"], ["memodata", 43, 54, "ptr"], ["crc", 55, 58, "sum"],
  ];

  const T = {
    en: {
      intro: "An enveloped Entry, 59 bytes. Point at a field, a schema line or a byte to see what it encodes.",
      magic: ["Magic", "The four bytes GRVN mark the start of a Graven envelope."],
      version: ["Format version", "Format version 1. A reader rejects versions it does not know."],
      flags: ["Flags, reserved", "Always zero in v1, and checked: a non-zero byte is rejected."],
      sid: ["Schema id", "Identifies bank.Entry. It is recorded in the lockfile on the first build and never changes afterwards."],
      plen: ["Payload length", "The message that follows is 35 bytes long."],
      flen: ["Table length", "The table's fixed part is 21 bytes. A newer reader with a longer layout sees the missing fields as absent."],
      bits: ["Presence bits", "account and amount are present. A present zero is different from an absent field."],
      account: ["account", "1001 as a little-endian u64, read in place."],
      amount: ["amount", "−2500 as an i64. No alignment and no padding."],
      memo: ["memo pointer", "A relative pointer, +4, to the string's data."],
      memodata: ["memo data", "A u32 length and the UTF-8 bytes of \"transfer\", borrowed straight from the buffer."],
      crc: ["CRC32C", "Covers every byte before it. A cut-off write reads as incomplete, a flipped bit as corruption."],
    },
    "pt-BR": {
      intro: "Um Entry com envelope, 59 bytes. Aponte para um campo, uma linha do schema ou um byte para ver o que ele codifica.",
      magic: ["Magic", "Os quatro bytes GRVN marcam o início de um envelope Graven."],
      version: ["Versão do formato", "Versão 1 do formato. O leitor recusa versões que não conhece."],
      flags: ["Flags, reservado", "Sempre zero na v1, e conferido: um byte diferente de zero é recusado."],
      sid: ["Schema id", "Identifica bank.Entry. É gravado no lockfile no primeiro build e nunca mais muda."],
      plen: ["Tamanho do payload", "A mensagem que vem a seguir tem 35 bytes."],
      flen: ["Tamanho da tabela", "A parte fixa da tabela tem 21 bytes. Um leitor mais novo, com layout maior, vê os campos que faltam como ausentes."],
      bits: ["Bits de presença", "account e amount estão presentes. Um zero presente é diferente de um campo ausente."],
      account: ["account", "1001 como u64 little-endian, lido no lugar."],
      amount: ["amount", "−2500 como i64. Sem alinhamento e sem padding."],
      memo: ["ponteiro de memo", "Um ponteiro relativo, +4, até os dados da string."],
      memodata: ["dados de memo", "Um tamanho u32 e os bytes UTF-8 de \"transfer\", emprestados direto do buffer."],
      crc: ["CRC32C", "Cobre todos os bytes anteriores. Uma escrita interrompida aparece como incompleta; um bit trocado, como corrupção."],
    },
    es: {
      intro: "Un Entry con sobre, 59 bytes. Señala un campo, una línea del esquema o un byte para ver qué codifica.",
      magic: ["Magic", "Los cuatro bytes GRVN marcan el inicio de un sobre Graven."],
      version: ["Versión del formato", "Versión 1 del formato. El lector rechaza las versiones que no conoce."],
      flags: ["Flags, reservado", "Siempre cero en la v1, y se comprueba: un byte distinto de cero se rechaza."],
      sid: ["Schema id", "Identifica bank.Entry. Se registra en el lockfile en la primera compilación y nunca cambia después."],
      plen: ["Longitud del payload", "El mensaje que sigue mide 35 bytes."],
      flen: ["Longitud de la tabla", "La parte fija de la tabla mide 21 bytes. Un lector más nuevo, con un layout más largo, ve los campos que faltan como ausentes."],
      bits: ["Bits de presencia", "account y amount están presentes. Un cero presente es distinto de un campo ausente."],
      account: ["account", "1001 como u64 little-endian, leído en su lugar."],
      amount: ["amount", "−2500 como i64. Sin alineación y sin relleno."],
      memo: ["puntero de memo", "Un puntero relativo, +4, hasta los datos de la cadena."],
      memodata: ["datos de memo", "Una longitud u32 y los bytes UTF-8 de \"transfer\", tomados directamente del buffer."],
      crc: ["CRC32C", "Cubre todos los bytes anteriores. Una escritura cortada se lee como incompleta; un bit alterado, como corrupción."],
    },
    "zh-CN": {
      intro: "一条带信封的 Entry 消息，共 59 字节。将指针移到字段、schema 行或某个字节上，查看它编码的内容。",
      magic: ["魔数", "四个字节 GRVN 标记一个 Graven 信封的开始。"],
      version: ["格式版本", "格式版本 1。读取方会拒绝不认识的版本。"],
      flags: ["标志位与保留位", "在 v1 中恒为零，并且会被检查：非零字节会被拒绝。"],
      sid: ["Schema id", "标识 bank.Entry。它在首次构建时记录到 lockfile 中，之后不再改变。"],
      plen: ["负载长度", "随后的消息长 35 字节。"],
      flen: ["表长度", "该表的固定部分为 21 字节。布局更长的新版读取方会把缺少的字段视为不存在。"],
      bits: ["存在位", "account 和 amount 存在。存在的零值与不存在的字段是不同的。"],
      account: ["account", "1001，小端序 u64，原地读取。"],
      amount: ["amount", "−2500，i64。没有对齐，也没有填充。"],
      memo: ["memo 指针", "一个相对指针（+4），指向字符串数据。"],
      memodata: ["memo 数据", "一个 u32 长度加上 \"transfer\" 的 UTF-8 字节，直接借用自缓冲区。"],
      crc: ["CRC32C", "覆盖其之前的所有字节。被截断的写入会被识别为不完整，翻转的比特会被识别为损坏。"],
    },
  };
  const lang = document.documentElement.lang;
  const t = T[lang] || T.en;
  const sep = lang === "zh-CN" ? "：" : ". ";

  const linked = { memo: ["memo", "memodata"], sid: ["sid"] };
  const hexEl = document.getElementById("hex");
  const legendEl = document.getElementById("legend");
  const explainEl = document.getElementById("explain");
  const rangeOf = (i) => ranges.find((f) => i >= f[1] && i <= f[2]);

  bytes.forEach((b, i) => {
    const f = rangeOf(i);
    const s = document.createElement("span");
    s.className = "byte g-" + f[3];
    s.textContent = b;
    s.dataset.f = f[0];
    if (i === f[1]) s.classList.add("first");
    if (i === f[2]) s.classList.add("last");
    hexEl.appendChild(s);
  });

  ranges.forEach((f) => {
    const li = document.createElement("li");
    const btn = document.createElement("button");
    btn.type = "button";
    btn.dataset.f = f[0];
    btn.className = "g-" + f[3];
    btn.innerHTML = '<span class="sw"></span><span class="nm"></span><span class="rg"></span>';
    btn.querySelector(".nm").textContent = t[f[0]][0];
    btn.querySelector(".rg").textContent = f[1] === f[2] ? String(f[1]) : f[1] + "–" + f[2];
    li.appendChild(btn);
    legendEl.appendChild(li);
  });

  const root = document.querySelector(".anatomy");
  function show(ids) {
    root.classList.toggle("focus", !!ids);
    document.querySelectorAll(".anatomy [data-f]").forEach((el) => {
      el.classList.toggle("on", !!ids && ids.includes(el.dataset.f));
    });
    explainEl.textContent = ids ? ids.map((id) => t[id][0] + sep + t[id][1]).join(" ") : t.intro;
  }
  const idsFor = (id) => linked[id] || [id];

  root.addEventListener("mouseover", (e) => {
    const el = e.target.closest("[data-f]");
    if (el) show(idsFor(el.dataset.f));
  });
  root.addEventListener("mouseleave", () => show(null));
  legendEl.addEventListener("focusin", (e) => {
    const el = e.target.closest("[data-f]");
    if (el) show(idsFor(el.dataset.f));
  });
  legendEl.addEventListener("focusout", () => show(null));
  legendEl.addEventListener("click", (e) => {
    const el = e.target.closest("[data-f]");
    if (el) show(idsFor(el.dataset.f));
  });
  show(null);
})();
