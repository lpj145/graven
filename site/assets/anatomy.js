// The hero diagram: one real `bank.Entry` message (the same 35 bytes from the Rust and the
// JavaScript writers) in its two containers: on disk inside an envelope, on the wire inside
// an RPC frame. Every byte range is explained. Texts follow the page's `lang`.
(function () {
  const message =
    "15 00 03 e9 03 00 00 00 00 00 00 3c f6 ff ff ff ff ff ff 04 00 00 00 08 00 00 00 " +
    "74 72 61 6e 73 66 65 72";
  // The message's ranges, from its first byte. [id, first, last, group]
  const msgRanges = [
    ["flen", 0, 1, "tbl"], ["bits", 2, 2, "tbl"], ["account", 3, 10, "val"], ["amount", 11, 18, "val"],
    ["memo", 19, 22, "ptr"], ["memodata", 23, 34, "ptr"],
  ];
  const at = (off) => msgRanges.map(([id, a, b, g]) => [id, a + off, b + off, g]);
  const views = {
    // Envelope: magic, version, flags, schema id, payload length, the message, CRC32C.
    disk: {
      bytes: ("47 52 56 4e 01 00 00 00 65 54 62 9e f3 a2 ab f3 23 00 00 00 " + message + " 2d 69 0b 71").split(" "),
      ranges: [
        ["magic", 0, 3, "env"], ["version", 4, 4, "env"], ["flags", 5, 7, "env"], ["sid", 8, 15, "env"],
        ["plen", 16, 19, "env"], ...at(20), ["crc", 55, 58, "sum"],
      ],
    },
    // An `Ok` reply frame: version and kind, variant, length, the message.
    wire: {
      bytes: ("12 00 00 23 00 00 00 " + message).split(" "),
      ranges: [["vk", 0, 0, "env"], ["variant", 1, 2, "env"], ["len", 3, 6, "env"], ...at(7)],
    },
  };

  const T = {
    en: {
      tabDisk: ["On disk", "59 bytes"],
      tabWire: ["On the wire", "42 bytes"],
      introWire: "The same message as a service reply, 42 bytes. The call already says what it is, and TLS keeps it intact, so only its kind and length travel with it.",
      vk: ["Version and kind", "Frame version 1 and kind 2, an Ok reply, in one byte."],
      variant: ["Variant", "The union tag when the reply type is a union; 0 for a table like Entry."],
      len: ["Message length", "The message that follows is 35 bytes long. No schema id and no CRC: the call knows the type, the transport keeps the bytes."],
      intro: "The message stored on disk, 59 bytes: the envelope says what it is and whether it survived. Point at a field, a schema line or a byte to see what it encodes.",
      magic: ["Magic", "The four bytes GRVN mark the start of a Graven envelope."],
      version: ["Format version", "Format version 1. A reader rejects versions it does not know."],
      flags: ["Flags, reserved", "Always zero in v1, and checked: a non-zero byte is rejected."],
      sid: ["Schema id", "Identifies bank.Entry. It is recorded in the lockfile on the first build and never changes afterwards."],
      plen: ["Payload length", "The message that follows is 35 bytes long: the same 35 bytes the wire view sends."],
      flen: ["Table length", "The table's fixed part is 21 bytes. A newer reader with a longer layout sees the missing fields as absent."],
      bits: ["Presence bits", "account and amount are present. A present zero is different from an absent field."],
      account: ["account", "1001 as a little-endian u64, read in place."],
      amount: ["amount", "−2500 as an i64. No alignment and no padding."],
      memo: ["memo pointer", "A relative pointer, +4, to the string's data."],
      memodata: ["memo data", "A u32 length and the UTF-8 bytes of \"transfer\", borrowed straight from the buffer."],
      crc: ["CRC32C", "Covers every byte before it. A cut-off write reads as incomplete, a flipped bit as corruption."],
    },
    "pt-BR": {
      tabDisk: ["No disco", "59 bytes"],
      tabWire: ["Na rede", "42 bytes"],
      introWire: "A mesma mensagem como resposta de um serviço, 42 bytes. A chamada já diz o que ela é, e o TLS a mantém intacta, então só o tipo e o tamanho viajam junto.",
      vk: ["Versão e tipo", "Versão 1 do frame e tipo 2, uma resposta Ok, num byte só."],
      variant: ["Variante", "A tag da union quando a resposta é uma union; 0 para uma tabela como Entry."],
      len: ["Tamanho da mensagem", "A mensagem que vem a seguir tem 35 bytes. Sem schema id e sem CRC: a chamada sabe o tipo, o transporte cuida dos bytes."],
      intro: "A mensagem guardada em disco, 59 bytes: o envelope diz o que ela é e se sobreviveu. Aponte para um campo, uma linha do schema ou um byte para ver o que ele codifica.",
      magic: ["Magic", "Os quatro bytes GRVN marcam o início de um envelope Graven."],
      version: ["Versão do formato", "Versão 1 do formato. O leitor recusa versões que não conhece."],
      flags: ["Flags, reservado", "Sempre zero na v1, e conferido: um byte diferente de zero é recusado."],
      sid: ["Schema id", "Identifica bank.Entry. É gravado no lockfile no primeiro build e nunca mais muda."],
      plen: ["Tamanho do payload", "A mensagem que vem a seguir tem 35 bytes: os mesmos 35 bytes que a visão de rede envia."],
      flen: ["Tamanho da tabela", "A parte fixa da tabela tem 21 bytes. Um leitor mais novo, com layout maior, vê os campos que faltam como ausentes."],
      bits: ["Bits de presença", "account e amount estão presentes. Um zero presente é diferente de um campo ausente."],
      account: ["account", "1001 como u64 little-endian, lido no lugar."],
      amount: ["amount", "−2500 como i64. Sem alinhamento e sem padding."],
      memo: ["ponteiro de memo", "Um ponteiro relativo, +4, até os dados da string."],
      memodata: ["dados de memo", "Um tamanho u32 e os bytes UTF-8 de \"transfer\", emprestados direto do buffer."],
      crc: ["CRC32C", "Cobre todos os bytes anteriores. Uma escrita interrompida aparece como incompleta; um bit trocado, como corrupção."],
    },
    es: {
      tabDisk: ["En disco", "59 bytes"],
      tabWire: ["En la red", "42 bytes"],
      introWire: "El mismo mensaje como respuesta de un servicio, 42 bytes. La llamada ya dice qué es, y TLS lo mantiene intacto, así que solo viajan con él su tipo y su longitud.",
      vk: ["Versión y tipo", "Versión 1 del frame y tipo 2, una respuesta Ok, en un solo byte."],
      variant: ["Variante", "La etiqueta de la union cuando la respuesta es una union; 0 para una tabla como Entry."],
      len: ["Longitud del mensaje", "El mensaje que sigue mide 35 bytes. Sin schema id y sin CRC: la llamada conoce el tipo, el transporte cuida los bytes."],
      intro: "El mensaje guardado en disco, 59 bytes: el sobre dice qué es y si sobrevivió. Señala un campo, una línea del esquema o un byte para ver qué codifica.",
      magic: ["Magic", "Los cuatro bytes GRVN marcan el inicio de un sobre Graven."],
      version: ["Versión del formato", "Versión 1 del formato. El lector rechaza las versiones que no conoce."],
      flags: ["Flags, reservado", "Siempre cero en la v1, y se comprueba: un byte distinto de cero se rechaza."],
      sid: ["Schema id", "Identifica bank.Entry. Se registra en el lockfile en la primera compilación y nunca cambia después."],
      plen: ["Longitud del payload", "El mensaje que sigue mide 35 bytes: los mismos 35 bytes que envía la vista de red."],
      flen: ["Longitud de la tabla", "La parte fija de la tabla mide 21 bytes. Un lector más nuevo, con un layout más largo, ve los campos que faltan como ausentes."],
      bits: ["Bits de presencia", "account y amount están presentes. Un cero presente es distinto de un campo ausente."],
      account: ["account", "1001 como u64 little-endian, leído en su lugar."],
      amount: ["amount", "−2500 como i64. Sin alineación y sin relleno."],
      memo: ["puntero de memo", "Un puntero relativo, +4, hasta los datos de la cadena."],
      memodata: ["datos de memo", "Una longitud u32 y los bytes UTF-8 de \"transfer\", tomados directamente del buffer."],
      crc: ["CRC32C", "Cubre todos los bytes anteriores. Una escritura cortada se lee como incompleta; un bit alterado, como corrupción."],
    },
    "zh-CN": {
      tabDisk: ["磁盘上", "59 字节"],
      tabWire: ["网络上", "42 字节"],
      introWire: "同一条消息作为服务回复，共 42 字节。调用本身已说明它是什么，TLS 保证它完好，所以随它传输的只有类型和长度。",
      vk: ["版本与类型", "帧版本 1 和类型 2（Ok 回复），合在一个字节里。"],
      variant: ["变体", "回复类型为 union 时是其标签；像 Entry 这样的表为 0。"],
      len: ["消息长度", "随后的消息长 35 字节。没有 schema id，也没有 CRC：调用知道类型，传输层保证字节完好。"],
      intro: "存储在磁盘上的消息，共 59 字节：信封说明它是什么、是否完好。将指针移到字段、schema 行或某个字节上，查看它编码的内容。",
      magic: ["魔数", "四个字节 GRVN 标记一个 Graven 信封的开始。"],
      version: ["格式版本", "格式版本 1。读取方会拒绝不认识的版本。"],
      flags: ["标志位与保留位", "在 v1 中恒为零，并且会被检查：非零字节会被拒绝。"],
      sid: ["Schema id", "标识 bank.Entry。它在首次构建时记录到 lockfile 中，之后不再改变。"],
      plen: ["负载长度", "随后的消息长 35 字节，与网络视图发送的 35 字节完全相同。"],
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
  const tabsEl = document.getElementById("views");
  let view = "disk";

  function render() {
    const { bytes, ranges } = views[view];
    const rangeOf = (i) => ranges.find((f) => i >= f[1] && i <= f[2]);
    hexEl.replaceChildren();
    legendEl.replaceChildren();
    hexEl.setAttribute("aria-label", (view === "disk" ? t.tabDisk : t.tabWire).join(", "));
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
  }

  // The two views, as tabs above the bytes.
  [["disk", t.tabDisk], ["wire", t.tabWire]].forEach(([id, [name, size]]) => {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.setAttribute("role", "tab");
    btn.dataset.view = id;
    btn.innerHTML = '<span class="vn"></span><span class="vs"></span>';
    btn.querySelector(".vn").textContent = name;
    btn.querySelector(".vs").textContent = size;
    tabsEl.appendChild(btn);
  });
  function select(id) {
    view = id;
    tabsEl.querySelectorAll("button").forEach((b) => b.setAttribute("aria-selected", String(b.dataset.view === id)));
    render();
    show(null);
  }
  tabsEl.addEventListener("click", (e) => {
    const b = e.target.closest("button[data-view]");
    if (b) select(b.dataset.view);
  });

  const root = document.querySelector(".anatomy");
  function show(ids) {
    root.classList.toggle("focus", !!ids);
    document.querySelectorAll(".anatomy [data-f]").forEach((el) => {
      el.classList.toggle("on", !!ids && ids.includes(el.dataset.f));
    });
    const intro = view === "disk" ? t.intro : t.introWire;
    explainEl.textContent = ids ? ids.map((id) => t[id][0] + sep + t[id][1]).join(" ") : intro;
  }
  // On the wire there is no schema id: the schema's name lines point at the length instead.
  const idsFor = (id) => (view === "wire" && id === "sid" ? ["len"] : linked[id] || [id]);

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
  select("disk");
})();
