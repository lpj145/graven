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
    ('<title>Graven: a zero-copy binary format for stored data and RPC</title>',
     '<title>Graven: formato binário zero-copy para dados e RPC</title>'),
    ('content="Zero-copy binary format and RPC protocol with a .lock file for safe evolution. Read fields in place on disk and over the wire."',
     'content="Formato binário zero-copy e protocolo RPC com arquivo .lock para evolução segura. Leia campos no lugar, no disco e na rede."'),
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
    ('<p class="eyebrow">Zero-copy &amp; RPC &amp; MIT</p>',
     '<p class="eyebrow">Zero-copy &amp; RPC &amp; MIT</p>'),
    ('<h1>A zero-copy binary format for stored data and RPC.</h1>',
     '<h1>Formato binário zero-copy para dados armazenados e RPC.</h1>'),
    ('<p class="lede">Write a <code>.graven</code> file to define your tables and services. Generated code reads fields directly at their byte offsets without unpacking the message, using the same layout on disk and over the network.</p>',
     '<p class="lede">Escreva um arquivo <code>.graven</code> para definir suas tabelas e serviços. O código gerado lê cada campo direto no seu offset em bytes sem desempacotar a mensagem, usando o mesmo layout no disco e na rede.</p>'),
    ('<a class="btn primary" href="#start">Get started</a>', '<a class="btn primary" href="#start">Primeiros passos</a>'),
    ('<a class="btn" href="../docs/index.html">Read the docs</a>', '<a class="btn" href="../docs/index.html">Ler a documentação</a>'),
    ('<p class="hero-meta"><a href="../docs/spec/graven-format-v1.html">Format v1 specification</a> · Borrows ideas from FlatBuffers and Cap&#39;n Proto (<a href="../docs/history/index.html">history</a>)</p>',
     '<p class="hero-meta"><a href="../docs/spec/graven-format-v1.html">Especificação do formato v1</a> · Traz ideias do FlatBuffers e do Cap&#39;n Proto (<a href="../docs/history/index.html">histórico</a>)</p>\n      <p class="note">A especificação e a documentação técnica estão em inglês.</p>'),
    ('One message, on disk and on the wire', 'Uma mensagem, no disco e na rede'),
    ('<span class="anatomy-hint">Hover or tap a field</span>',
     '<span class="anatomy-hint">Passe o cursor ou toque em um campo</span>'),
    ('aria-label="Where the message is"', 'aria-label="Onde a mensagem está"'),
    ('aria-label="The 59 bytes of an enveloped Entry message"', 'aria-label="Os 59 bytes de uma mensagem Entry com envelope"'),
    ('aria-label="Fields"', 'aria-label="Campos"'),

    # Band 1: Why
    ('<h2>What you can build with Graven</h2>',
     '<h2>O que você pode construir com o Graven</h2>'),
    ('<p>Logs you read years later, fast RPC services, local caches, or binary IPC where copying and parsing cost too much.</p>',
     '<p>Logs lidos anos depois, serviços RPC rápidos, caches locais ou IPC binário onde copiar e fazer parsing custam caro demais.</p>'),
    ('<h3>Logs, ledgers, and archives</h3>',
     '<h3>Logs, livros-razão e arquivos</h3>'),
    ('<p>Every stored record has a length prefix and a CRC32C, so a crash-truncated tail reads differently from bit rot. Append data for years and read old records with new code without migrating files.</p>',
     '<p>Todo registro gravado leva tamanho e CRC32C, então um final cortado por queda é diferente de corrupção em disco. Dá para gravar por anos e ler registros antigos com código novo sem migrar arquivos.</p>'),
    ('<h3>Low-latency RPC and streams</h3>',
     '<h3>RPC de baixa latência e streams</h3>'),
    ('<p>Put methods, streams, and error unions next to your data definitions. Replies add only 7 bytes of framing over TLS or a socket, and stream items arrive one frame at a time.</p>',
     '<p>Coloque métodos, streams e unions de erro junto das suas definições de dados. Respostas adicionam só 7 bytes de cabeçalho sobre TLS ou socket, e cada item do stream chega em seu próprio frame.</p>'),
    ('<h3>Caches and memory-mapped files</h3>',
     '<h3>Caches e arquivos em memória</h3>'),
    ('<p>Fields live at fixed offsets, and strings or byte slices borrow directly from the input buffer. Reading one field from a large record skips everything else and allocates nothing.</p>',
     '<p>Campos ficam em offsets fixos, e strings ou fatias de bytes vêm direto do buffer de entrada. Ler um campo de um registro grande pula todo o resto e não aloca nada.</p>'),
    ('<h3>Embedded sync and Wasm IPC</h3>',
     '<h3>Sync em embarcados e IPC com Wasm</h3>'),
    ('<p>Call <code>verify</code> once when bytes arrive from an untrusted peer or plugin. Nesting stops at 64 levels and traversal work is bounded, so malformed input cannot blow the stack or stall a reader.</p>',
     '<p>Chame <code>verify</code> uma vez quando chegam bytes de fora ou de um plugin. O aninhamento para em 64 níveis e o trabalho total tem limite, então dados malformados não estouram a pilha nem travam o leitor.</p>'),

    # Band 2: Containers
    ('<h2>Envelopes and frames</h2>',
     '<h2>Envelopes e frames</h2>'),
    ('<p>The <code>Entry</code> message is the same 35 bytes on disk and on the wire. Only the wrapper changes.</p>',
     '<p>A mensagem <code>Entry</code> tem os mesmos 35 bytes no disco e na rede. Só o invólucro muda.</p>'),
    ('<span class="box-name">On disk: envelope</span>',
     '<span class="box-name">No disco: envelope</span>'),
    ('<span class="box-name">On the wire: frame</span>',
     '<span class="box-name">Na rede: frame</span>'),
    ('>header 20</span>', '>cabeçalho 20</span>'),
    ('>message 35</span>', '>mensagem 35</span>'),
    ('<p>Stored bytes live without a connection, so the envelope adds a 20-byte header (magic, version, <code>schema_id</code>, length) and a 4-byte CRC32C at the end.</p>',
     '<p>Bytes em disco vivem fora de uma conexão, então o envelope coloca um cabeçalho de 20 bytes (magic, versão, <code>schema_id</code>, tamanho) e um CRC32C de 4 bytes no final.</p>'),
    ('<p>Over TLS or a socket, the connection handles integrity and the RPC call already knows the type. Replies use a 7-byte header (kind, variant, length); calls use 17 bytes to include the service and method IDs.</p>',
     '<p>Sobre TLS ou socket, a conexão cuida da integridade e a chamada RPC já sabe o tipo. Respostas usam 7 bytes de cabeçalho (tipo, variante, tamanho); chamadas usam 17 bytes para incluir serviço e método.</p>'),
    ('<p class="more">Because those inner 35 bytes are identical, a server can save an incoming RPC payload into a disk envelope without re-encoding it. <a href="../docs/services.html#the-bytes">Frame layout</a></p>',
     '<p class="more">Como esses 35 bytes internos são idênticos, um servidor pode salvar um payload RPC recebido num envelope em disco sem codificar de novo. <a href="../docs/services.html#the-bytes">Layout dos frames</a></p>'),

    # Band 3: Services
    ('<h2>Services</h2>',
     '<h2>Serviços</h2>'),
    ('<p>Services live in the same file as your tables. A method can return a value, a typed error union, a stream, or nothing at all.</p>',
     '<p>Serviços ficam no mesmo arquivo que suas tabelas. Um método pode devolver um valor, uma union de erro tipado, um stream ou nada.</p>'),
    ('<p class="evo-label">Typed client</p>', '<p class="evo-label">Cliente tipado</p>'),
    ('<dt>Typed errors</dt>',
     '<dt>Erros tipados</dt>'),
    ('<dd>Domain errors are part of the method&#39;s return union. Transport and protocol errors stay on a separate path, so you handle them differently.</dd>',
     '<dd>Erros de domínio fazem parte da union de retorno do método. Erros de transporte e protocolo chegam por um caminho separado, para você tratar cada caso como deve.</dd>'),
    ('<dt>Streams</dt>',
     '<dt>Streams</dt>'),
    ('<dd>Each frame starts with its byte length, so receivers yield items as they arrive instead of buffering the whole response.</dd>',
     '<dd>Cada frame começa com o seu tamanho em bytes, então quem recebe entrega os itens conforme chegam, sem precisar guardar a resposta inteira em buffer.</dd>'),
    ('<dt>Validation codes</dt>',
     '<dt>Códigos de validação</dt>'),
    ('<dd>When a rule fails, the wire carries numeric codes for the field, rule, bounds, and reason. No prose strings over the wire, so clients format or translate messages themselves.</dd>',
     '<dd>Quando uma regra falha, trafegam só códigos numéricos de campo, regra, limites e motivo. Nada de frases soltas na rede: o cliente formata ou traduz como quiser.</dd>'),

    # Band 4: Evolution
    ('<h2>Protocol evolution</h2>',
     '<h2>Evolução do protocolo</h2>'),
    ('<p>Every <code>.graven</code> file has a companion <code>.graven.lock</code> that you commit to git. It records the exact binary layout of every version you have shipped. When you change a definition, <code>gravenc update</code> checks your edit against the lock and refuses anything that would break old files or running clients. If the change is safe, it updates the lock.</p>',
     '<p>Todo arquivo <code>.graven</code> tem um <code>.graven.lock</code> que vai para o git. Ele guarda o layout binário exato de cada versão já publicada. Quando você muda uma definição, o <code>gravenc update</code> confere a edição contra esse lock e recusa o que quebraria arquivos antigos ou clientes rodando. Se estiver seguro, ele atualiza o lock.</p>'),
    ('<p class="evo-label">Rejected</p>', '<p class="evo-label">Recusado</p>'),
    ('<p class="evo-label">Accepted</p>', '<p class="evo-label">Aceito</p>'),

    # Band 5: Quickstart
    ('<h2>Quickstart</h2>',
     '<h2>Primeiros passos</h2>'),
    ('<p>Install <code>gravenc</code>, create the initial <code>.lock</code> file, and run codegen.</p>',
     '<p>Instale o <code>gravenc</code>, crie o arquivo <code>.lock</code> inicial e rode o gerador.</p>'),
    ('Install the compiler', 'Instale o compilador'),
    ('Initialize the <code>.lock</code> file', 'Inicialize o arquivo <code>.lock</code>'),
    ('Generate code, write a message, read it back',
     'Gere o código, escreva uma mensagem e leia de volta'),
    ('// Generate with `gravenc gen schemas/bank.graven -o src/gen` (or build.rs)',
     '// Gere com `gravenc gen schemas/bank.graven -o src/gen` (ou via build.rs)'),
    ('// checks CRC32C and runs verify', '// confere CRC32C e executa verify'),
    ('// Generate with `gravenc gen schemas/bank.graven -o src/gen --lang js`',
     '// Gere com `gravenc gen schemas/bank.graven -o src/gen --lang js`'),
    ('<p class="more">Next: the <a href="../docs/getting-started.html">getting started guide</a>, <a href="../docs/coming-from-json.html">coming from JSON</a>, or the <a href="../docs/graven-language.html">language reference</a>.</p>',
     '<p class="more">Depois: o <a href="../docs/getting-started.html">guia de primeiros passos</a>, <a href="../docs/coming-from-json.html">vindo do JSON</a> ou a <a href="../docs/graven-language.html">referência da linguagem</a> (em inglês).</p>'),

    # Band 6: Implementations
    ('<h2>Implementations</h2>',
     '<h2>Implementações</h2>'),
    ('<p>Both runtimes implement the frozen v1 spec and pass all 42 format vectors and 23 RPC vectors.</p>',
     '<p>Os dois runtimes implementam a spec v1 congelada e passam nos 42 vetores de formato e nos 23 vetores RPC.</p>'),
    ('<span class="sdk-badge">Reference · no_std</span>', '<span class="sdk-badge">Referência · no_std</span>'),
    ('<p><code>no_std</code> + <code>alloc</code> runtime (two dependencies), the <code>gravenc</code> compiler, client and server codegen, and a reference interpreter.</p>',
     '<p>Runtime <code>no_std</code> + <code>alloc</code> (duas dependências), o compilador <code>gravenc</code>, geração de cliente e servidor e um interpretador de referência.</p>'),
    ('>Rust SDK</a>', '>SDK Rust</a>'),
    ('>API reference</a>', '>Referência da API</a>'),
    ('<span class="sdk-badge">Zero dependencies</span>', '<span class="sdk-badge">Zero dependências</span>'),
    ('<p>ES2022 modules with <code>.d.ts</code> declarations generated by <code>gravenc gen --lang js</code>. Works in browsers, Node, and Deno with zero runtime dependencies.</p>',
     '<p>Módulos ES2022 com declarações <code>.d.ts</code> gerados por <code>gravenc gen --lang js</code>. Roda em browsers, Node e Deno sem nenhuma dependência em runtime.</p>'),
    ('>JS / TS SDK</a>', '>SDK JS / TS</a>'),
    ('>Add another language</a>', '>Adicionar outra linguagem</a>'),
    ('<p class="more">Writing a runtime in another language? Read layouts from the <code>.lock</code> file and test your encoder and decoder against the <a href="../docs/conformance-vectors.html">conformance vectors</a>.</p>',
     '<p class="more">Quer escrever um runtime em outra linguagem? Leia os layouts do arquivo <code>.lock</code> e teste seu encoder e decoder nos <a href="../docs/conformance-vectors.html">vetores de conformidade</a>.</p>'),
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
    ('<title>Graven: a zero-copy binary format for stored data and RPC</title>',
     '<title>Graven: formato binario zero-copy para datos y RPC</title>'),
    ('content="Zero-copy binary format and RPC protocol with a .lock file for safe evolution. Read fields in place on disk and over the wire."',
     'content="Formato binario zero-copy y protocolo RPC con archivo .lock para evolucionar sin roturas. Lee campos en su lugar en disco y en la red."'),
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
    ('<p class="eyebrow">Zero-copy &amp; RPC &amp; MIT</p>',
     '<p class="eyebrow">Zero-copy &amp; RPC &amp; MIT</p>'),
    ('<h1>A zero-copy binary format for stored data and RPC.</h1>',
     '<h1>Formato binario zero-copy para datos almacenados y RPC.</h1>'),
    ('<p class="lede">Write a <code>.graven</code> file to define your tables and services. Generated code reads fields directly at their byte offsets without unpacking the message, using the same layout on disk and over the network.</p>',
     '<p class="lede">Escribe un archivo <code>.graven</code> para definir tus tablas y servicios. El código generado lee cada campo directo en su offset de bytes sin desempaquetar el mensaje, con el mismo layout en disco y en la red.</p>'),
    ('<a class="btn primary" href="#start">Get started</a>', '<a class="btn primary" href="#start">Primeros pasos</a>'),
    ('<a class="btn" href="../docs/index.html">Read the docs</a>', '<a class="btn" href="../docs/index.html">Leer la documentación</a>'),
    ('<p class="hero-meta"><a href="../docs/spec/graven-format-v1.html">Format v1 specification</a> · Borrows ideas from FlatBuffers and Cap&#39;n Proto (<a href="../docs/history/index.html">history</a>)</p>',
     '<p class="hero-meta"><a href="../docs/spec/graven-format-v1.html">Especificación del formato v1</a> · Toma ideas de FlatBuffers y Cap&#39;n Proto (<a href="../docs/history/index.html">historia</a>)</p>\n      <p class="note">La especificación y la documentación técnica están en inglés.</p>'),
    ('One message, on disk and on the wire', 'Un mensaje, en disco y en la red'),
    ('<span class="anatomy-hint">Hover or tap a field</span>',
     '<span class="anatomy-hint">Pasa el cursor o toca un campo</span>'),
    ('aria-label="Where the message is"', 'aria-label="Dónde está el mensaje"'),
    ('aria-label="The 59 bytes of an enveloped Entry message"', 'aria-label="Los 59 bytes de un mensaje Entry con sobre"'),
    ('aria-label="Fields"', 'aria-label="Campos"'),

    # Band 1: Why
    ('<h2>What you can build with Graven</h2>',
     '<h2>Qué puedes construir con Graven</h2>'),
    ('<p>Logs you read years later, fast RPC services, local caches, or binary IPC where copying and parsing cost too much.</p>',
     '<p>Logs que leerás años después, servicios RPC rápidos, cachés locales o IPC binario donde copiar y parsear cuestan demasiado.</p>'),
    ('<h3>Logs, ledgers, and archives</h3>',
     '<h3>Logs, libros contables y archivos</h3>'),
    ('<p>Every stored record has a length prefix and a CRC32C, so a crash-truncated tail reads differently from bit rot. Append data for years and read old records with new code without migrating files.</p>',
     '<p>Cada registro guardado lleva longitud y CRC32C, así que un final cortado por caída se distingue de bytes dañados. Escribe durante años y lee registros viejos con código nuevo sin migrar archivos.</p>'),
    ('<h3>Low-latency RPC and streams</h3>',
     '<h3>RPC de baja latencia y streams</h3>'),
    ('<p>Put methods, streams, and error unions next to your data definitions. Replies add only 7 bytes of framing over TLS or a socket, and stream items arrive one frame at a time.</p>',
     '<p>Pon métodos, streams y unions de error junto a tus definiciones de datos. Las respuestas añaden solo 7 bytes de cabecera sobre TLS o socket, y cada elemento del stream llega en su propio frame.</p>'),
    ('<h3>Caches and memory-mapped files</h3>',
     '<h3>Cachés y archivos mapeados en memoria</h3>'),
    ('<p>Fields live at fixed offsets, and strings or byte slices borrow directly from the input buffer. Reading one field from a large record skips everything else and allocates nothing.</p>',
     '<p>Los campos están en offsets fijos, y las cadenas o porciones de bytes se toman directo del buffer de entrada. Leer un campo de un registro grande salta todo lo demás y no reserva memoria.</p>'),
    ('<h3>Embedded sync and Wasm IPC</h3>',
     '<h3>Sync embebido e IPC con Wasm</h3>'),
    ('<p>Call <code>verify</code> once when bytes arrive from an untrusted peer or plugin. Nesting stops at 64 levels and traversal work is bounded, so malformed input cannot blow the stack or stall a reader.</p>',
     '<p>Llama a <code>verify</code> una vez cuando llegan bytes de un par o plugin no confiable. El anidamiento se detiene en 64 niveles y el recorrido está acotado, así que datos malformados no desbordan la pila ni bloquean al lector.</p>'),

    # Band 2: Containers
    ('<h2>Envelopes and frames</h2>',
     '<h2>Sobres y frames</h2>'),
    ('<p>The <code>Entry</code> message is the same 35 bytes on disk and on the wire. Only the wrapper changes.</p>',
     '<p>El mensaje <code>Entry</code> son los mismos 35 bytes en disco y en la red. Solo cambia la envoltura.</p>'),
    ('<span class="box-name">On disk: envelope</span>',
     '<span class="box-name">En disco: sobre</span>'),
    ('<span class="box-name">On the wire: frame</span>',
     '<span class="box-name">En la red: frame</span>'),
    ('>header 20</span>', '>cabecera 20</span>'),
    ('>message 35</span>', '>mensaje 35</span>'),
    ('<p>Stored bytes live without a connection, so the envelope adds a 20-byte header (magic, version, <code>schema_id</code>, length) and a 4-byte CRC32C at the end.</p>',
     '<p>Los bytes en disco viven sin conexión, así que el sobre añade una cabecera de 20 bytes (magic, versión, <code>schema_id</code>, longitud) y un CRC32C de 4 bytes al final.</p>'),
    ('<p>Over TLS or a socket, the connection handles integrity and the RPC call already knows the type. Replies use a 7-byte header (kind, variant, length); calls use 17 bytes to include the service and method IDs.</p>',
     '<p>Sobre TLS o un socket, la conexión cuida la integridad y la llamada RPC ya conoce el tipo. Las respuestas usan 7 bytes de cabecera (tipo, variante, longitud); las llamadas usan 17 bytes para incluir servicio y método.</p>'),
    ('<p class="more">Because those inner 35 bytes are identical, a server can save an incoming RPC payload into a disk envelope without re-encoding it. <a href="../docs/services.html#the-bytes">Frame layout</a></p>',
     '<p class="more">Como esos 35 bytes internos son idénticos, un servidor puede guardar un payload RPC entrante en un sobre en disco sin volver a codificarlo. <a href="../docs/services.html#the-bytes">Layout de los frames</a></p>'),

    # Band 3: Services
    ('<h2>Services</h2>',
     '<h2>Servicios</h2>'),
    ('<p>Services live in the same file as your tables. A method can return a value, a typed error union, a stream, or nothing at all.</p>',
     '<p>Los servicios viven en el mismo archivo que tus tablas. Un método puede devolver un valor, una union de error tipado, un stream o nada.</p>'),
    ('<p class="evo-label">Typed client</p>', '<p class="evo-label">Cliente tipado</p>'),
    ('<dt>Typed errors</dt>',
     '<dt>Errores tipados</dt>'),
    ('<dd>Domain errors are part of the method&#39;s return union. Transport and protocol errors stay on a separate path, so you handle them differently.</dd>',
     '<dd>Los errores de dominio forman parte de la union de retorno del método. Los errores de transporte y protocolo van por un camino aparte, para tratarlos por separado.</dd>'),
    ('<dt>Streams</dt>',
     '<dt>Streams</dt>'),
    ('<dd>Each frame starts with its byte length, so receivers yield items as they arrive instead of buffering the whole response.</dd>',
     '<dd>Cada frame empieza con su longitud en bytes, así que quien recibe entrega los elementos conforme llegan sin almacenar toda la respuesta.</dd>'),
    ('<dt>Validation codes</dt>',
     '<dt>Códigos de validación</dt>'),
    ('<dd>When a rule fails, the wire carries numeric codes for the field, rule, bounds, and reason. No prose strings over the wire, so clients format or translate messages themselves.</dd>',
     '<dd>Cuando falla una regla, por la red viajan códigos numéricos para el campo, la regla, los límites y el motivo. Sin textos sueltos en el cable: el cliente formatea o traduce los mensajes.</dd>'),

    # Band 4: Evolution
    ('<h2>Protocol evolution</h2>',
     '<h2>Evolución del protocolo</h2>'),
    ('<p>Every <code>.graven</code> file has a companion <code>.graven.lock</code> that you commit to git. It records the exact binary layout of every version you have shipped. When you change a definition, <code>gravenc update</code> checks your edit against the lock and refuses anything that would break old files or running clients. If the change is safe, it updates the lock.</p>',
     '<p>Cada archivo <code>.graven</code> tiene un <code>.graven.lock</code> que guardas en git. Registra el layout binario exacto de cada versión publicada. Cuando modificas una definición, <code>gravenc update</code> comprueba el cambio contra ese lock y rechaza lo que rompería archivos antiguos o clientes en marcha. Si es seguro, actualiza el lock.</p>'),
    ('<p class="evo-label">Rejected</p>', '<p class="evo-label">Rechazado</p>'),
    ('<p class="evo-label">Accepted</p>', '<p class="evo-label">Aceptado</p>'),

    # Band 5: Quickstart
    ('<h2>Quickstart</h2>',
     '<h2>Primeros pasos</h2>'),
    ('<p>Install <code>gravenc</code>, create the initial <code>.lock</code> file, and run codegen.</p>',
     '<p>Instala <code>gravenc</code>, crea el archivo <code>.lock</code> inicial y ejecuta el generador.</p>'),
    ('Install the compiler', 'Instala el compilador'),
    ('Initialize the <code>.lock</code> file', 'Inicializa el archivo <code>.lock</code>'),
    ('Generate code, write a message, read it back',
     'Genera el código, escribe un mensaje y vuelve a leerlo'),
    ('// Generate with `gravenc gen schemas/bank.graven -o src/gen` (or build.rs)',
     '// Genera con `gravenc gen schemas/bank.graven -o src/gen` (o build.rs)'),
    ('// checks CRC32C and runs verify', '// comprueba CRC32C y ejecuta verify'),
    ('// Generate with `gravenc gen schemas/bank.graven -o src/gen --lang js`',
     '// Genera con `gravenc gen schemas/bank.graven -o src/gen --lang js`'),
    ('<p class="more">Next: the <a href="../docs/getting-started.html">getting started guide</a>, <a href="../docs/coming-from-json.html">coming from JSON</a>, or the <a href="../docs/graven-language.html">language reference</a>.</p>',
     '<p class="more">Después: la <a href="../docs/getting-started.html">guía de primeros pasos</a>, <a href="../docs/coming-from-json.html">viniendo de JSON</a> o la <a href="../docs/graven-language.html">referencia del lenguaje</a> (en inglés).</p>'),

    # Band 6: Implementations
    ('<h2>Implementations</h2>',
     '<h2>Implementaciones</h2>'),
    ('<p>Both runtimes implement the frozen v1 spec and pass all 42 format vectors and 23 RPC vectors.</p>',
     '<p>Ambos runtimes implementan la spec v1 congelada y pasan los 42 vectores de formato y los 23 vectores RPC.</p>'),
    ('<span class="sdk-badge">Reference · no_std</span>', '<span class="sdk-badge">Referencia · no_std</span>'),
    ('<p><code>no_std</code> + <code>alloc</code> runtime (two dependencies), the <code>gravenc</code> compiler, client and server codegen, and a reference interpreter.</p>',
     '<p>Runtime <code>no_std</code> + <code>alloc</code> (dos dependencias), el compilador <code>gravenc</code>, generación de cliente y servidor y un intérprete de referencia.</p>'),
    ('>Rust SDK</a>', '>SDK Rust</a>'),
    ('>API reference</a>', '>Referencia de la API</a>'),
    ('<span class="sdk-badge">Zero dependencies</span>', '<span class="sdk-badge">Cero dependencias</span>'),
    ('<p>ES2022 modules with <code>.d.ts</code> declarations generated by <code>gravenc gen --lang js</code>. Works in browsers, Node, and Deno with zero runtime dependencies.</p>',
     '<p>Módulos ES2022 con declaraciones <code>.d.ts</code> generados por <code>gravenc gen --lang js</code>. Funciona en navegadores, Node y Deno sin dependencias en tiempo de ejecución.</p>'),
    ('>JS / TS SDK</a>', '>SDK JS / TS</a>'),
    ('>Add another language</a>', '>Añadir otro lenguaje</a>'),
    ('<p class="more">Writing a runtime in another language? Read layouts from the <code>.lock</code> file and test your encoder and decoder against the <a href="../docs/conformance-vectors.html">conformance vectors</a>.</p>',
     '<p class="more">¿Vas a escribir un runtime en otro lenguaje? Lee los layouts del archivo <code>.lock</code> y prueba tu encoder y decoder con los <a href="../docs/conformance-vectors.html">vectores de conformidad</a>.</p>'),

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
    ('<title>Graven: a zero-copy binary format for stored data and RPC</title>',
     '<title>Graven：面向存储数据与 RPC 的零拷贝二进制格式</title>'),
    ('content="Zero-copy binary format and RPC protocol with a .lock file for safe evolution. Read fields in place on disk and over the wire."',
     'content="零拷贝二进制格式与 RPC 协议，通过 .lock 文件保障安全演进。在磁盘和网络上原地读取字段。"'),
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
    ('<p class="eyebrow">Zero-copy &amp; RPC &amp; MIT</p>',
     '<p class="eyebrow">零拷贝 · RPC · MIT</p>'),
    ('<h1>A zero-copy binary format for stored data and RPC.</h1>',
     '<h1>面向存储数据与 RPC 的零拷贝二进制格式。</h1>'),
    ('<p class="lede">Write a <code>.graven</code> file to define your tables and services. Generated code reads fields directly at their byte offsets without unpacking the message, using the same layout on disk and over the network.</p>',
     '<p class="lede">编写一个 <code>.graven</code> 文件来定义表和服务。生成的代码无需解包整条消息，直接按字节偏移量读取字段，磁盘存储和网络传输共用同一布局。</p>'),
    ('<a class="btn primary" href="#start">Get started</a>', '<a class="btn primary" href="#start">快速上手</a>'),
    ('<a class="btn" href="../docs/index.html">Read the docs</a>', '<a class="btn" href="../docs/index.html">阅读文档</a>'),
    ('<p class="hero-meta"><a href="../docs/spec/graven-format-v1.html">Format v1 specification</a> · Borrows ideas from FlatBuffers and Cap&#39;n Proto (<a href="../docs/history/index.html">history</a>)</p>',
     '<p class="hero-meta"><a href="../docs/spec/graven-format-v1.html">格式 v1 规范</a> · 借鉴了 FlatBuffers 与 Cap&#39;n Proto（<a href="../docs/history/index.html">设计历程</a>）</p>\n      <p class="note">规范与技术文档为英文。</p>'),
    ('One message, on disk and on the wire', '同一条消息：磁盘上与网络上'),
    ('<span class="anatomy-hint">Hover or tap a field</span>',
     '<span class="anatomy-hint">悬停或点按字段</span>'),
    ('aria-label="Where the message is"', 'aria-label="消息所在位置"'),
    ('aria-label="The 59 bytes of an enveloped Entry message"', 'aria-label="一条带信封的 Entry 消息的 59 个字节"'),
    ('aria-label="Fields"', 'aria-label="字段"'),

    # Band 1: Why
    ('<h2>What you can build with Graven</h2>',
     '<h2>用 Graven 构建什么</h2>'),
    ('<p>Logs you read years later, fast RPC services, local caches, or binary IPC where copying and parsing cost too much.</p>',
     '<p>多年后仍需读取的日志、高速 RPC 服务、本地缓存，或是拷贝与解析开销过高的二进制 IPC。</p>'),
    ('<h3>Logs, ledgers, and archives</h3>',
     '<h3>日志、账本与归档</h3>'),
    ('<p>Every stored record has a length prefix and a CRC32C, so a crash-truncated tail reads differently from bit rot. Append data for years and read old records with new code without migrating files.</p>',
     '<p>每条存储记录都带长度前缀和 CRC32C，崩溃导致的写入截断与字节损坏一目了然。持续追加多年后，新代码依然能直接读取旧记录，无需迁移文件。</p>'),
    ('<h3>Low-latency RPC and streams</h3>',
     '<h3>低延迟 RPC 与流</h3>'),
    ('<p>Put methods, streams, and error unions next to your data definitions. Replies add only 7 bytes of framing over TLS or a socket, and stream items arrive one frame at a time.</p>',
     '<p>把方法、流和错误联合体写在数据结构旁边。在 TLS 或 socket 上，响应仅增加 7 字节帧头，流中的每一项逐帧送达。</p>'),
    ('<h3>Caches and memory-mapped files</h3>',
     '<h3>缓存与内存映射文件</h3>'),
    ('<p>Fields live at fixed offsets, and strings or byte slices borrow directly from the input buffer. Reading one field from a large record skips everything else and allocates nothing.</p>',
     '<p>字段位于固定偏移量，字符串和字节切片直接借用输入缓冲区。从大记录中读取单个字段时跳过其余部分，零内存分配。</p>'),
    ('<h3>Embedded sync and Wasm IPC</h3>',
     '<h3>嵌入式同步与 Wasm IPC</h3>'),
    ('<p>Call <code>verify</code> once when bytes arrive from an untrusted peer or plugin. Nesting stops at 64 levels and traversal work is bounded, so malformed input cannot blow the stack or stall a reader.</p>',
     '<p>当字节来自不可信的对端或插件时，先调用一次 <code>verify</code>。嵌套限制在 64 层且遍历工作量有上限，畸形输入不会撑爆调用栈或卡死读取方。</p>'),

    # Band 2: Containers
    ('<h2>Envelopes and frames</h2>',
     '<h2>信封与帧</h2>'),
    ('<p>The <code>Entry</code> message is the same 35 bytes on disk and on the wire. Only the wrapper changes.</p>',
     '<p><code>Entry</code> 消息在磁盘上和网络上都是同样的 35 个字节，变化的只有外层包装。</p>'),
    ('<span class="box-name">On disk: envelope</span>',
     '<span class="box-name">磁盘上：信封</span>'),
    ('<span class="box-name">On the wire: frame</span>',
     '<span class="box-name">网络上：帧</span>'),
    ('>header 20</span>', '>头部 20</span>'),
    ('>message 35</span>', '>消息 35</span>'),
    ('<p>Stored bytes live without a connection, so the envelope adds a 20-byte header (magic, version, <code>schema_id</code>, length) and a 4-byte CRC32C at the end.</p>',
     '<p>存储在磁盘上的字节没有连接上下文，所以信封加上 20 字节头部（魔数、版本、<code>schema_id</code>、长度）以及末尾 4 字节 CRC32C。</p>'),
    ('<p>Over TLS or a socket, the connection handles integrity and the RPC call already knows the type. Replies use a 7-byte header (kind, variant, length); calls use 17 bytes to include the service and method IDs.</p>',
     '<p>在 TLS 或 socket 上，连接本身保证完整性，RPC 调用也已知晓类型。响应使用 7 字节头部（类型、变体、长度）；调用使用 17 字节以包含服务和方法 ID。</p>'),
    ('<p class="more">Because those inner 35 bytes are identical, a server can save an incoming RPC payload into a disk envelope without re-encoding it. <a href="../docs/services.html#the-bytes">Frame layout</a></p>',
     '<p class="more">因为内部的 35 字节完全相同，服务端可以把收到的 RPC 负载直接存进磁盘信封，无需重新编码。<a href="../docs/services.html#the-bytes">帧布局</a></p>'),

    # Band 3: Services
    ('<h2>Services</h2>',
     '<h2>服务</h2>'),
    ('<p>Services live in the same file as your tables. A method can return a value, a typed error union, a stream, or nothing at all.</p>',
     '<p>服务与表定义在同一个文件中。方法可以返回单个值、类型化错误联合体、流，或者不返回任何值。</p>'),
    ('<p class="evo-label">Typed client</p>', '<p class="evo-label">类型化客户端</p>'),
    ('<dt>Typed errors</dt>',
     '<dt>类型化错误</dt>'),
    ('<dd>Domain errors are part of the method&#39;s return union. Transport and protocol errors stay on a separate path, so you handle them differently.</dd>',
     '<dd>业务错误包含在方法的返回联合体中。传输错误和协议错误走另一条路径，便于分别处理。</dd>'),
    ('<dt>Streams</dt>',
     '<dt>流</dt>'),
    ('<dd>Each frame starts with its byte length, so receivers yield items as they arrive instead of buffering the whole response.</dd>',
     '<dd>每一帧开头都带有字节长度，接收方收到一项即可交付一项，不必缓存整个响应。</dd>'),
    ('<dt>Validation codes</dt>',
     '<dt>校验错误码</dt>'),
    ('<dd>When a rule fails, the wire carries numeric codes for the field, rule, bounds, and reason. No prose strings over the wire, so clients format or translate messages themselves.</dd>',
     '<dd>规则失败时，线上传输的是字段、规则、边界和原因的数字代码。不传提示文本，由客户端自行格式化或翻译。</dd>'),

    # Band 4: Evolution
    ('<h2>Protocol evolution</h2>',
     '<h2>协议演进</h2>'),
    ('<p>Every <code>.graven</code> file has a companion <code>.graven.lock</code> that you commit to git. It records the exact binary layout of every version you have shipped. When you change a definition, <code>gravenc update</code> checks your edit against the lock and refuses anything that would break old files or running clients. If the change is safe, it updates the lock.</p>',
     '<p>每个 <code>.graven</code> 文件都对应一个提交到 git 的 <code>.graven.lock</code>，记录所有已发布版本的精确二进制布局。修改定义后，<code>gravenc update</code> 会拿改动去核对 lock，拒绝会破坏旧文件或在线客户端的修改；只有安全变更才会更新 lock。</p>'),
    ('<p class="evo-label">Rejected</p>', '<p class="evo-label">被拒绝</p>'),
    ('<p class="evo-label">Accepted</p>', '<p class="evo-label">已接受</p>'),

    # Band 5: Quickstart
    ('<h2>Quickstart</h2>',
     '<h2>快速上手</h2>'),
    ('<p>Install <code>gravenc</code>, create the initial <code>.lock</code> file, and run codegen.</p>',
     '<p>安装 <code>gravenc</code>，创建初始 <code>.lock</code> 文件，然后生成代码。</p>'),
    ('Install the compiler', '安装编译器'),
    ('Initialize the <code>.lock</code> file', '初始化 <code>.lock</code> 文件'),
    ('Generate code, write a message, read it back',
     '生成代码，写入一条消息并读回'),
    ('// Generate with `gravenc gen schemas/bank.graven -o src/gen` (or build.rs)',
     '// 使用 `gravenc gen schemas/bank.graven -o src/gen`（或 build.rs）生成代码'),
    ('// checks CRC32C and runs verify', '// 检查 CRC32C 并运行 verify'),
    ('// Generate with `gravenc gen schemas/bank.graven -o src/gen --lang js`',
     '// 使用 `gravenc gen schemas/bank.graven -o src/gen --lang js` 生成代码'),
    ('<p class="more">Next: the <a href="../docs/getting-started.html">getting started guide</a>, <a href="../docs/coming-from-json.html">coming from JSON</a>, or the <a href="../docs/graven-language.html">language reference</a>.</p>',
     '<p class="more">接下来：<a href="../docs/getting-started.html">入门指南</a>、<a href="../docs/coming-from-json.html">从 JSON 迁移</a>或 <a href="../docs/graven-language.html">语言参考</a>（英文）。</p>'),

    # Band 6: Implementations
    ('<h2>Implementations</h2>',
     '<h2>实现</h2>'),
    ('<p>Both runtimes implement the frozen v1 spec and pass all 42 format vectors and 23 RPC vectors.</p>',
     '<p>两个运行时均实现了已冻结的 v1 规范，并通过全部 42 个格式向量与 23 个 RPC 向量。</p>'),
    ('<span class="sdk-badge">Reference · no_std</span>', '<span class="sdk-badge">参考实现 · no_std</span>'),
    ('<p><code>no_std</code> + <code>alloc</code> runtime (two dependencies), the <code>gravenc</code> compiler, client and server codegen, and a reference interpreter.</p>',
     '<p><code>no_std</code> + <code>alloc</code> 运行时（两个依赖）、<code>gravenc</code> 编译器、客户端与服务端代码生成以及参考解释器。</p>'),
    ('>Rust SDK</a>', '>Rust SDK</a>'),
    ('>API reference</a>', '>API 参考</a>'),
    ('<span class="sdk-badge">Zero dependencies</span>', '<span class="sdk-badge">零依赖</span>'),
    ('<p>ES2022 modules with <code>.d.ts</code> declarations generated by <code>gravenc gen --lang js</code>. Works in browsers, Node, and Deno with zero runtime dependencies.</p>',
     '<p>由 <code>gravenc gen --lang js</code> 生成的 ES2022 模块及 <code>.d.ts</code> 声明，零运行时依赖，适用于浏览器、Node 和 Deno。</p>'),
    ('>JS / TS SDK</a>', '>JS / TS SDK</a>'),
    ('>Add another language</a>', '>添加其他语言</a>'),
    ('<p class="more">Writing a runtime in another language? Read layouts from the <code>.lock</code> file and test your encoder and decoder against the <a href="../docs/conformance-vectors.html">conformance vectors</a>.</p>',
     '<p class="more">想用其他语言编写运行时？从 <code>.lock</code> 文件读取布局，并用<a href="../docs/conformance-vectors.html">一致性测试向量</a>验证你的编解码器。</p>'),

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
