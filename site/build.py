#!/usr/bin/env python3
"""Render the landing page (site/index.html, English) into target/site/, translate it, and
render the documentation (site/docs.py).

Usage: python3 site/build.py <out_dir>    (needs site/requirements.txt)

Each translation replaces exact English fragments of index.html; a fragment that is no
longer found fails the build, so the English page and its translations cannot drift apart
silently. The specification and the technical docs stay in English (the normative text).
"""
import hashlib
import html
import json
import os
import re
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = open(os.path.join(HERE, "index.html"), encoding="utf8").read()
# Cache busting: the stylesheet URL changes whenever the stylesheet does.
CSS_V = hashlib.sha256(open(os.path.join(HERE, "assets", "site.css"), "rb").read()).hexdigest()[:10]
SRC = SRC.replace('href="assets/site.css"', f'href="assets/site.css?v={CSS_V}"')

COMMON = [  # shared by every translation: paths one level down
    ('href="docs/', 'href="../docs/'),
    ('href="api/', 'href="../api/'),
    ('href="assets/', 'href="../assets/'),
    ('src="assets/', 'src="../assets/'),
    ('<a class="wordmark" href="./"', '<a class="wordmark" href="../"'),
    ('href="pt-br/"', 'href="../pt-br/"'),
    ('href="es/"', 'href="../es/"'),
    ('href="zh/"', 'href="../zh/"'),
    ('<a href="./" hreflang="en" lang="en" aria-current="page">English</a>', '<a href="../" hreflang="en" lang="en">English</a>'),
]

PT = [
    ('<html lang="en">', '<html lang="pt-BR">'),
    ('<title>Graven — an open specification for a zero-copy binary format</title>',
     '<title>Graven — uma especificação aberta para um formato binário zero-copy</title>'),
    ('content="Graven is an open specification for a zero-copy binary format and its schema language, with frozen conformance vectors and implementations in Rust and JavaScript."',
     'content="Graven é uma especificação aberta para um formato binário zero-copy e sua linguagem de schema, com vetores de conformidade congelados e implementações em Rust e JavaScript."'),
    ('Skip to content', 'Pular para o conteúdo'),
    ('aria-label="Graven home"', 'aria-label="Página inicial do Graven"'),
    ('aria-label="Main"', 'aria-label="Principal"'),
    ('aria-label="Language"', 'aria-label="Idioma"'),
    ('aria-label="Footer"', 'aria-label="Rodapé"'),
    ('<a href="#containers">How it works</a>', '<a href="#containers">Como funciona</a>'),
    ('<a href="#start">Quickstart</a>', '<a href="#start">Começar</a>'),
    ('>Docs</a>', '>Documentação</a>'),
    ('<a href="../pt-br/" hreflang="pt-BR" lang="pt-BR">Português</a>',
     '<a href="./" hreflang="pt-BR" lang="pt-BR" aria-current="page">Português</a>'),

    # Hero
    ('<p class="eyebrow">Format v1 frozen · Rust &amp; JS / TS · MIT</p>',
     '<p class="eyebrow">Formato v1 congelado · Rust &amp; JS / TS · MIT</p>'),
    ('<h1>A binary format built for data that lasts.</h1>',
     '<h1>Um formato binário feito para dados que duram.</h1>'),
    ('<p class="lede">Read fields straight from raw bytes with no parsing or allocation. One shared schema and lockfile keep Rust and TypeScript/JavaScript compatible across stored logs and RPC streams.</p>',
     '<p class="lede">Leia campos direto dos bytes brutos, sem parsing nem alocação. Um schema e lockfile compartilhados mantêm Rust e TypeScript/JavaScript compatíveis em logs em disco e streams RPC.</p>'),
    ('<a class="btn primary" href="#start">Get started</a>', '<a class="btn primary" href="#start">Primeiros passos</a>'),
    ('<a class="btn" href="../docs/index.html">Read the docs</a>', '<a class="btn" href="../docs/index.html">Ler a documentação</a>'),
    ('<p class="hero-meta">Normative <a href="../docs/spec/graven-format-v1.html">format v1 spec</a> · Inspired by FlatBuffers and Cap&#39;n Proto (<a href="../docs/history/index.html">how we got here</a>)</p>',
     '<p class="hero-meta"><a href="../docs/spec/graven-format-v1.html">Especificação normativa v1</a> · Inspirado no FlatBuffers e Cap&#39;n Proto (<a href="../docs/history/index.html">como chegamos aqui</a>)</p>\n      <p class="note">A especificação e a documentação técnica estão em inglês.</p>'),
    ('One message, on disk and on the wire', 'Uma mensagem, no disco e na rede'),
    ('<span class="anatomy-hint">Hover or tap any field</span>', '<span class="anatomy-hint">Passe o cursor ou toque em um campo</span>'),
    ('aria-label="Where the message is"', 'aria-label="Onde a mensagem está"'),
    ('aria-label="The 59 bytes of an enveloped Entry message"', 'aria-label="Os 59 bytes de uma mensagem Entry com envelope"'),
    ('aria-label="Fields"', 'aria-label="Campos"'),

    # Band 1: Why
    ('<h2>Built for data that outlives the code that wrote it</h2>',
     '<h2>Feito para dados que vivem mais que o código que os escreveu</h2>'),
    ('<p>Write records once and keep reading them as services upgrade, storage engines change, and audits run years later.</p>',
     '<p>Escreva registros uma vez e siga lendo enquanto serviços evoluem, bancos mudam e auditorias rodam anos depois.</p>'),
    ('<h3>Read in place</h3>', '<h3>Leitura no lugar</h3>'),
    ('<p>Fields decode directly from byte offsets. Strings and byte slices borrow straight from the buffer, so untouched fields cost zero work.</p>',
     '<p>Os campos são lidos direto dos offsets nos bytes. Strings e fatias emprestam direto do buffer, então campos não acessados têm custo zero.</p>'),
    ('<h3>One bounded verify pass</h3>', '<h3>Passada única e limitada de verify</h3>'),
    ('<p>Untrusted bytes pass through a single <code>verify</code> walk with fixed depth (64) and work limits, returning a trusted view without re-checking on every read.</p>',
     '<p>Bytes não confiáveis passam por uma única checagem <code>verify</code> com limite fixo de profundidade (64) e trabalho, devolvendo uma view confiável sem revalidar leituras.</p>'),
    ('<h3>Lockfile-guarded evolution</h3>', '<h3>Evolução protegida por lockfile</h3>'),
    ('<p><code>gravenc</code> records every layout in a lockfile and rejects schema edits that would make old and new readers disagree.</p>',
     '<p>O <code>gravenc</code> registra cada layout num lockfile e recusa alterações de schema que fariam leitores antigos e novos discordarem.</p>'),
    ('<h3>Crash vs. corruption clarity</h3>', '<h3>Diferença clara entre queda e corrupção</h3>'),
    ('<p>Normative envelope checks distinguish a write cut short by a crash from a flipped bit (CRC32C)—they are never confused.</p>',
     '<p>As checagens normativas do envelope separam uma escrita cortada por queda de um bit invertido (CRC32C) — os dois nunca se confundem.</p>'),
    ('<p class="uses-label">Common fits</p>', '<p class="uses-label">Onde se encaixa</p>'),
    ('<span class="chip">Ledgers &amp; event logs</span>', '<span class="chip">Livros-razão e logs de eventos</span>'),
    ('<span class="chip">Browser &harr; Rust RPC</span>', '<span class="chip">RPC Browser &harr; Rust</span>'),
    ('<span class="chip">Offline &amp; embedded sync</span>', '<span class="chip">Sync offline e embarcados</span>'),
    ('<span class="chip">KV storage &amp; caches</span>', '<span class="chip">Caches e bancos KV</span>'),
    ('<span class="chip">Long-term archives</span>', '<span class="chip">Arquivos de longo prazo</span>'),
    ('<span class="chip">Wasm plugin boundaries</span>', '<span class="chip">Fronteiras Wasm</span>'),

    # Band 2: Containers
    ('<h2>One message, two containers</h2>', '<h2>Uma mensagem, dois contêineres</h2>'),
    ('<p>The <code>Entry</code> payload is the exact same 35 bytes everywhere. Only its outer container changes between storage and network.</p>',
     '<p>Os dados de <code>Entry</code> são os exatos mesmos 35 bytes em qualquer lugar. Só muda o contêiner externo entre disco e rede.</p>'),
    ('<span class="box-name">At rest: the envelope</span>', '<span class="box-name">Em repouso: o envelope</span>'),
    ('<span class="box-name">In flight: the frame</span>', '<span class="box-name">Em trânsito: o frame</span>'),
    ('>header 20</span>', '>cabeçalho 20</span>'),
    ('>message 35</span>', '>mensagem 35</span>'),
    ('<p>Files, logs, and queues have no call context, so the envelope adds a <code>schema_id</code>, payload length, and CRC32C checksum.</p>',
     '<p>Arquivos, logs e filas não têm contexto de chamada, então o envelope adiciona <code>schema_id</code>, tamanho e checksum CRC32C.</p>'),
    ('<p>Over an ordered transport (TLS, sockets), a reply needs only 7 header bytes (kind, variant, length); a call adds method and service id (17 bytes).</p>',
     '<p>Sobre transporte ordenado (TLS, sockets), uma resposta precisa só de 7 bytes de cabeçalho (tipo, variante, tamanho); chamadas somam método e service id (17 bytes).</p>'),
    ('<p class="more">Zero re-encoding: a server persists what it received simply by wrapping the same payload slice in an envelope. <a href="../docs/services.html#the-bytes">How frames work</a></p>',
     '<p class="more">Sem recodificar: o servidor grava o que recebeu apenas envolvendo a mesma fatia de payload num envelope. <a href="../docs/services.html#the-bytes">Como os frames funcionam</a></p>'),

    # Band 3: Services
    ('<h2>Services from the same schema</h2>', '<h2>Serviços a partir do mesmo schema</h2>'),
    ('<p>Declare unary methods, typed errors, and streams next to your tables. <code>gravenc</code> emits typed clients for Rust and browsers/Node, plus a <code>no_std</code> Rust server edge.</p>',
     '<p>Declare métodos unários, erros tipados e streams ao lado das tabelas. O <code>gravenc</code> gera clientes tipados para Rust e browser/Node, além de borda de servidor <code>no_std</code> em Rust.</p>'),
    ('<p class="evo-label">Typed client</p>', '<p class="evo-label">Cliente tipado</p>'),
    ('<dt>Errors are part of the type</dt>', '<dt>Erros fazem parte do tipo</dt>'),
    ('<dd>Declared errors travel as typed unions checked by the compiler; transport and decode faults share one uniform handler.</dd>',
     '<dd>Erros declarados viajam como uniões tipadas checadas pelo compilador; falhas de transporte e decodificação usam um único handler.</dd>'),
    ('<dt>Streams yield as frames arrive</dt>', '<dt>Streams entregam conforme chegam</dt>'),
    ('<dd>Self-delimiting frames deliver each streamed item as soon as its bytes arrive, enforcing configurable size limits.</dd>',
     '<dd>Frames autodelimitados entregam cada item do stream assim que seus bytes chegam, aplicando limites configuráveis de tamanho.</dd>'),
    ('<dt>Validation speaks in codes</dt>', '<dt>A validação fala em códigos</dt>'),
    ('<dd>Broken rules return field, rule, bounds, and check reason codes—never hardcoded server text—ready for client localization.</dd>',
     '<dd>Regras violadas devolvem códigos de campo, regra, limites e motivo — nunca texto fixo do servidor — prontos para tradução no cliente.</dd>'),

    # Band 4: Evolution
    ('<h2>Schema changes reviewed before they ship</h2>', '<h2>Mudanças de schema revisadas antes de publicar</h2>'),
    ('<p>Change a field type or add a required field and <code>gravenc update</code> refuses, citing rule and line. Add a default instead and the lockfile updates cleanly.</p>',
     '<p>Altere o tipo ou adicione um campo obrigatório e o <code>gravenc update</code> recusa, citando regra e linha. Adicione um valor padrão e o lockfile é atualizado.</p>'),
    ('<p class="evo-label">Rejected</p>', '<p class="evo-label">Recusado</p>'),
    ('<p class="evo-label">Accepted</p>', '<p class="evo-label">Aceito</p>'),

    # Band 5: Quickstart
    ('<h2>Get started</h2>', '<h2>Primeiros passos</h2>'),
    ('<p>Shared schema and lockfile on disk; native zero-copy bindings in your language.</p>',
     '<p>Schema e lockfile compartilhados no disco; bindings zero-copy nativos na sua linguagem.</p>'),
    ('Install the compiler', 'Instale o compilador'),
    ('Create your schema &amp; lockfile', 'Crie o schema e o lockfile'),
    ('Generate code, write, and verify in place', 'Gere o código, escreva e verifique no lugar'),
    ('// Generate with `gravenc gen schemas/bank.graven -o src/gen` (or build.rs)',
     '// Gere com `gravenc gen schemas/bank.graven -o src/gen` (ou via build.rs)'),
    ('// checks CRC32C and runs verify', '// confere CRC32C e executa verify'),
    ('// Generate with `gravenc gen schemas/bank.graven -o src/gen --lang js`',
     '// Gere com `gravenc gen schemas/bank.graven -o src/gen --lang js`'),
    ('<p class="more">Continue with the <a href="../docs/getting-started.html">getting started guide</a>, <a href="../docs/coming-from-json.html">coming from JSON</a>, or the <a href="../docs/graven-language.html">schema language reference</a>.</p>',
     '<p class="more">Continue no <a href="../docs/getting-started.html">guia de primeiros passos</a>, <a href="../docs/coming-from-json.html">vindo do JSON</a> ou na <a href="../docs/graven-language.html">referência da linguagem de schema</a> (em inglês).</p>'),

    # Band 6: Implementations & Verification
    ('<h2>Implementations &amp; verification</h2>', '<h2>Implementações e verificação</h2>'),
    ('<p>The specification is the product. Every claim below is pinned by frozen conformance vectors, fuzzing, and automated checks.</p>',
     '<p>A especificação é o produto. Cada afirmação abaixo é garantida por vetores de conformidade congelados, fuzzing e testes automatizados.</p>'),
    ('<span class="sdk-badge">Reference · no_std</span>', '<span class="sdk-badge">Referência · no_std</span>'),
    ('<p><code>no_std</code> + <code>alloc</code> runtime with two dependencies, <code>gravenc</code> compiler and lockfile checks, typed client &amp; server edge codegen, and schema interpreter. Passes all 42 v1 + 23 RPC vectors.</p>',
     '<p>Runtime <code>no_std</code> + <code>alloc</code> com duas dependências, compilador <code>gravenc</code> com lockfile, codegen tipado de cliente e borda de servidor, e interpretador de schema. Passa em todos os 42 vetores v1 + 23 RPC.</p>'),
    ('>Rust SDK</a>', '>SDK Rust</a>'),
    ('>API reference</a>', '>Referência da API</a>'),
    ('<span class="sdk-badge">Zero dependencies</span>', '<span class="sdk-badge">Zero dependências</span>'),
    ('<p>ES2022 <code>.js</code> modules + <code>.d.ts</code> types generated by <code>gravenc gen --lang js</code>. Canonical writers, validation codes, and browser/Node/Deno RPC clients. Passes all 42 v1 + 23 RPC vectors.</p>',
     '<p>Módulos ES2022 <code>.js</code> + tipos <code>.d.ts</code> gerados via <code>gravenc gen --lang js</code>. Writers canônicos, códigos de validação e clientes RPC para browser, Node e Deno. Passa nos 42 vetores v1 + 23 RPC.</p>'),
    ('>JS / TS SDK</a>', '>SDK JS / TS</a>'),
    ('>Add another language</a>', '>Adicionar outra linguagem</a>'),
    ('<th scope="col">Guarantee</th><th scope="col">How it is checked</th>', '<th scope="col">Garantia</th><th scope="col">Como é verificada</th>'),
    ('<th scope="row">Frozen wire specification</th>', '<th scope="row">Especificação binária congelada</th>'),
    ('<td>Normative RFC 2119 spec with 42 format v1 vectors and 23 RPC frame vectors. The reference writer reproduces every canonical <code>.bin</code> byte for byte.</td>',
     '<td>Especificação normativa RFC 2119 com 42 vetores v1 e 23 vetores de frames RPC. O writer de referência reproduz cada <code>.bin</code> canônico byte a byte.</td>'),
    ('<th scope="row">Cross-implementation agreement</th>', '<th scope="row">Acordo entre implementações</th>'),
    ('<td>An independent Rust schema interpreter and generated code decode every vector and single-byte mutation identically; the JS runtime is fuzzed against Rust across millions of mutated messages and frames.</td>',
     '<td>Um interpretador independente em Rust e o código gerado decodificam igual cada vetor e mutação de 1 byte; o runtime JS passa por fuzzing contra o Rust em milhões de mensagens e frames mutados.</td>'),
    ('<th scope="row">Bounded verification &amp; Miri clean</th>', '<th scope="row">Verificação limitada e limpa no Miri</th>'),
    ('<td><code>verify</code> caps depth at 64 and work at <code>max(len × 8, 1 MiB)</code>. Five fuzz targets ran &gt;80 million inputs with overflow checks, and the sole <code>unsafe</code> UTF-8 fast path runs clean under Miri.</td>',
     '<td><code>verify</code> limita profundidade em 64 e trabalho em <code>max(len × 8, 1 MiB)</code>. Cinco alvos de fuzz rodaram &gt;80 milhões de entradas com checagem de overflow, e o único caminho rápido <code>unsafe</code> para UTF-8 passa limpo no Miri.</td>'),
    ('<th scope="row">Lockfile compatibility enforcement</th>', '<th scope="row">Compatibilidade garantida pelo lockfile</th>'),
    ('<td>15 hard errors reject breaking layout or method edits and 2 warnings require confirmation, verified over a corpus of multi-version schema chains.</td>',
     '<td>15 erros rígidos recusam quebras de layout ou método e 2 avisos exigem confirmação, testados contra um corpus de cadeias de versões.</td>'),
    ('<p class="more">Implementing Graven in another language? Use the compiler&#39;s lockfile layouts and verify against all <a href="../docs/conformance-vectors.html">conformance vectors</a>.</p>',
     '<p class="more">Implementando Graven em outra linguagem? Use os layouts do lockfile do compilador e valide contra os <a href="../docs/conformance-vectors.html">vetores de conformidade</a>.</p>'),

    # Footer
    ('Graven · format v1 · MIT license', 'Graven · formato v1 · licença MIT'),
    ('>Specification</a>', '>Especificação</a>'),
    ('>Schema language</a>', '>Linguagem de schema</a>'),
    ('>Services</a>', '>Serviços</a>'),
    ('>Conformance vectors</a>', '>Vetores de conformidade</a>'),
    ('>Rust API</a>', '>API Rust</a>'),
    ('>Testing</a>', '>Testes</a>'),
]

ES = [
    ('<html lang="en">', '<html lang="es">'),
    ('<title>Graven — an open specification for a zero-copy binary format</title>',
     '<title>Graven — una especificación abierta para un formato binario zero-copy</title>'),
    ('content="Graven is an open specification for a zero-copy binary format and its schema language, with frozen conformance vectors and implementations in Rust and JavaScript."',
     'content="Graven es una especificación abierta para un formato binario zero-copy y su lenguaje de esquemas, con vectores de conformidad congelados e implementaciones en Rust y JavaScript."'),
    ('Skip to content', 'Saltar al contenido'),
    ('aria-label="Graven home"', 'aria-label="Inicio de Graven"'),
    ('aria-label="Main"', 'aria-label="Principal"'),
    ('aria-label="Language"', 'aria-label="Idioma"'),
    ('aria-label="Footer"', 'aria-label="Pie de página"'),
    ('<a href="#containers">How it works</a>', '<a href="#containers">Cómo funciona</a>'),
    ('<a href="#start">Quickstart</a>', '<a href="#start">Empezar</a>'),
    ('>Docs</a>', '>Documentación</a>'),
    ('<a href="../es/" hreflang="es" lang="es">Español</a>',
     '<a href="./" hreflang="es" lang="es" aria-current="page">Español</a>'),

    # Hero
    ('<p class="eyebrow">Format v1 frozen · Rust &amp; JS / TS · MIT</p>',
     '<p class="eyebrow">Formato v1 congelado · Rust y JS / TS · MIT</p>'),
    ('<h1>A binary format built for data that lasts.</h1>',
     '<h1>Un formato binario hecho para datos que perduran.</h1>'),
    ('<p class="lede">Read fields straight from raw bytes with no parsing or allocation. One shared schema and lockfile keep Rust and TypeScript/JavaScript compatible across stored logs and RPC streams.</p>',
     '<p class="lede">Lee campos directo desde bytes sin parsing ni asignación. Un esquema y lockfile compartidos mantienen a Rust y TypeScript/JavaScript compatibles en logs en disco y streams RPC.</p>'),
    ('<a class="btn primary" href="#start">Get started</a>', '<a class="btn primary" href="#start">Primeros pasos</a>'),
    ('<a class="btn" href="../docs/index.html">Read the docs</a>', '<a class="btn" href="../docs/index.html">Leer la documentación</a>'),
    ('<p class="hero-meta">Normative <a href="../docs/spec/graven-format-v1.html">format v1 spec</a> · Inspired by FlatBuffers and Cap&#39;n Proto (<a href="../docs/history/index.html">how we got here</a>)</p>',
     '<p class="hero-meta"><a href="../docs/spec/graven-format-v1.html">Especificación normativa v1</a> · Inspirado en FlatBuffers y Cap&#39;n Proto (<a href="../docs/history/index.html">cómo llegamos aquí</a>)</p>\n      <p class="note">La especificación y la documentación técnica están en inglés.</p>'),
    ('One message, on disk and on the wire', 'Un mensaje, en disco y en la red'),
    ('<span class="anatomy-hint">Hover or tap any field</span>', '<span class="anatomy-hint">Pasa el cursor o toca cualquier campo</span>'),
    ('aria-label="Where the message is"', 'aria-label="Dónde está el mensaje"'),
    ('aria-label="The 59 bytes of an enveloped Entry message"', 'aria-label="Los 59 bytes de un mensaje Entry con sobre"'),
    ('aria-label="Fields"', 'aria-label="Campos"'),

    # Band 1: Why
    ('<h2>Built for data that outlives the code that wrote it</h2>',
     '<h2>Hecho para datos que sobreviven al código que los escribió</h2>'),
    ('<p>Write records once and keep reading them as services upgrade, storage engines change, and audits run years later.</p>',
     '<p>Escribe registros una vez y síguelos leyendo mientras los servicios avanzan, el almacenamiento cambia y las auditorías corren años después.</p>'),
    ('<h3>Read in place</h3>', '<h3>Lectura en su lugar</h3>'),
    ('<p>Fields decode directly from byte offsets. Strings and byte slices borrow straight from the buffer, so untouched fields cost zero work.</p>',
     '<p>Los campos se decodifican directo desde desplazamientos de bytes. Las cadenas y segmentos se toman prestados del buffer, sin coste para campos no tocados.</p>'),
    ('<h3>One bounded verify pass</h3>', '<h3>Una pasada acotada de verify</h3>'),
    ('<p>Untrusted bytes pass through a single <code>verify</code> walk with fixed depth (64) and work limits, returning a trusted view without re-checking on every read.</p>',
     '<p>Los bytes no confiables pasan por una única comprobación <code>verify</code> con límite fijo de profundidad (64) y trabajo, devolviendo una vista verificada sin recomprobar lecturas.</p>'),
    ('<h3>Lockfile-guarded evolution</h3>', '<h3>Evolución protegida por lockfile</h3>'),
    ('<p><code>gravenc</code> records every layout in a lockfile and rejects schema edits that would make old and new readers disagree.</p>',
     '<p><code>gravenc</code> registra cada layout en un lockfile y rechaza ediciones de esquema que harían discrepar a lectores antiguos y nuevos.</p>'),
    ('<h3>Crash vs. corruption clarity</h3>', '<h3>Claridad entre caída y corrupción</h3>'),
    ('<p>Normative envelope checks distinguish a write cut short by a crash from a flipped bit (CRC32C)—they are never confused.</p>',
     '<p>Las comprobaciones normativas del sobre distinguen una escritura truncada por una caída de un bit alterado (CRC32C); nunca se confunden.</p>'),
    ('<p class="uses-label">Common fits</p>', '<p class="uses-label">Casos frecuentes</p>'),
    ('<span class="chip">Ledgers &amp; event logs</span>', '<span class="chip">Libros contables y logs</span>'),
    ('<span class="chip">Browser &harr; Rust RPC</span>', '<span class="chip">RPC Navegador &harr; Rust</span>'),
    ('<span class="chip">Offline &amp; embedded sync</span>', '<span class="chip">Sync sin conexión y embebidos</span>'),
    ('<span class="chip">KV storage &amp; caches</span>', '<span class="chip">Cachés y almacenamiento KV</span>'),
    ('<span class="chip">Long-term archives</span>', '<span class="chip">Archivos históricos</span>'),
    ('<span class="chip">Wasm plugin boundaries</span>', '<span class="chip">Fronteras Wasm</span>'),

    # Band 2: Containers
    ('<h2>One message, two containers</h2>', '<h2>Un mensaje, dos contenedores</h2>'),
    ('<p>The <code>Entry</code> payload is the exact same 35 bytes everywhere. Only its outer container changes between storage and network.</p>',
     '<p>La carga de <code>Entry</code> son exactamente los mismos 35 bytes en todas partes. Solo cambia su contenedor exterior entre almacenamiento y red.</p>'),
    ('<span class="box-name">At rest: the envelope</span>', '<span class="box-name">En reposo: el sobre</span>'),
    ('<span class="box-name">In flight: the frame</span>', '<span class="box-name">En tránsito: el frame</span>'),
    ('>header 20</span>', '>cabecera 20</span>'),
    ('>message 35</span>', '>mensaje 35</span>'),
    ('<p>Files, logs, and queues have no call context, so the envelope adds a <code>schema_id</code>, payload length, and CRC32C checksum.</p>',
     '<p>Archivos, logs y colas no tienen contexto de llamada, así que el sobre añade <code>schema_id</code>, longitud y suma de verificación CRC32C.</p>'),
    ('<p>Over an ordered transport (TLS, sockets), a reply needs only 7 header bytes (kind, variant, length); a call adds method and service id (17 bytes).</p>',
     '<p>Sobre un transporte ordenado (TLS, sockets), una respuesta solo necesita 7 bytes de cabecera (tipo, variante, longitud); una llamada añade método y service id (17 bytes).</p>'),
    ('<p class="more">Zero re-encoding: a server persists what it received simply by wrapping the same payload slice in an envelope. <a href="../docs/services.html#the-bytes">How frames work</a></p>',
     '<p class="more">Sin recodificar: un servidor guarda lo recibido simplemente envolviendo el mismo segmento de bytes en un sobre. <a href="../docs/services.html#the-bytes">Cómo funcionan los frames</a></p>'),

    # Band 3: Services
    ('<h2>Services from the same schema</h2>', '<h2>Servicios desde el mismo esquema</h2>'),
    ('<p>Declare unary methods, typed errors, and streams next to your tables. <code>gravenc</code> emits typed clients for Rust and browsers/Node, plus a <code>no_std</code> Rust server edge.</p>',
     '<p>Declara métodos unarios, errores tipados y streams junto a tus tablas. <code>gravenc</code> genera clientes tipados para Rust y navegadores/Node, además de un borde de servidor Rust <code>no_std</code>.</p>'),
    ('<p class="evo-label">Typed client</p>', '<p class="evo-label">Cliente tipado</p>'),
    ('<dt>Errors are part of the type</dt>', '<dt>Los errores son parte del tipo</dt>'),
    ('<dd>Declared errors travel as typed unions checked by the compiler; transport and decode faults share one uniform handler.</dd>',
     '<dd>Los errores declarados viajan como uniones tipadas verificadas por el compilador; los fallos de transporte y decodificación comparten un único manejador.</dd>'),
    ('<dt>Streams yield as frames arrive</dt>', '<dt>Los streams entregan al llegar cada frame</dt>'),
    ('<dd>Self-delimiting frames deliver each streamed item as soon as its bytes arrive, enforcing configurable size limits.</dd>',
     '<dd>Tramas autodelimitadas entregan cada elemento del stream en cuanto llegan sus bytes, aplicando límites de tamaño configurables.</dd>'),
    ('<dt>Validation speaks in codes</dt>', '<dt>La validación habla en códigos</dt>'),
    ('<dd>Broken rules return field, rule, bounds, and check reason codes—never hardcoded server text—ready for client localization.</dd>',
     '<dd>Las reglas rotas devuelven códigos de campo, regla, límites y motivo — nunca texto fijo del servidor — listos para localizar en el cliente.</dd>'),

    # Band 4: Evolution
    ('<h2>Schema changes reviewed before they ship</h2>', '<h2>Cambios de esquema revisados antes de desplegar</h2>'),
    ('<p>Change a field type or add a required field and <code>gravenc update</code> refuses, citing rule and line. Add a default instead and the lockfile updates cleanly.</p>',
     '<p>Cambia el tipo o añade un campo obligatorio y <code>gravenc update</code> lo rechaza, indicando regla y línea. Añade un valor por defecto y el lockfile se actualiza.</p>'),
    ('<p class="evo-label">Rejected</p>', '<p class="evo-label">Rechazado</p>'),
    ('<p class="evo-label">Accepted</p>', '<p class="evo-label">Aceptado</p>'),

    # Band 5: Quickstart
    ('<h2>Get started</h2>', '<h2>Primeros pasos</h2>'),
    ('<p>Shared schema and lockfile on disk; native zero-copy bindings in your language.</p>',
     '<p>Esquema y lockfile compartidos en disco; bindings zero-copy nativos en tu lenguaje.</p>'),
    ('Install the compiler', 'Instala el compilador'),
    ('Create your schema &amp; lockfile', 'Crea tu esquema y lockfile'),
    ('Generate code, write, and verify in place', 'Genera código, escribe y verifica en su lugar'),
    ('// Generate with `gravenc gen schemas/bank.graven -o src/gen` (or build.rs)',
     '// Genera con `gravenc gen schemas/bank.graven -o src/gen` (o build.rs)'),
    ('// checks CRC32C and runs verify', '// comprueba CRC32C y ejecuta verify'),
    ('// Generate with `gravenc gen schemas/bank.graven -o src/gen --lang js`',
     '// Genera con `gravenc gen schemas/bank.graven -o src/gen --lang js`'),
    ('<p class="more">Continue with the <a href="../docs/getting-started.html">getting started guide</a>, <a href="../docs/coming-from-json.html">coming from JSON</a>, or the <a href="../docs/graven-language.html">schema language reference</a>.</p>',
     '<p class="more">Continúa con la <a href="../docs/getting-started.html">guía de primeros pasos</a>, <a href="../docs/coming-from-json.html">viniendo desde JSON</a> o la <a href="../docs/graven-language.html">referencia del lenguaje de esquemas</a> (en inglés).</p>'),

    # Band 6: Implementations & Verification
    ('<h2>Implementations &amp; verification</h2>', '<h2>Implementaciones y verificación</h2>'),
    ('<p>The specification is the product. Every claim below is pinned by frozen conformance vectors, fuzzing, and automated checks.</p>',
     '<p>La especificación es el producto. Cada afirmación está respaldada por vectores de conformidad congelados, fuzzing y pruebas automáticas.</p>'),
    ('<span class="sdk-badge">Reference · no_std</span>', '<span class="sdk-badge">Referencia · no_std</span>'),
    ('<p><code>no_std</code> + <code>alloc</code> runtime with two dependencies, <code>gravenc</code> compiler and lockfile checks, typed client &amp; server edge codegen, and schema interpreter. Passes all 42 v1 + 23 RPC vectors.</p>',
     '<p>Runtime <code>no_std</code> + <code>alloc</code> con dos dependencias, compilador <code>gravenc</code> con lockfile, codegen tipado de cliente y borde de servidor, e intérprete de esquema. Pasa los 42 vectores v1 + 23 RPC.</p>'),
    ('>Rust SDK</a>', '>SDK Rust</a>'),
    ('>API reference</a>', '>Referencia de la API</a>'),
    ('<span class="sdk-badge">Zero dependencies</span>', '<span class="sdk-badge">Cero dependencias</span>'),
    ('<p>ES2022 <code>.js</code> modules + <code>.d.ts</code> types generated by <code>gravenc gen --lang js</code>. Canonical writers, validation codes, and browser/Node/Deno RPC clients. Passes all 42 v1 + 23 RPC vectors.</p>',
     '<p>Módulos ES2022 <code>.js</code> + tipos <code>.d.ts</code> generados con <code>gravenc gen --lang js</code>. Writers canónicos, códigos de validación y clientes RPC para navegador, Node y Deno. Pasa los 42 vectores v1 + 23 RPC.</p>'),
    ('>JS / TS SDK</a>', '>SDK JS / TS</a>'),
    ('>Add another language</a>', '>Añadir otro lenguaje</a>'),
    ('<th scope="col">Guarantee</th><th scope="col">How it is checked</th>', '<th scope="col">Garantía</th><th scope="col">Cómo se comprueba</th>'),
    ('<th scope="row">Frozen wire specification</th>', '<th scope="row">Especificación binaria congelada</th>'),
    ('<td>Normative RFC 2119 spec with 42 format v1 vectors and 23 RPC frame vectors. The reference writer reproduces every canonical <code>.bin</code> byte for byte.</td>',
     '<td>Especificación normativa RFC 2119 con 42 vectores v1 y 23 vectores de frames RPC. El escritor de referencia reproduce cada <code>.bin</code> canónico byte a byte.</td>'),
    ('<th scope="row">Cross-implementation agreement</th>', '<th scope="row">Consistencia entre implementaciones</th>'),
    ('<td>An independent Rust schema interpreter and generated code decode every vector and single-byte mutation identically; the JS runtime is fuzzed against Rust across millions of mutated messages and frames.</td>',
     '<td>Un intérprete de esquema independiente en Rust y el código generado decodifican igual cada vector y mutación de un byte; el runtime JS se somete a fuzzing contra Rust en millones de mensajes y frames mutados.</td>'),
    ('<th scope="row">Bounded verification &amp; Miri clean</th>', '<th scope="row">Verificación acotada y limpia en Miri</th>'),
    ('<td><code>verify</code> caps depth at 64 and work at <code>max(len × 8, 1 MiB)</code>. Five fuzz targets ran &gt;80 million inputs with overflow checks, and the sole <code>unsafe</code> UTF-8 fast path runs clean under Miri.</td>',
     '<td><code>verify</code> limita la profundidad a 64 y el trabajo a <code>max(len × 8, 1 MiB)</code>. Cinco objetivos de fuzzing ejecutaron &gt;80 millones de entradas con comprobación de desbordamiento, y el único camino rápido <code>unsafe</code> para UTF-8 pasa limpio bajo Miri.</td>'),
    ('<th scope="row">Lockfile compatibility enforcement</th>', '<th scope="row">Compatibilidad exigida por el lockfile</th>'),
    ('<td>15 hard errors reject breaking layout or method edits and 2 warnings require confirmation, verified over a corpus of multi-version schema chains.</td>',
     '<td>15 errores duros rechazan roturas de layout o método y 2 avisos exigen confirmación, verificados con un corpus de cadenas de versiones.</td>'),
    ('<p class="more">Implementing Graven in another language? Use the compiler&#39;s lockfile layouts and verify against all <a href="../docs/conformance-vectors.html">conformance vectors</a>.</p>',
     '<p class="more">¿Implementando Graven en otro lenguaje? Usa los layouts del lockfile del compilador y valida contra todos los <a href="../docs/conformance-vectors.html">vectores de conformidad</a>.</p>'),

    # Footer
    ('Graven · format v1 · MIT license', 'Graven · formato v1 · licencia MIT'),
    ('>Specification</a>', '>Especificación</a>'),
    ('>Schema language</a>', '>Lenguaje de esquemas</a>'),
    ('>Services</a>', '>Servicios</a>'),
    ('>Conformance vectors</a>', '>Vectores de conformidad</a>'),
    ('>Rust API</a>', '>API de Rust</a>'),
    ('>Testing</a>', '>Pruebas</a>'),
]

ZH = [
    ('<html lang="en">', '<html lang="zh-CN">'),
    ('<title>Graven — an open specification for a zero-copy binary format</title>',
     '<title>Graven — 零拷贝二进制格式的开放规范</title>'),
    ('content="Graven is an open specification for a zero-copy binary format and its schema language, with frozen conformance vectors and implementations in Rust and JavaScript."',
     'content="Graven 是一个零拷贝二进制格式及其 schema 语言的开放规范，附带冻结的一致性测试向量，以及 Rust 和 JavaScript 实现。"'),
    ('Skip to content', '跳到正文'),
    ('aria-label="Graven home"', 'aria-label="Graven 首页"'),
    ('aria-label="Main"', 'aria-label="主导航"'),
    ('aria-label="Language"', 'aria-label="语言"'),
    ('aria-label="Footer"', 'aria-label="页脚"'),
    ('<a href="#containers">How it works</a>', '<a href="#containers">工作原理</a>'),
    ('<a href="#start">Quickstart</a>', '<a href="#start">快速开始</a>'),
    ('>Docs</a>', '>文档</a>'),
    ('<a href="../zh/" hreflang="zh-CN" lang="zh-CN">中文</a>',
     '<a href="./" hreflang="zh-CN" lang="zh-CN" aria-current="page">中文</a>'),

    # Hero
    ('<p class="eyebrow">Format v1 frozen · Rust &amp; JS / TS · MIT</p>',
     '<p class="eyebrow">格式 v1 已冻结 · Rust &amp; JS / TS · MIT</p>'),
    ('<h1>A binary format built for data that lasts.</h1>',
     '<h1>为长期保存的数据而设计的二进制格式。</h1>'),
    ('<p class="lede">Read fields straight from raw bytes with no parsing or allocation. One shared schema and lockfile keep Rust and TypeScript/JavaScript compatible across stored logs and RPC streams.</p>',
     '<p class="lede">直接从原始字节原地读取字段，无需解析或内存分配。同一份 schema 与 lockfile 让 Rust 和 TypeScript/JavaScript 在持久化日志和 RPC 流之间保持兼容。</p>'),
    ('<a class="btn primary" href="#start">Get started</a>', '<a class="btn primary" href="#start">快速上手</a>'),
    ('<a class="btn" href="../docs/index.html">Read the docs</a>', '<a class="btn" href="../docs/index.html">阅读文档</a>'),
    ('<p class="hero-meta">Normative <a href="../docs/spec/graven-format-v1.html">format v1 spec</a> · Inspired by FlatBuffers and Cap&#39;n Proto (<a href="../docs/history/index.html">how we got here</a>)</p>',
     '<p class="hero-meta"><a href="../docs/spec/graven-format-v1.html">规范性格式 v1 文档</a> · 受 FlatBuffers 与 Cap&#39;n Proto 启发（<a href="../docs/history/index.html">演进历程</a>）</p>\n      <p class="note">规范与技术文档为英文。</p>'),
    ('One message, on disk and on the wire', '同一条消息：磁盘上与网络上'),
    ('<span class="anatomy-hint">Hover or tap any field</span>', '<span class="anatomy-hint">悬停或点击任意字段</span>'),
    ('aria-label="Where the message is"', 'aria-label="消息所在位置"'),
    ('aria-label="The 59 bytes of an enveloped Entry message"', 'aria-label="一条带信封的 Entry 消息的 59 个字节"'),
    ('aria-label="Fields"', 'aria-label="字段"'),

    # Band 1: Why
    ('<h2>Built for data that outlives the code that wrote it</h2>',
     '<h2>为比写入它的代码活得更久的数据而生</h2>'),
    ('<p>Write records once and keep reading them as services upgrade, storage engines change, and audits run years later.</p>',
     '<p>记录只写一次，但在服务升级、存储引擎更替以及多年后的审计中仍可持续读取。</p>'),
    ('<h3>Read in place</h3>', '<h3>原地读取</h3>'),
    ('<p>Fields decode directly from byte offsets. Strings and byte slices borrow straight from the buffer, so untouched fields cost zero work.</p>',
     '<p>字段直接从字节偏移解码。字符串和字节切片直接借用自缓冲区，未访问的字段零开销。</p>'),
    ('<h3>One bounded verify pass</h3>', '<h3>一次有界的 verify 遍历</h3>'),
    ('<p>Untrusted bytes pass through a single <code>verify</code> walk with fixed depth (64) and work limits, returning a trusted view without re-checking on every read.</p>',
     '<p>不可信字节只经过一次带固定深度（64）与工作量上限的 <code>verify</code> 检查，返回可信视图，后续读取不再重复校验。</p>'),
    ('<h3>Lockfile-guarded evolution</h3>', '<h3>由 lockfile 守护的演进</h3>'),
    ('<p><code>gravenc</code> records every layout in a lockfile and rejects schema edits that would make old and new readers disagree.</p>',
     '<p><code>gravenc</code> 将所有布局记录在 lockfile 中，并拒绝会让新旧读取端产生分歧的 schema 修改。</p>'),
    ('<h3>Crash vs. corruption clarity</h3>', '<h3>明确区分崩溃与数据损坏</h3>'),
    ('<p>Normative envelope checks distinguish a write cut short by a crash from a flipped bit (CRC32C)—they are never confused.</p>',
     '<p>规范化的信封检查步骤可以区分因崩溃而截断的写入和比特翻转（CRC32C），两者绝不混淆。</p>'),
    ('<p class="uses-label">Common fits</p>', '<p class="uses-label">适用场景</p>'),
    ('<span class="chip">Ledgers &amp; event logs</span>', '<span class="chip">账本与事件日志</span>'),
    ('<span class="chip">Browser &harr; Rust RPC</span>', '<span class="chip">浏览器 &harr; Rust RPC</span>'),
    ('<span class="chip">Offline &amp; embedded sync</span>', '<span class="chip">离线与嵌入式同步</span>'),
    ('<span class="chip">KV storage &amp; caches</span>', '<span class="chip">KV 存储与缓存</span>'),
    ('<span class="chip">Long-term archives</span>', '<span class="chip">长期归档</span>'),
    ('<span class="chip">Wasm plugin boundaries</span>', '<span class="chip">Wasm 插件边界</span>'),

    # Band 2: Containers
    ('<h2>One message, two containers</h2>', '<h2>一条消息，两种容器</h2>'),
    ('<p>The <code>Entry</code> payload is the exact same 35 bytes everywhere. Only its outer container changes between storage and network.</p>',
     '<p><code>Entry</code> 的负载在任何地方都是完全相同的 35 字节。磁盘存储与网络传输之间变化的只是外层容器。</p>'),
    ('<span class="box-name">At rest: the envelope</span>', '<span class="box-name">静态存储：envelope</span>'),
    ('<span class="box-name">In flight: the frame</span>', '<span class="box-name">网络传输：帧</span>'),
    ('>header 20</span>', '>头部 20</span>'),
    ('>message 35</span>', '>消息 35</span>'),
    ('<p>Files, logs, and queues have no call context, so the envelope adds a <code>schema_id</code>, payload length, and CRC32C checksum.</p>',
     '<p>文件、日志和队列没有调用上下文，因此 envelope 会加上 <code>schema_id</code>、负载长度和 CRC32C 校验和。</p>'),
    ('<p>Over an ordered transport (TLS, sockets), a reply needs only 7 header bytes (kind, variant, length); a call adds method and service id (17 bytes).</p>',
     '<p>在有序传输（TLS、socket）之上，回复只需 7 字节帧头（类型、变体、长度）；调用帧额外携带方法与 service id（共 17 字节）。</p>'),
    ('<p class="more">Zero re-encoding: a server persists what it received simply by wrapping the same payload slice in an envelope. <a href="../docs/services.html#the-bytes">How frames work</a></p>',
     '<p class="more">零重编码：服务端只需用 envelope 包裹收到的同一段负载字节切片即可落盘。<a href="../docs/services.html#the-bytes">帧的工作方式</a></p>'),

    # Band 3: Services
    ('<h2>Services from the same schema</h2>', '<h2>同一份 schema 定义服务</h2>'),
    ('<p>Declare unary methods, typed errors, and streams next to your tables. <code>gravenc</code> emits typed clients for Rust and browsers/Node, plus a <code>no_std</code> Rust server edge.</p>',
     '<p>在表定义旁边声明一元方法、带类型错误和流式方法。<code>gravenc</code> 为 Rust 与浏览器/Node 生成类型化客户端，以及 Rust 的 <code>no_std</code> 服务端边界。</p>'),
    ('<p class="evo-label">Typed client</p>', '<p class="evo-label">类型化客户端</p>'),
    ('<dt>Errors are part of the type</dt>', '<dt>错误是类型的一部分</dt>'),
    ('<dd>Declared errors travel as typed unions checked by the compiler; transport and decode faults share one uniform handler.</dd>',
     '<dd>声明的错误作为联合体由编译器强制处理；传输与解码故障由统一的故障处理器兜底。</dd>'),
    ('<dt>Streams yield as frames arrive</dt>', '<dt>流式回复边到边交付</dt>'),
    ('<dd>Self-delimiting frames deliver each streamed item as soon as its bytes arrive, enforcing configurable size limits.</dd>',
     '<dd>自定界帧会在字节到达时立即交付每个流条目，并执行可配置的帧大小上限。</dd>'),
    ('<dt>Validation speaks in codes</dt>', '<dt>校验以错误码表达</dt>'),
    ('<dd>Broken rules return field, rule, bounds, and check reason codes—never hardcoded server text—ready for client localization.</dd>',
     '<dd>违反校验规则时返回字段、规则、上下限和原因代码，而非服务端硬编码文本，便于前端本地化显示。</dd>'),

    # Band 4: Evolution
    ('<h2>Schema changes reviewed before they ship</h2>', '<h2>上线前由编译器审查 schema 修改</h2>'),
    ('<p>Change a field type or add a required field and <code>gravenc update</code> refuses, citing rule and line. Add a default instead and the lockfile updates cleanly.</p>',
     '<p>修改字段类型或新增必填字段时，<code>gravenc update</code> 会拒绝并给出规则名和行号；改为提供默认值即可顺利更新 lockfile。</p>'),
    ('<p class="evo-label">Rejected</p>', '<p class="evo-label">被拒绝</p>'),
    ('<p class="evo-label">Accepted</p>', '<p class="evo-label">已接受</p>'),

    # Band 5: Quickstart
    ('<h2>Get started</h2>', '<h2>快速上手</h2>'),
    ('<p>Shared schema and lockfile on disk; native zero-copy bindings in your language.</p>',
     '<p>磁盘上共享同一份 schema 与 lockfile；各自语言生成原生零拷贝绑定。</p>'),
    ('Install the compiler', '安装编译器'),
    ('Create your schema &amp; lockfile', '创建 schema 与 lockfile'),
    ('Generate code, write, and verify in place', '生成代码、写入并原地验证'),
    ('// Generate with `gravenc gen schemas/bank.graven -o src/gen` (or build.rs)',
     '// 使用 `gravenc gen schemas/bank.graven -o src/gen`（或 build.rs）生成代码'),
    ('// checks CRC32C and runs verify', '// 检查 CRC32C 并运行 verify'),
    ('// Generate with `gravenc gen schemas/bank.graven -o src/gen --lang js`',
     '// 使用 `gravenc gen schemas/bank.graven -o src/gen --lang js` 生成代码'),
    ('<p class="more">Continue with the <a href="../docs/getting-started.html">getting started guide</a>, <a href="../docs/coming-from-json.html">coming from JSON</a>, or the <a href="../docs/graven-language.html">schema language reference</a>.</p>',
     '<p class="more">继续阅读<a href="../docs/getting-started.html">快速上手指南</a>、<a href="../docs/coming-from-json.html">从 JSON 迁移</a>或 <a href="../docs/graven-language.html">schema 语言参考</a>（英文）。</p>'),

    # Band 6: Implementations & Verification
    ('<h2>Implementations &amp; verification</h2>', '<h2>实现与验证</h2>'),
    ('<p>The specification is the product. Every claim below is pinned by frozen conformance vectors, fuzzing, and automated checks.</p>',
     '<p>规范本身就是产品。下面每项承诺都由冻结的一致性测试向量、模糊测试和自动化检查担保。</p>'),
    ('<span class="sdk-badge">Reference · no_std</span>', '<span class="sdk-badge">参考实现 · no_std</span>'),
    ('<p><code>no_std</code> + <code>alloc</code> runtime with two dependencies, <code>gravenc</code> compiler and lockfile checks, typed client &amp; server edge codegen, and schema interpreter. Passes all 42 v1 + 23 RPC vectors.</p>',
     '<p>仅含两个依赖的 <code>no_std</code> + <code>alloc</code> 运行时、带 lockfile 检查的 <code>gravenc</code> 编译器、类型化客户端与服务端边界生成，以及一致性解释器。通过全部 42 个 v1 与 23 个 RPC 向量。</p>'),
    ('>Rust SDK</a>', '>Rust SDK</a>'),
    ('>API reference</a>', '>API 参考</a>'),
    ('<span class="sdk-badge">Zero dependencies</span>', '<span class="sdk-badge">零依赖</span>'),
    ('<p>ES2022 <code>.js</code> modules + <code>.d.ts</code> types generated by <code>gravenc gen --lang js</code>. Canonical writers, validation codes, and browser/Node/Deno RPC clients. Passes all 42 v1 + 23 RPC vectors.</p>',
     '<p>由 <code>gravenc gen --lang js</code> 生成零依赖 ES2022 <code>.js</code> 模块与 <code>.d.ts</code> 类型声明，含规范写入器、校验码与浏览器/Node/Deno RPC 客户端。通过全部 42 个 v1 与 23 个 RPC 向量。</p>'),
    ('>JS / TS SDK</a>', '>JS / TS SDK</a>'),
    ('>Add another language</a>', '>添加其他语言</a>'),
    ('<th scope="col">Guarantee</th><th scope="col">How it is checked</th>', '<th scope="col">保证</th><th scope="col">检查方式</th>'),
    ('<th scope="row">Frozen wire specification</th>', '<th scope="row">冻结的线上格式规范</th>'),
    ('<td>Normative RFC 2119 spec with 42 format v1 vectors and 23 RPC frame vectors. The reference writer reproduces every canonical <code>.bin</code> byte for byte.</td>',
     '<td>采用 RFC 2119 用语的规范性文档，附带 42 个格式 v1 向量与 23 个 RPC 帧向量；参考写入器逐字节复现每个规范 <code>.bin</code>。</td>'),
    ('<th scope="row">Cross-implementation agreement</th>', '<th scope="row">跨实现一致性</th>'),
    ('<td>An independent Rust schema interpreter and generated code decode every vector and single-byte mutation identically; the JS runtime is fuzzed against Rust across millions of mutated messages and frames.</td>',
     '<td>独立的 Rust schema 解释器与生成代码对所有向量及单字节变异的解码结果完全一致；JS 运行时以 Rust 为基准在数百万条变异消息和帧上进行模糊测试。</td>'),
    ('<th scope="row">Bounded verification &amp; Miri clean</th>', '<th scope="row">有界验证与 Miri 零告警</th>'),
    ('<td><code>verify</code> caps depth at 64 and work at <code>max(len × 8, 1 MiB)</code>. Five fuzz targets ran &gt;80 million inputs with overflow checks, and the sole <code>unsafe</code> UTF-8 fast path runs clean under Miri.</td>',
     '<td><code>verify</code> 将深度限制在 64、总工作量限制在 <code>max(len × 8, 1 MiB)</code>。五个模糊测试目标在开启溢出检查下运行了超过 8000 万个输入，且唯一的 <code>unsafe</code> UTF-8 快速路径在 Miri 下零未定义行为。</td>'),
    ('<th scope="row">Lockfile compatibility enforcement</th>', '<th scope="row">Lockfile 兼容性强制执行</th>'),
    ('<td>15 hard errors reject breaking layout or method edits and 2 warnings require confirmation, verified over a corpus of multi-version schema chains.</td>',
     '<td>15 条硬性错误规则拒绝破坏布局或方法的修改，2 条警告要求显式确认，并在多版本 schema 演化链语料上逐一验证。</td>'),
    ('<p class="more">Implementing Graven in another language? Use the compiler&#39;s lockfile layouts and verify against all <a href="../docs/conformance-vectors.html">conformance vectors</a>.</p>',
     '<p class="more">正在为其他语言实现 Graven？直接使用编译器 lockfile 中的布局定义，并通过全套<a href="../docs/conformance-vectors.html">一致性测试向量</a>验证。</p>'),

    # Footer
    ('Graven · format v1 · MIT license', 'Graven · 格式 v1 · MIT 许可证'),
    ('>Specification</a>', '>规范</a>'),
    ('>Schema language</a>', '>Schema 语言</a>'),
    ('>Services</a>', '>服务</a>'),
    ('>Conformance vectors</a>', '>一致性测试向量</a>'),
    ('>Rust API</a>', '>Rust API</a>'),
    ('>Testing</a>', '>测试</a>'),
]

LANGS = {"pt-br": PT, "es": ES, "zh": ZH}


CJK = "\u3000-\u30ff\u3400-\u9fff\uff00-\uffef"


def render(pairs, cjk=False):
    s = SRC
    for a, b in COMMON + pairs:
        if a not in s:
            sys.exit(f"site/build.py: fragment not found: {a[:70]!r}")
        s = s.replace(a, b)
    if cjk:
        # A line break between CJK characters renders as a stray space; join those lines
        # (outside <pre>, which keeps its own line breaks).
        parts = re.split(r"(<pre.*?</pre>)", s, flags=re.S)
        join = re.compile(rf"(?<=[{CJK}])\n\s*|\n\s*(?=[{CJK}])")
        s = "".join(p if p.startswith("<pre") else join.sub("", p) for p in parts)
    return s


SITE = "https://graven.pages.dev"
REPO = "https://github.com/lpj145/graven"
# Landing page versions: directory ("" for English), hreflang, Open Graph locale.
VERSIONS = [("", "en", "en_US"), ("pt-br", "pt-BR", "pt_BR"), ("es", "es", "es_ES"), ("zh", "zh-CN", "zh_CN")]


def url_of(code):
    return f"{SITE}/{code}/" if code else f"{SITE}/"


def seo_head(page, code):
    """Canonical, language alternates, Open Graph, Twitter and JSON-LD for one landing version."""
    title = html.unescape(re.search(r"<title>(.*?)</title>", page).group(1))
    desc = html.unescape(re.search(r'<meta name="description" content="([^"]*)"', page).group(1))
    lang, locale = next((l, loc) for c, l, loc in VERSIONS if c == code)
    esc = lambda v: html.escape(v, quote=True)
    tags = [f'<link rel="canonical" href="{url_of(code)}">']
    tags += [f'<link rel="alternate" hreflang="{l}" href="{url_of(c)}">' for c, l, _ in VERSIONS]
    tags.append(f'<link rel="alternate" hreflang="x-default" href="{url_of("")}">')
    tags += [
        '<meta property="og:type" content="website">',
        '<meta property="og:site_name" content="Graven">',
        f'<meta property="og:title" content="{esc(title)}">',
        f'<meta property="og:description" content="{esc(desc)}">',
        f'<meta property="og:url" content="{url_of(code)}">',
        f'<meta property="og:image" content="{SITE}/assets/og.png">',
        '<meta property="og:image:width" content="1200">',
        '<meta property="og:image:height" content="630">',
        f'<meta property="og:locale" content="{locale}">',
    ]
    tags += [f'<meta property="og:locale:alternate" content="{loc}">' for c, _, loc in VERSIONS if c != code]
    tags.append('<meta name="twitter:card" content="summary_large_image">')
    ld = {
        "@context": "https://schema.org",
        "@graph": [
            {"@type": "WebSite", "name": "Graven", "url": url_of(code), "inLanguage": lang},
            {
                "@type": "SoftwareSourceCode",
                "name": "Graven",
                "description": desc,
                "url": f"{SITE}/",
                "codeRepository": REPO,
                "programmingLanguage": ["Rust", "JavaScript", "TypeScript"],
                "license": "https://opensource.org/licenses/MIT",
            },
        ],
    }
    tags.append('<script type="application/ld+json">' + json.dumps(ld, ensure_ascii=False) + "</script>")
    return page.replace("</head>", "\n".join(tags) + "\n</head>", 1)


NOT_FOUND = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Page not found — Graven</title>
<meta name="robots" content="noindex">
<link rel="icon" href="/assets/favicon.svg" type="image/svg+xml">
<link rel="stylesheet" href="/assets/site.css?v=CSS_V">
</head>
<body>
<main id="main" class="wrap" style="padding: 96px 16px; max-width: 40em;">
  <h1>Page not found</h1>
  <p>There is no page at this address. It may have moved when the documentation was reorganized.</p>
  <ul>
    <li><a href="/">Graven home</a></li>
    <li><a href="/docs/">Documentation</a></li>
    <li><a href="/docs/spec/graven-format-v1">Format specification</a></li>
  </ul>
</main>
</body>
</html>
"""


def site_files(out, docs):
    """404 page, robots.txt, sitemap.xml and the 301 redirects of moved pages (Cloudflare Pages)."""
    with open(os.path.join(out, "404.html"), "w", encoding="utf8") as f:
        f.write(NOT_FOUND.replace("CSS_V", CSS_V))
    with open(os.path.join(out, "robots.txt"), "w", encoding="utf8") as f:
        f.write(f"User-agent: *\nAllow: /\n\nSitemap: {SITE}/sitemap.xml\n")
    alts = "".join(
        f'\n    <xhtml:link rel="alternate" hreflang="{l}" href="{url_of(c)}"/>' for c, l, _ in VERSIONS
    ) + f'\n    <xhtml:link rel="alternate" hreflang="x-default" href="{url_of("")}"/>'
    urls = [f"  <url>\n    <loc>{url_of(c)}</loc>{alts}\n  </url>" for c, _, _ in VERSIONS]
    urls += [f"  <url>\n    <loc>{u}</loc>\n  </url>" for u in docs.SITEMAP]
    with open(os.path.join(out, "sitemap.xml"), "w", encoding="utf8") as f:
        f.write(
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">\n'
            + "\n".join(urls)
            + "\n</urlset>\n"
        )
    # Moved pages whose new address has no fragment get a real 301; the others keep the
    # HTML redirect docs.py writes (which carries the fragment).
    lines = []
    for old, new in docs.REDIRECTS.items():
        if "#" in new:
            continue
        target = docs.canonical("docs/" + new)[len(SITE):]
        lines += [f"/docs/{old} {target} 301", f"/docs/{old}.html {target} 301"]
        if old.endswith("/index"):
            lines.append(f"/docs/{old[: -len('index')]} {target} 301")
    with open(os.path.join(out, "_redirects"), "w", encoding="utf8") as f:
        f.write("\n".join(lines) + "\n")


def main():
    out = sys.argv[1]
    shutil.copytree(os.path.join(HERE, "assets"), os.path.join(out, "assets"), dirs_exist_ok=True)
    # Search engine ownership checks (Google Search Console): served as-is at the root.
    for name in os.listdir(HERE):
        if name.startswith("google") and name.endswith(".html"):
            shutil.copy(os.path.join(HERE, name), os.path.join(out, name))
    with open(os.path.join(out, "index.html"), "w", encoding="utf8") as f:
        f.write(seo_head(SRC, ""))
    for code, pairs in LANGS.items():
        os.makedirs(os.path.join(out, code), exist_ok=True)
        with open(os.path.join(out, code, "index.html"), "w", encoding="utf8") as f:
            f.write(seo_head(render(pairs, cjk=(code == "zh")), code))
    print("landing: en, " + ", ".join(LANGS))
    import docs  # site/docs.py
    pages = docs.build(out)
    print(f"docs: {len(pages)} pages")
    site_files(out, docs)


if __name__ == "__main__":
    main()
