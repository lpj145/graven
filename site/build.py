#!/usr/bin/env python3
"""Render the landing page (site/index.html, English) into target/site/, translate it, and
render the documentation (site/docs.py).

Usage: python3 site/build.py <out_dir>    (needs site/requirements.txt)

Each translation replaces exact English fragments of index.html; a fragment that is no
longer found fails the build, so the English page and its translations cannot drift apart
silently. The specification and the technical docs stay in English (the normative text).
"""
import os
import re
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = open(os.path.join(HERE, "index.html"), encoding="utf8").read()

COMMON = [  # shared by every translation: paths one level down
    ('href="docs/', 'href="../docs/'),
    ('href="api/', 'href="../api/'),
    ('href="assets/', 'href="../assets/'),
    ('src="assets/', 'src="../assets/'),
    ('<a class="wordmark" href="./"', '<a class="wordmark" href="../"'),
    ('<link rel="alternate" hreflang="en" href="./">', '<link rel="alternate" hreflang="en" href="../">'),
    ('href="pt-br/"', 'href="../pt-br/"'),
    ('href="es/"', 'href="../es/"'),
    ('href="zh/"', 'href="../zh/"'),
    ('<a href="./" hreflang="en" lang="en" aria-current="page">English</a>', '<a href="../" hreflang="en" lang="en">English</a>'),
]

PT = [
    ("Inspired by FlatBuffers and Cap'n Proto. We started from their ideas, measured their trade-offs in benchmarks, and kept exploring until we reached this format.", "Inspirado no FlatBuffers e no Cap'n Proto. Partimos das ideias deles, medimos seus trade-offs em benchmarks e seguimos explorando até chegar a este formato."),
    (">How we got here</a>", ">Como chegamos aqui</a>"),
    ('<html lang="en">', '<html lang="pt-BR">'),
    ('<title>Graven — an open specification for a zero-copy binary format</title>',
     '<title>Graven — uma especificação aberta para um formato binário zero-copy</title>'),
    ('content="Graven is an open specification for a zero-copy binary format and its schema language, with frozen conformance vectors and a reference implementation in Rust."',
     'content="Graven é uma especificação aberta para um formato binário zero-copy e sua linguagem de schema, com vetores de conformidade congelados e uma implementação de referência em Rust."'),
    ('Skip to content', 'Pular para o conteúdo'),
    ('aria-label="Graven home"', 'aria-label="Página inicial do Graven"'),
    ('>Specification</a>', '>Especificação</a>'),
    ('>Schema language</a>', '>Linguagem de schema</a>'),
    ('>Docs</a>', '>Documentação</a>'),
    ('>Implementations</a>', '>Implementações</a>'),
    ('<a href="../pt-br/" hreflang="pt-BR" lang="pt-BR">Português</a>', '<a href="./" hreflang="pt-BR" lang="pt-BR" aria-current="page">Português</a>'),
    ('<h1>A binary format for data that has to last.</h1>', '<h1>Um formato binário para dados que precisam durar.</h1>'),
    ('''Graven is an open specification for a zero-copy serialization format and its schema language.
        Messages are read in place, straight from the buffer. The format is frozen and fully specified, and its
        conformance vectors let an implementation in any language prove that it reads every message correctly.''',
     '''Graven é uma especificação aberta para um formato de serialização zero-copy e sua linguagem de schema.
        As mensagens são lidas no lugar, direto do buffer. O formato está congelado e totalmente especificado, e seus
        vetores de conformidade permitem que uma implementação em qualquer linguagem prove que lê cada mensagem corretamente.'''),
    ('>Read the specification</a>', '>Ler a especificação</a>'),
    ('Format v1 is frozen. Reference implementation in Rust. MIT licensed.',
     'Formato v1 congelado. Implementação de referência em Rust. Licença MIT.</p>\n      <p class="note">A especificação e a documentação técnica estão em inglês.'),
    ('One message, byte by byte', 'Uma mensagem, byte a byte'),
    ('aria-label="The 59 bytes of an enveloped Entry message"', 'aria-label="Os 59 bytes de uma mensagem Entry com envelope"'),
    ('aria-label="Fields"', 'aria-label="Campos"'),
    ('<h2>Built for data that outlives the code that wrote it</h2>', '<h2>Feito para dados que duram mais que o código que os escreveu</h2>'),
    ('''Business data is written once and read for years: by the next release, by a service that upgrades
        later, by an audit long after the original team has moved on. Graven is designed around that reality.''',
     '''Dados de negócio são escritos uma vez e lidos por anos: pela próxima versão, por um serviço que só atualiza
        depois, por uma auditoria muito tempo após a equipe original ter seguido em frente. O Graven foi pensado para essa realidade.'''),
    ('<dt>Read in place</dt>', '<dt>Leitura no lugar</dt>'),
    ('''Fields are read directly from the bytes. Strings and byte slices borrow from the buffer, so reading a
          large record costs nothing until you touch a field.''',
     '''Os campos são lidos direto dos bytes. Strings e fatias de bytes são emprestadas do buffer, então ler um
          registro grande não custa nada até você tocar num campo.'''),
    ('<dt>Evolve without guessing</dt>', '<dt>Evoluir sem adivinhar</dt>'),
    ('''A lockfile records every layout. The compiler refuses changes that would make old and new code
          disagree, and asks for explicit confirmation before anything breaks an older reader.''',
     '''Um lockfile registra cada layout. O compilador recusa mudanças que fariam código antigo e novo
          discordarem, e pede confirmação explícita antes de qualquer coisa quebrar um leitor antigo.'''),
    ('<dt>Untrusted bytes are safe to open</dt>', '<dt>Bytes não confiáveis podem ser abertos com segurança</dt>'),
    ('''A single <code>verify</code> pass walks the whole message under fixed depth and work limits. After
          that, a verified view reads without re-checking, and malformed input can only ever produce an error.''',
     '''Uma única passada de <code>verify</code> percorre a mensagem inteira com limites fixos de profundidade e de trabalho.
          Depois disso, a view verificada lê sem checar de novo, e entrada malformada só consegue produzir um erro.'''),
    ('<dt>Know what happened to a file</dt>', '<dt>Saber o que aconteceu com um arquivo</dt>'),
    ('''The optional envelope carries a schema id and a CRC32C. A write cut short by a crash is reported as
          incomplete; a flipped bit is reported as corruption. They are never confused.''',
     '''O envelope opcional carrega um schema id e um CRC32C. Uma escrita interrompida por uma queda é reportada como
          incompleta; um bit trocado é reportado como corrupção. Os dois nunca se confundem.'''),
    ('<h2>How the format is kept honest</h2>', '<h2>Como o formato se mantém confiável</h2>'),
    ('Every claim the specification makes is pinned by something that runs. This is what is checked, and how.',
     'Cada afirmação da especificação é garantida por algo que roda. Isto é o que é verificado, e como.'),
    ('<th scope="col">Guarantee</th><th scope="col">How it is checked</th>', '<th scope="col">Garantia</th><th scope="col">Como é verificada</th>'),
    ('<th scope="row">The wire format does not drift</th>', '<th scope="row">O formato não muda por acidente</th>'),
    ('''A normative specification (RFC 2119 language) and 42 frozen conformance vectors, each a <code>.bin</code>
            with its expected result. The reference writer must reproduce the canonical vectors byte for byte.''',
     '''Uma especificação normativa (linguagem RFC 2119) e 42 vetores de conformidade congelados, cada um um <code>.bin</code>
            com seu resultado esperado. O writer de referência precisa reproduzir os vetores canônicos byte a byte.'''),
    ('<th scope="row">Two implementations agree</th>', '<th scope="row">Duas implementações concordam</th>'),
    ('''Within the reference implementation, an independent interpreter driven by the schema as data and the
            generated code read every vector and every single-byte mutation of it. They must accept, reject and decode
            identically. Every new implementation is held to the same vectors.''',
     '''Dentro da implementação de referência, um interpretador independente guiado pelo schema como dado e o
            código gerado leem cada vetor e cada mutação de um byte dele. Os dois precisam aceitar, recusar e decodificar
            da mesma forma. Toda nova implementação é cobrada pelos mesmos vetores.'''),
    ('<th scope="row">Hostile input stays bounded</th>', '<th scope="row">Entrada hostil fica limitada</th>'),
    ('''limits depth to 64 and total work to <code>max(len × 8, 1 MiB)</code>, so cycles and
            shared-data amplification are rejected. Five fuzz targets ran more than 80 million inputs with overflow
            checks enabled, with no findings.''',
     '''limita a profundidade a 64 e o trabalho total a <code>max(len × 8, 1 MiB)</code>, então ciclos e
            amplificação por dados compartilhados são recusados. Cinco alvos de fuzzing rodaram mais de 80 milhões de entradas
            com checagem de overflow ligada, sem nenhum achado.'''),
    ('<th scope="row">The unsafe fast path is sound</th>', '<th scope="row">O caminho rápido com unsafe é correto</th>'),
    ('''In the Rust implementation, the only <code>unsafe</code> code skips UTF-8 revalidation after <code>verify</code>. Its invariants are
            written down, enforced by a mutation gate, and the runtime and generated code run under Miri with no
            undefined behaviour reported.''',
     '''Na implementação em Rust, o único código <code>unsafe</code> pula a revalidação de UTF-8 depois do <code>verify</code>. Suas invariantes
            estão documentadas, são cobradas por um teste de mutação, e o runtime e o código gerado rodam sob o Miri sem
            nenhum comportamento indefinido reportado.'''),
    ('<th scope="row">Schema changes cannot corrupt silently</th>', '<th scope="row">Mudanças de schema não corrompem em silêncio</th>'),
    ('''The compiler compares every change against the lockfile: 15 rules reject changes that would misread
            old data, 2 warnings require confirmation in the schema. A corpus of version chains tests each rule.''',
     '''O compilador compara cada mudança com o lockfile: 15 regras recusam mudanças que leriam dados antigos
            errado, e 2 avisos exigem confirmação no schema. Um corpus de cadeias de versões testa cada regra.'''),
    ('<th scope="row">Crashes and corruption are told apart</th>', '<th scope="row">Queda e corrupção são diferenciadas</th>'),
    ('''The envelope's check order is normative, so a truncated tail and a damaged record produce different,
            predictable errors on every implementation.''',
     '''A ordem de checagem do envelope é normativa, então um final truncado e um registro danificado produzem erros
            diferentes e previsíveis em todas as implementações.'''),
    ('<h2>Schema changes are reviewed by the compiler</h2>', '<h2>Mudanças de schema são revisadas pelo compilador</h2>'),
    ('''Change a type or add a required field, and <code>gravenc update</code> refuses, naming the rule and the line.
        Add the field with a default instead, and the new lock is written. This is real output.''',
     '''Mude um tipo ou acrescente um campo obrigatório, e o <code>gravenc update</code> recusa, citando a regra e a linha.
        Acrescente o campo com um default, e o novo lock é escrito. Esta é a saída real.'''),
    ('<p class="evo-label">Rejected</p>', '<p class="evo-label">Recusado</p>'),
    ('<p class="evo-label">Accepted</p>', '<p class="evo-label">Aceito</p>'),
    ('<h2>What it is for</h2>', '<h2>Para que serve</h2>'),
    ('Graven fits wherever records are written once, kept for a long time and read by software that changes.',
     'O Graven se encaixa onde registros são escritos uma vez, guardados por muito tempo e lidos por software que muda.'),
    ('<dt>Ledgers and event logs</dt>', '<dt>Livros-razão e logs de eventos</dt>'),
    ('''Append envelopes to a file and never rewrite them. Each record carries its schema id and checksum, a
          crash leaves a detectably incomplete tail, and reports can scan the file without deserializing it.''',
     '''Acrescente envelopes a um arquivo e nunca os reescreva. Cada registro carrega seu schema id e checksum, uma
          queda deixa um final reconhecidamente incompleto, e relatórios podem varrer o arquivo sem desserializá-lo.'''),
    ('<dt>Service and SDK protocols</dt>', '<dt>Protocolos de serviços e SDKs</dt>'),
    ('''Route requests by schema id, verify once at the edge and pass the verified view along without copying.
          The compatibility rules mean an older client keeps working when the server moves ahead.''',
     '''Roteie requisições pelo schema id, verifique uma vez na entrada e repasse a view verificada sem copiar.
          As regras de compatibilidade garantem que um cliente antigo continue funcionando quando o servidor avança.'''),
    ('<dt>Archives and audit trails</dt>', '<dt>Arquivos e trilhas de auditoria</dt>'),
    ('''A frozen format with public conformance vectors is something you can still decode in ten years, with
          a different implementation if need be.''',
     '''Um formato congelado com vetores de conformidade públicos é algo que você ainda consegue decodificar daqui a dez anos,
          com outra implementação se for preciso.'''),
    ('<dt>Devices and offline sync</dt>', '<dt>Dispositivos e sincronização offline</dt>'),
    ('''The format needs no allocator and no alignment, and the Rust runtime is <code>no_std</code> with two
          dependencies, so the same records can come from point-of-sale terminals and embedded devices that batch
          them while offline.''',
     '''O formato não precisa de alocador nem de alinhamento, e o runtime em Rust é <code>no_std</code> com duas
          dependências, então os mesmos registros podem vir de terminais de ponto de venda e dispositivos embarcados que
          os acumulam enquanto estão offline.'''),
    ('<dt>Caches and storage values</dt>', '<dt>Caches e valores armazenados</dt>'),
    ('Store records in a key-value store and read only the field you need, straight from the stored bytes.',
     'Guarde registros num banco chave-valor e leia só o campo de que precisa, direto dos bytes armazenados.'),
    ('<dt>Plugin boundaries</dt>', '<dt>Fronteiras com plugins</dt>'),
    ('''Exchange buffers with untrusted extensions, for example WebAssembly modules, and let <code>verify</code>
          protect the host from whatever they send back.''',
     '''Troque buffers com extensões não confiáveis, como módulos WebAssembly, e deixe o <code>verify</code>
          proteger o host de qualquer coisa que elas devolvam.'''),
    ('<h2>Implementations</h2>', '<h2>Implementações</h2>'),
    ('''The specification is the product; implementations are measured against it. A conforming implementation
        reads all 42 vectors with exactly the expected result (spec §10).''',
     '''A especificação é o produto; as implementações são medidas por ela. Uma implementação conforme
        lê os 42 vetores com exatamente o resultado esperado (spec §10).'''),
    ('<th scope="col">Language</th><th scope="col">Status</th><th scope="col">What it includes</th>',
     '<th scope="col">Linguagem</th><th scope="col">Status</th><th scope="col">O que inclui</th>'),
    ('<td>Reference implementation. Passes all v1 and RPC frame vectors.</td>', '<td>Implementação de referência. Passa em todos os vetores v1 e de frames RPC.</td>'),
    ('''Runtime (<code>no_std</code>), the <code>gravenc</code> compiler with lockfile and compatibility
            checks, Rust code generation (client and server edge), and the conformance interpreter.''',
     '''Runtime (<code>no_std</code>), o compilador <code>gravenc</code> com lockfile e checagem de
            compatibilidade, geração de código Rust (cliente e borda de servidor) e o interpretador de conformidade.'''),
    ('>Rust SDK</a>', '>SDK Rust</a>'),
    ('<td><code>gravenc gen --lang js</code>. Passes all v1 and RPC frame vectors.</td>',
     '<td><code>gravenc gen --lang js</code>. Passa em todos os vetores v1 e de frames RPC.</td>'),
    ('''Zero-dependency ES2022 <code>.js</code> modules with TypeScript <code>.d.ts</code> declarations,
            canonical writers, validation, and RPC clients for browsers, Node, and Deno.''',
     '''Módulos ES2022 <code>.js</code> sem dependências com declarações TypeScript <code>.d.ts</code>,
            writers canônicos, validação e clientes RPC para navegadores, Node e Deno.'''),
    ('>JS / TS SDK</a>', '>SDK JS / TS</a>'),
    ('>API reference</a>', '>Referência da API</a>'),
    ('<h3>Writing an implementation</h3>', '<h3>Escrevendo uma implementação</h3>'),
    ('''Start with the layout algorithm (spec §3.3), the read rules (§4–§6) and <code>verify</code> (§7), then run
          the <a href="../docs/conformance-vectors.html">conformance vectors</a>. Each vector is a <code>.bin</code> with a
          <code>.json</code> stating the decoded value or the exact error, and each invalid one has a single defect,
          so a failure points at one rule.''',
     '''Comece pelo algoritmo de layout (spec §3.3), pelas regras de leitura (§4–§6) e pelo <code>verify</code> (§7), e então rode
          os <a href="../docs/conformance-vectors.html">vetores de conformidade</a>. Cada vetor é um <code>.bin</code> com um
          <code>.json</code> que traz o valor decodificado ou o erro exato, e cada vetor inválido tem um único defeito,
          então uma falha aponta para uma única regra.'''),
    ('<h3>Generating code for another language</h3>', '<h3>Gerando código para outra linguagem</h3>'),
    ('''The schema language and its compatibility rules are language-neutral. The lockfile written by
          <code>gravenc</code> already records every table's layout, schema id and field presence, which is what a code
          generator for another language needs to emit readers and writers.''',
     '''A linguagem de schema e suas regras de compatibilidade não dependem de linguagem. O lockfile escrito pelo
          <code>gravenc</code> já registra o layout, o schema id e a presença dos campos de cada tabela, que é o que um
          gerador de código para outra linguagem precisa para emitir leitores e writers.'''),
    ('<h2>Getting started with Rust</h2>', '<h2>Primeiros passos com Rust</h2>'),
    ('Using the reference implementation needs only the Rust toolchain.', 'Usar a implementação de referência exige só o toolchain do Rust.'),
    ('<h3>Install the compiler</h3>', '<h3>Instale o compilador</h3>'),
    ('<h3>Write a schema and create its lockfile</h3>', '<h3>Escreva um schema e crie seu lockfile</h3>'),
    ('<h3>Generate code from <code>build.rs</code></h3>', '<h3>Gere o código no <code>build.rs</code></h3>'),
    ('<h3>Write and read</h3>', '<h3>Escreva e leia</h3>'),
    ('''The <a href="../docs/getting-started.html">getting started guide</a> and the
      <a href="../docs/graven-language.html">schema language reference</a> cover the full workflow.''',
     '''O <a href="../docs/getting-started.html">guia de primeiros passos</a> e a
      <a href="../docs/graven-language.html">referência da linguagem de schema</a> (em inglês) cobrem o fluxo completo.'''),
    ('Graven · format v1 · MIT license', 'Graven · formato v1 · licença MIT'),
    ('aria-label="Footer"', 'aria-label="Rodapé"'),
    ('aria-label="Main"', 'aria-label="Principal"'),
    ('aria-label="Language"', 'aria-label="Idioma"'),
    ('>Conformance vectors</a>', '>Vetores de conformidade</a>'),
    ('>Rust API</a>', '>API Rust</a>'),
    ('>Testing</a>', '>Testes</a>'),
]

ES = [
    ("Inspired by FlatBuffers and Cap'n Proto. We started from their ideas, measured their trade-offs in benchmarks, and kept exploring until we reached this format.", "Inspirado en FlatBuffers y Cap'n Proto. Partimos de sus ideas, medimos sus compromisos con benchmarks y seguimos explorando hasta llegar a este formato."),
    (">How we got here</a>", ">Cómo llegamos aquí</a>"),
    ('<html lang="en">', '<html lang="es">'),
    ('<title>Graven — an open specification for a zero-copy binary format</title>',
     '<title>Graven — una especificación abierta para un formato binario zero-copy</title>'),
    ('content="Graven is an open specification for a zero-copy binary format and its schema language, with frozen conformance vectors and a reference implementation in Rust."',
     'content="Graven es una especificación abierta para un formato binario zero-copy y su lenguaje de esquemas, con vectores de conformidad congelados y una implementación de referencia en Rust."'),
    ('Skip to content', 'Saltar al contenido'),
    ('aria-label="Graven home"', 'aria-label="Inicio de Graven"'),
    ('>Specification</a>', '>Especificación</a>'),
    ('>Schema language</a>', '>Lenguaje de esquemas</a>'),
    ('>Docs</a>', '>Documentación</a>'),
    ('>Implementations</a>', '>Implementaciones</a>'),
    ('<a href="../es/" hreflang="es" lang="es">Español</a>', '<a href="./" hreflang="es" lang="es" aria-current="page">Español</a>'),
    ('<h1>A binary format for data that has to last.</h1>', '<h1>Un formato binario para datos que tienen que durar.</h1>'),
    ('''Graven is an open specification for a zero-copy serialization format and its schema language.
        Messages are read in place, straight from the buffer. The format is frozen and fully specified, and its
        conformance vectors let an implementation in any language prove that it reads every message correctly.''',
     '''Graven es una especificación abierta para un formato de serialización zero-copy y su lenguaje de esquemas.
        Los mensajes se leen en su lugar, directamente del buffer. El formato está congelado y completamente especificado, y sus
        vectores de conformidad permiten que una implementación en cualquier lenguaje demuestre que lee cada mensaje correctamente.'''),
    ('>Read the specification</a>', '>Leer la especificación</a>'),
    ('Format v1 is frozen. Reference implementation in Rust. MIT licensed.',
     'Formato v1 congelado. Implementación de referencia en Rust. Licencia MIT.</p>\n      <p class="note">La especificación y la documentación técnica están en inglés.'),
    ('One message, byte by byte', 'Un mensaje, byte a byte'),
    ('aria-label="The 59 bytes of an enveloped Entry message"', 'aria-label="Los 59 bytes de un mensaje Entry con sobre"'),
    ('aria-label="Fields"', 'aria-label="Campos"'),
    ('<h2>Built for data that outlives the code that wrote it</h2>', '<h2>Hecho para datos que sobreviven al código que los escribió</h2>'),
    ('''Business data is written once and read for years: by the next release, by a service that upgrades
        later, by an audit long after the original team has moved on. Graven is designed around that reality.''',
     '''Los datos de negocio se escriben una vez y se leen durante años: por la próxima versión, por un servicio que se
        actualiza más tarde, por una auditoría mucho después de que el equipo original haya cambiado. Graven está diseñado para esa realidad.'''),
    ('<dt>Read in place</dt>', '<dt>Lectura en su lugar</dt>'),
    ('''Fields are read directly from the bytes. Strings and byte slices borrow from the buffer, so reading a
          large record costs nothing until you touch a field.''',
     '''Los campos se leen directamente de los bytes. Las cadenas y los segmentos de bytes se toman prestados del buffer,
          así que leer un registro grande no cuesta nada hasta que tocas un campo.'''),
    ('<dt>Evolve without guessing</dt>', '<dt>Evolucionar sin adivinar</dt>'),
    ('''A lockfile records every layout. The compiler refuses changes that would make old and new code
          disagree, and asks for explicit confirmation before anything breaks an older reader.''',
     '''Un lockfile registra cada layout. El compilador rechaza los cambios que harían que el código antiguo y el nuevo
          no coincidan, y pide confirmación explícita antes de que algo rompa a un lector antiguo.'''),
    ('<dt>Untrusted bytes are safe to open</dt>', '<dt>Los bytes no confiables se abren con seguridad</dt>'),
    ('''A single <code>verify</code> pass walks the whole message under fixed depth and work limits. After
          that, a verified view reads without re-checking, and malformed input can only ever produce an error.''',
     '''Una sola pasada de <code>verify</code> recorre todo el mensaje con límites fijos de profundidad y de trabajo.
          Después, una vista verificada lee sin volver a comprobar, y una entrada mal formada solo puede producir un error.'''),
    ('<dt>Know what happened to a file</dt>', '<dt>Saber qué le pasó a un archivo</dt>'),
    ('''The optional envelope carries a schema id and a CRC32C. A write cut short by a crash is reported as
          incomplete; a flipped bit is reported as corruption. They are never confused.''',
     '''El sobre opcional lleva un schema id y un CRC32C. Una escritura interrumpida por una caída se informa como
          incompleta; un bit alterado se informa como corrupción. Nunca se confunden.'''),
    ('<h2>How the format is kept honest</h2>', '<h2>Cómo se mantiene fiable el formato</h2>'),
    ('Every claim the specification makes is pinned by something that runs. This is what is checked, and how.',
     'Cada afirmación de la especificación está respaldada por algo que se ejecuta. Esto es lo que se comprueba, y cómo.'),
    ('<th scope="col">Guarantee</th><th scope="col">How it is checked</th>', '<th scope="col">Garantía</th><th scope="col">Cómo se comprueba</th>'),
    ('<th scope="row">The wire format does not drift</th>', '<th scope="row">El formato no cambia por accidente</th>'),
    ('''A normative specification (RFC 2119 language) and 42 frozen conformance vectors, each a <code>.bin</code>
            with its expected result. The reference writer must reproduce the canonical vectors byte for byte.''',
     '''Una especificación normativa (lenguaje RFC 2119) y 42 vectores de conformidad congelados, cada uno un <code>.bin</code>
            con su resultado esperado. El escritor de referencia debe reproducir los vectores canónicos byte a byte.'''),
    ('<th scope="row">Two implementations agree</th>', '<th scope="row">Dos implementaciones coinciden</th>'),
    ('''Within the reference implementation, an independent interpreter driven by the schema as data and the
            generated code read every vector and every single-byte mutation of it. They must accept, reject and decode
            identically. Every new implementation is held to the same vectors.''',
     '''Dentro de la implementación de referencia, un intérprete independiente guiado por el esquema como dato y el
            código generado leen cada vector y cada mutación de un byte. Deben aceptar, rechazar y decodificar
            igual. Toda nueva implementación se mide con los mismos vectores.'''),
    ('<th scope="row">Hostile input stays bounded</th>', '<th scope="row">La entrada hostil queda acotada</th>'),
    ('''limits depth to 64 and total work to <code>max(len × 8, 1 MiB)</code>, so cycles and
            shared-data amplification are rejected. Five fuzz targets ran more than 80 million inputs with overflow
            checks enabled, with no findings.''',
     '''limita la profundidad a 64 y el trabajo total a <code>max(len × 8, 1 MiB)</code>, así que los ciclos y la
            amplificación por datos compartidos se rechazan. Cinco objetivos de fuzzing ejecutaron más de 80 millones de entradas
            con comprobación de desbordamiento activada, sin hallazgos.'''),
    ('<th scope="row">The unsafe fast path is sound</th>', '<th scope="row">El camino rápido con unsafe es correcto</th>'),
    ('''In the Rust implementation, the only <code>unsafe</code> code skips UTF-8 revalidation after <code>verify</code>. Its invariants are
            written down, enforced by a mutation gate, and the runtime and generated code run under Miri with no
            undefined behaviour reported.''',
     '''En la implementación en Rust, el único código <code>unsafe</code> omite la revalidación de UTF-8 después de <code>verify</code>. Sus invariantes
            están documentadas, se hacen cumplir con una prueba de mutación, y el runtime y el código generado se ejecutan bajo Miri sin
            ningún comportamiento indefinido reportado.'''),
    ('<th scope="row">Schema changes cannot corrupt silently</th>', '<th scope="row">Los cambios de esquema no corrompen en silencio</th>'),
    ('''The compiler compares every change against the lockfile: 15 rules reject changes that would misread
            old data, 2 warnings require confirmation in the schema. A corpus of version chains tests each rule.''',
     '''El compilador compara cada cambio con el lockfile: 15 reglas rechazan los cambios que leerían mal
            datos antiguos, y 2 avisos exigen confirmación en el esquema. Un corpus de cadenas de versiones prueba cada regla.'''),
    ('<th scope="row">Crashes and corruption are told apart</th>', '<th scope="row">Caídas y corrupción se distinguen</th>'),
    ('''The envelope's check order is normative, so a truncated tail and a damaged record produce different,
            predictable errors on every implementation.''',
     '''El orden de comprobación del sobre es normativo, así que un final truncado y un registro dañado producen errores
            distintos y predecibles en todas las implementaciones.'''),
    ('<h2>Schema changes are reviewed by the compiler</h2>', '<h2>El compilador revisa los cambios de esquema</h2>'),
    ('''Change a type or add a required field, and <code>gravenc update</code> refuses, naming the rule and the line.
        Add the field with a default instead, and the new lock is written. This is real output.''',
     '''Cambia un tipo o añade un campo obligatorio, y <code>gravenc update</code> lo rechaza, indicando la regla y la línea.
        Añade el campo con un valor por defecto, y se escribe el nuevo lock. Esta es la salida real.'''),
    ('<p class="evo-label">Rejected</p>', '<p class="evo-label">Rechazado</p>'),
    ('<p class="evo-label">Accepted</p>', '<p class="evo-label">Aceptado</p>'),
    ('<h2>What it is for</h2>', '<h2>Para qué sirve</h2>'),
    ('Graven fits wherever records are written once, kept for a long time and read by software that changes.',
     'Graven encaja donde los registros se escriben una vez, se guardan mucho tiempo y los lee software que cambia.'),
    ('<dt>Ledgers and event logs</dt>', '<dt>Libros contables y logs de eventos</dt>'),
    ('''Append envelopes to a file and never rewrite them. Each record carries its schema id and checksum, a
          crash leaves a detectably incomplete tail, and reports can scan the file without deserializing it.''',
     '''Añade sobres a un archivo y nunca los reescribas. Cada registro lleva su schema id y su checksum, una
          caída deja un final detectablemente incompleto, y los informes pueden recorrer el archivo sin deserializarlo.'''),
    ('<dt>Service and SDK protocols</dt>', '<dt>Protocolos de servicios y SDKs</dt>'),
    ('''Route requests by schema id, verify once at the edge and pass the verified view along without copying.
          The compatibility rules mean an older client keeps working when the server moves ahead.''',
     '''Enruta las peticiones por schema id, verifica una vez en la entrada y pasa la vista verificada sin copiar.
          Las reglas de compatibilidad hacen que un cliente antiguo siga funcionando cuando el servidor avanza.'''),
    ('<dt>Archives and audit trails</dt>', '<dt>Archivos y registros de auditoría</dt>'),
    ('''A frozen format with public conformance vectors is something you can still decode in ten years, with
          a different implementation if need be.''',
     '''Un formato congelado con vectores de conformidad públicos es algo que todavía podrás decodificar dentro de diez años,
          con otra implementación si hace falta.'''),
    ('<dt>Devices and offline sync</dt>', '<dt>Dispositivos y sincronización sin conexión</dt>'),
    ('''The format needs no allocator and no alignment, and the Rust runtime is <code>no_std</code> with two
          dependencies, so the same records can come from point-of-sale terminals and embedded devices that batch
          them while offline.''',
     '''El formato no necesita asignador de memoria ni alineación, y el runtime en Rust es <code>no_std</code> con dos
          dependencias, así que los mismos registros pueden venir de terminales de punto de venta y dispositivos embebidos
          que los acumulan mientras están sin conexión.'''),
    ('<dt>Caches and storage values</dt>', '<dt>Cachés y valores almacenados</dt>'),
    ('Store records in a key-value store and read only the field you need, straight from the stored bytes.',
     'Guarda registros en un almacén clave-valor y lee solo el campo que necesitas, directamente de los bytes guardados.'),
    ('<dt>Plugin boundaries</dt>', '<dt>Fronteras con plugins</dt>'),
    ('''Exchange buffers with untrusted extensions, for example WebAssembly modules, and let <code>verify</code>
          protect the host from whatever they send back.''',
     '''Intercambia buffers con extensiones no confiables, por ejemplo módulos WebAssembly, y deja que <code>verify</code>
          proteja al host de lo que devuelvan.'''),
    ('<h2>Implementations</h2>', '<h2>Implementaciones</h2>'),
    ('''The specification is the product; implementations are measured against it. A conforming implementation
        reads all 42 vectors with exactly the expected result (spec §10).''',
     '''La especificación es el producto; las implementaciones se miden con ella. Una implementación conforme
        lee los 42 vectores con exactamente el resultado esperado (spec §10).'''),
    ('<th scope="col">Language</th><th scope="col">Status</th><th scope="col">What it includes</th>',
     '<th scope="col">Lenguaje</th><th scope="col">Estado</th><th scope="col">Qué incluye</th>'),
    ('<td>Reference implementation. Passes all v1 and RPC frame vectors.</td>', '<td>Implementación de referencia. Pasa todos los vectores v1 y de tramas RPC.</td>'),
    ('''Runtime (<code>no_std</code>), the <code>gravenc</code> compiler with lockfile and compatibility
            checks, Rust code generation (client and server edge), and the conformance interpreter.''',
     '''Runtime (<code>no_std</code>), el compilador <code>gravenc</code> con lockfile y comprobación de
            compatibilidad, generación de código Rust (cliente y servidor) y el intérprete de conformidad.'''),
    ('>Rust SDK</a>', '>SDK Rust</a>'),
    ('<td><code>gravenc gen --lang js</code>. Passes all v1 and RPC frame vectors.</td>',
     '<td><code>gravenc gen --lang js</code>. Pasa todos los vectores v1 y de tramas RPC.</td>'),
    ('''Zero-dependency ES2022 <code>.js</code> modules with TypeScript <code>.d.ts</code> declarations,
            canonical writers, validation, and RPC clients for browsers, Node, and Deno.''',
     '''Módulos ES2022 <code>.js</code> sin dependencias con declaraciones TypeScript <code>.d.ts</code>,
            writers canónicos, validación y clientes RPC para navegadores, Node y Deno.'''),
    ('>JS / TS SDK</a>', '>SDK JS / TS</a>'),
    ('>API reference</a>', '>Referencia de la API</a>'),
    ('<h3>Writing an implementation</h3>', '<h3>Escribir una implementación</h3>'),
    ('''Start with the layout algorithm (spec §3.3), the read rules (§4–§6) and <code>verify</code> (§7), then run
          the <a href="../docs/conformance-vectors.html">conformance vectors</a>. Each vector is a <code>.bin</code> with a
          <code>.json</code> stating the decoded value or the exact error, and each invalid one has a single defect,
          so a failure points at one rule.''',
     '''Empieza por el algoritmo de layout (spec §3.3), las reglas de lectura (§4–§6) y <code>verify</code> (§7), y luego ejecuta
          los <a href="../docs/conformance-vectors.html">vectores de conformidad</a>. Cada vector es un <code>.bin</code> con un
          <code>.json</code> que indica el valor decodificado o el error exacto, y cada vector inválido tiene un único defecto,
          así que un fallo apunta a una sola regla.'''),
    ('<h3>Generating code for another language</h3>', '<h3>Generar código para otro lenguaje</h3>'),
    ('''The schema language and its compatibility rules are language-neutral. The lockfile written by
          <code>gravenc</code> already records every table's layout, schema id and field presence, which is what a code
          generator for another language needs to emit readers and writers.''',
     '''El lenguaje de esquemas y sus reglas de compatibilidad son independientes del lenguaje. El lockfile que escribe
          <code>gravenc</code> ya registra el layout, el schema id y la presencia de los campos de cada tabla, que es lo que un
          generador de código para otro lenguaje necesita para emitir lectores y escritores.'''),
    ('<h2>Getting started with Rust</h2>', '<h2>Primeros pasos con Rust</h2>'),
    ('Using the reference implementation needs only the Rust toolchain.', 'Usar la implementación de referencia solo requiere el toolchain de Rust.'),
    ('<h3>Install the compiler</h3>', '<h3>Instala el compilador</h3>'),
    ('<h3>Write a schema and create its lockfile</h3>', '<h3>Escribe un esquema y crea su lockfile</h3>'),
    ('<h3>Generate code from <code>build.rs</code></h3>', '<h3>Genera el código desde <code>build.rs</code></h3>'),
    ('<h3>Write and read</h3>', '<h3>Escribe y lee</h3>'),
    ('''The <a href="../docs/getting-started.html">getting started guide</a> and the
      <a href="../docs/graven-language.html">schema language reference</a> cover the full workflow.''',
     '''La <a href="../docs/getting-started.html">guía de primeros pasos</a> y la
      <a href="../docs/graven-language.html">referencia del lenguaje de esquemas</a> (en inglés) cubren el flujo completo.'''),
    ('Graven · format v1 · MIT license', 'Graven · formato v1 · licencia MIT'),
    ('aria-label="Footer"', 'aria-label="Pie de página"'),
    ('aria-label="Main"', 'aria-label="Principal"'),
    ('aria-label="Language"', 'aria-label="Idioma"'),
    ('>Conformance vectors</a>', '>Vectores de conformidad</a>'),
    ('>Rust API</a>', '>API de Rust</a>'),
    ('>Testing</a>', '>Pruebas</a>'),
]

ZH = [
    ("Inspired by FlatBuffers and Cap'n Proto. We started from their ideas, measured their trade-offs in benchmarks, and kept exploring until we reached this format.", "受 FlatBuffers 和 Cap'n Proto 启发。我们从它们的思路出发，用基准测试衡量各自的取舍，并持续探索，最终得到了这个格式。"),
    (">How we got here</a>", ">我们是如何走到这里的</a>"),
    ('<html lang="en">', '<html lang="zh-CN">'),
    ('<title>Graven — an open specification for a zero-copy binary format</title>',
     '<title>Graven — 零拷贝二进制格式的开放规范</title>'),
    ('content="Graven is an open specification for a zero-copy binary format and its schema language, with frozen conformance vectors and a reference implementation in Rust."',
     'content="Graven 是一个零拷贝二进制格式及其 schema 语言的开放规范，附带冻结的一致性测试向量和 Rust 参考实现。"'),
    ('Skip to content', '跳到正文'),
    ('aria-label="Graven home"', 'aria-label="Graven 首页"'),
    ('>Specification</a>', '>规范</a>'),
    ('>Schema language</a>', '>Schema 语言</a>'),
    ('>Docs</a>', '>文档</a>'),
    ('>Implementations</a>', '>实现</a>'),
    ('<a href="../zh/" hreflang="zh-CN" lang="zh-CN">中文</a>', '<a href="./" hreflang="zh-CN" lang="zh-CN" aria-current="page">中文</a>'),
    ('<h1>A binary format for data that has to last.</h1>', '<h1>为需要长期保存的数据而设计的二进制格式。</h1>'),
    ('''Graven is an open specification for a zero-copy serialization format and its schema language.
        Messages are read in place, straight from the buffer. The format is frozen and fully specified, and its
        conformance vectors let an implementation in any language prove that it reads every message correctly.''',
     '''Graven 是一个零拷贝序列化格式及其 schema 语言的开放规范。
        消息直接在缓冲区中原地读取。格式已经冻结并有完整的规范，
        其一致性测试向量让任何语言的实现都能证明自己正确读取每一条消息。'''),
    ('>Read the specification</a>', '>阅读规范</a>'),
    ('Format v1 is frozen. Reference implementation in Rust. MIT licensed.',
     '格式 v1 已冻结。Rust 参考实现。MIT 许可证。</p>\n      <p class="note">规范和技术文档为英文。'),
    ('One message, byte by byte', '逐字节解读一条消息'),
    ('aria-label="The 59 bytes of an enveloped Entry message"', 'aria-label="一条带信封的 Entry 消息的 59 个字节"'),
    ('aria-label="Fields"', 'aria-label="字段"'),
    ('<h2>Built for data that outlives the code that wrote it</h2>', '<h2>为比写入它的代码活得更久的数据而生</h2>'),
    ('''Business data is written once and read for years: by the next release, by a service that upgrades
        later, by an audit long after the original team has moved on. Graven is designed around that reality.''',
     '''业务数据只写一次，却要被读取很多年：被下一个版本读取，被稍后才升级的服务读取，
        被原团队早已离开之后的审计读取。Graven 正是围绕这一现实设计的。'''),
    ('<dt>Read in place</dt>', '<dt>原地读取</dt>'),
    ('''Fields are read directly from the bytes. Strings and byte slices borrow from the buffer, so reading a
          large record costs nothing until you touch a field.''',
     '''字段直接从字节中读取。字符串和字节切片借用自缓冲区，
          因此在访问某个字段之前，读取一条大记录没有任何开销。'''),
    ('<dt>Evolve without guessing</dt>', '<dt>演进无需猜测</dt>'),
    ('''A lockfile records every layout. The compiler refuses changes that would make old and new code
          disagree, and asks for explicit confirmation before anything breaks an older reader.''',
     '''lockfile 记录每一个布局。编译器会拒绝导致新旧代码不一致的修改，
          并在任何修改破坏旧版读取方之前要求显式确认。'''),
    ('<dt>Untrusted bytes are safe to open</dt>', '<dt>可以安全地打开不可信的字节</dt>'),
    ('''A single <code>verify</code> pass walks the whole message under fixed depth and work limits. After
          that, a verified view reads without re-checking, and malformed input can only ever produce an error.''',
     '''一次 <code>verify</code> 在固定的深度和工作量限制下遍历整条消息。
          之后，已验证视图读取时无需再次检查，而格式错误的输入只会产生错误。'''),
    ('<dt>Know what happened to a file</dt>', '<dt>清楚文件发生了什么</dt>'),
    ('''The optional envelope carries a schema id and a CRC32C. A write cut short by a crash is reported as
          incomplete; a flipped bit is reported as corruption. They are never confused.''',
     '''可选的信封携带 schema id 和 CRC32C。因崩溃而中断的写入被报告为不完整；
          翻转的比特被报告为损坏。两者绝不会混淆。'''),
    ('<h2>How the format is kept honest</h2>', '<h2>格式如何保持可信</h2>'),
    ('Every claim the specification makes is pinned by something that runs. This is what is checked, and how.',
     '规范中的每一项承诺都由可运行的检查来保证。以下是检查的内容和方式。'),
    ('<th scope="col">Guarantee</th><th scope="col">How it is checked</th>', '<th scope="col">保证</th><th scope="col">检查方式</th>'),
    ('<th scope="row">The wire format does not drift</th>', '<th scope="row">线上格式不会漂移</th>'),
    ('''A normative specification (RFC 2119 language) and 42 frozen conformance vectors, each a <code>.bin</code>
            with its expected result. The reference writer must reproduce the canonical vectors byte for byte.''',
     '''一份规范性文档（采用 RFC 2119 用语）和 42 个冻结的一致性测试向量，每个向量是一个 <code>.bin</code>
            及其预期结果。参考写入器必须逐字节复现规范向量。'''),
    ('<th scope="row">Two implementations agree</th>', '<th scope="row">两种实现结果一致</th>'),
    ('''Within the reference implementation, an independent interpreter driven by the schema as data and the
            generated code read every vector and every single-byte mutation of it. They must accept, reject and decode
            identically. Every new implementation is held to the same vectors.''',
     '''在参考实现内部，一个以 schema 为数据驱动的独立解释器和生成的代码会读取每个向量及其每一个单字节变异。
            二者必须以完全相同的方式接受、拒绝和解码。每个新实现都以同样的向量来检验。'''),
    ('<th scope="row">Hostile input stays bounded</th>', '<th scope="row">恶意输入受到限制</th>'),
    ('''limits depth to 64 and total work to <code>max(len × 8, 1 MiB)</code>, so cycles and
            shared-data amplification are rejected. Five fuzz targets ran more than 80 million inputs with overflow
            checks enabled, with no findings.''',
     '''将深度限制为 64，总工作量限制为 <code>max(len × 8, 1 MiB)</code>，因此循环和共享数据放大都会被拒绝。
            五个模糊测试目标在开启溢出检查的情况下运行了超过 8000 万个输入，没有发现任何问题。'''),
    ('<th scope="row">The unsafe fast path is sound</th>', '<th scope="row">unsafe 快速路径是可靠的</th>'),
    ('''In the Rust implementation, the only <code>unsafe</code> code skips UTF-8 revalidation after <code>verify</code>. Its invariants are
            written down, enforced by a mutation gate, and the runtime and generated code run under Miri with no
            undefined behaviour reported.''',
     '''在 Rust 实现中，唯一的 <code>unsafe</code> 代码是在 <code>verify</code> 之后跳过 UTF-8 重新校验。其不变量有书面说明，
            并由变异测试强制保证；运行时和生成的代码在 Miri 下运行，未报告任何未定义行为。'''),
    ('<th scope="row">Schema changes cannot corrupt silently</th>', '<th scope="row">Schema 修改不会悄悄破坏数据</th>'),
    ('''The compiler compares every change against the lockfile: 15 rules reject changes that would misread
            old data, 2 warnings require confirmation in the schema. A corpus of version chains tests each rule.''',
     '''编译器将每次修改与 lockfile 对比：15 条规则拒绝会误读旧数据的修改，
            2 条警告要求在 schema 中确认。一组版本链语料测试每一条规则。'''),
    ('<th scope="row">Crashes and corruption are told apart</th>', '<th scope="row">崩溃与损坏可以区分</th>'),
    ('''The envelope's check order is normative, so a truncated tail and a damaged record produce different,
            predictable errors on every implementation.''',
     '''信封的检查顺序是规范性的，因此被截断的尾部和损坏的记录在每个实现上都会产生不同且可预期的错误。'''),
    ('<h2>Schema changes are reviewed by the compiler</h2>', '<h2>Schema 修改由编译器审查</h2>'),
    ('''Change a type or add a required field, and <code>gravenc update</code> refuses, naming the rule and the line.
        Add the field with a default instead, and the new lock is written. This is real output.''',
     '''修改字段类型或添加必填字段时，<code>gravenc update</code> 会拒绝，并指出规则和行号。
        改为添加带默认值的字段，新的 lock 就会写入。以下是真实输出。'''),
    ('<p class="evo-label">Rejected</p>', '<p class="evo-label">被拒绝</p>'),
    ('<p class="evo-label">Accepted</p>', '<p class="evo-label">被接受</p>'),
    ('<h2>What it is for</h2>', '<h2>适用场景</h2>'),
    ('Graven fits wherever records are written once, kept for a long time and read by software that changes.',
     '凡是记录只写一次、长期保存、并由不断变化的软件读取的地方，都适合使用 Graven。'),
    ('<dt>Ledgers and event logs</dt>', '<dt>账本与事件日志</dt>'),
    ('''Append envelopes to a file and never rewrite them. Each record carries its schema id and checksum, a
          crash leaves a detectably incomplete tail, and reports can scan the file without deserializing it.''',
     '''将信封追加到文件中，永不改写。每条记录都带有自己的 schema id 和校验和，
          崩溃只会留下可检测的不完整尾部，报表可以直接扫描文件而无需反序列化。'''),
    ('<dt>Service and SDK protocols</dt>', '<dt>服务与 SDK 协议</dt>'),
    ('''Route requests by schema id, verify once at the edge and pass the verified view along without copying.
          The compatibility rules mean an older client keeps working when the server moves ahead.''',
     '''按 schema id 路由请求，在入口处验证一次，然后无拷贝地传递已验证视图。
          兼容性规则确保服务器升级后，旧版客户端仍能正常工作。'''),
    ('<dt>Archives and audit trails</dt>', '<dt>归档与审计记录</dt>'),
    ('''A frozen format with public conformance vectors is something you can still decode in ten years, with
          a different implementation if need be.''',
     '''一个拥有公开一致性测试向量的冻结格式，十年后依然可以解码，必要时还可以换用另一种实现。'''),
    ('<dt>Devices and offline sync</dt>', '<dt>设备与离线同步</dt>'),
    ('''The format needs no allocator and no alignment, and the Rust runtime is <code>no_std</code> with two
          dependencies, so the same records can come from point-of-sale terminals and embedded devices that batch
          them while offline.''',
     '''该格式不需要内存分配器，也不需要对齐；Rust 运行时支持 <code>no_std</code>，只有两个依赖，
          因此同样的记录可以来自收银终端和嵌入式设备，在离线时批量积累。'''),
    ('<dt>Caches and storage values</dt>', '<dt>缓存与存储值</dt>'),
    ('Store records in a key-value store and read only the field you need, straight from the stored bytes.',
     '把记录存入键值存储，只读取需要的字段，直接从存储的字节中读取。'),
    ('<dt>Plugin boundaries</dt>', '<dt>插件边界</dt>'),
    ('''Exchange buffers with untrusted extensions, for example WebAssembly modules, and let <code>verify</code>
          protect the host from whatever they send back.''',
     '''与不可信的扩展（例如 WebAssembly 模块）交换缓冲区，由 <code>verify</code>
          保护宿主免受它们返回的任何数据的影响。'''),
    ('<h2>Implementations</h2>', '<h2>实现</h2>'),
    ('''The specification is the product; implementations are measured against it. A conforming implementation
        reads all 42 vectors with exactly the expected result (spec §10).''',
     '''规范本身就是产品，各个实现都以它为衡量标准。一个符合规范的实现必须以完全符合预期的结果读取全部 42 个向量（规范 §10）。'''),
    ('<th scope="col">Language</th><th scope="col">Status</th><th scope="col">What it includes</th>',
     '<th scope="col">语言</th><th scope="col">状态</th><th scope="col">包含内容</th>'),
    ('<td>Reference implementation. Passes all v1 and RPC frame vectors.</td>', '<td>参考实现。通过全部 v1 与 RPC 帧向量。</td>'),
    ('''Runtime (<code>no_std</code>), the <code>gravenc</code> compiler with lockfile and compatibility
            checks, Rust code generation (client and server edge), and the conformance interpreter.''',
     '''运行时（<code>no_std</code>）、带 lockfile 和兼容性检查的 <code>gravenc</code> 编译器、
            Rust 代码生成（客户端与服务端边缘），以及一致性解释器。'''),
    ('>Rust SDK</a>', '>Rust SDK</a>'),
    ('<td><code>gravenc gen --lang js</code>. Passes all v1 and RPC frame vectors.</td>',
     '<td><code>gravenc gen --lang js</code>。通过全部 v1 与 RPC 帧向量。</td>'),
    ('''Zero-dependency ES2022 <code>.js</code> modules with TypeScript <code>.d.ts</code> declarations,
            canonical writers, validation, and RPC clients for browsers, Node, and Deno.''',
     '''零依赖 ES2022 <code>.js</code> 模块与 TypeScript <code>.d.ts</code> 声明、规范写入器、
            校验及适用于浏览器、Node 和 Deno 的 RPC 客户端。'''),
    ('>JS / TS SDK</a>', '>JS / TS SDK</a>'),
    ('>API reference</a>', '>API 参考</a>'),
    ('<h3>Writing an implementation</h3>', '<h3>编写一个实现</h3>'),
    ('''Start with the layout algorithm (spec §3.3), the read rules (§4–§6) and <code>verify</code> (§7), then run
          the <a href="../docs/conformance-vectors.html">conformance vectors</a>. Each vector is a <code>.bin</code> with a
          <code>.json</code> stating the decoded value or the exact error, and each invalid one has a single defect,
          so a failure points at one rule.''',
     '''从布局算法（规范 §3.3）、读取规则（§4–§6）和 <code>verify</code>（§7）开始，然后运行
          <a href="../docs/conformance-vectors.html">一致性测试向量</a>。每个向量是一个 <code>.bin</code> 加一个
          <code>.json</code>，后者给出解码后的值或确切的错误；每个无效向量只有一个缺陷，
          因此一次失败只指向一条规则。'''),
    ('<h3>Generating code for another language</h3>', '<h3>为其他语言生成代码</h3>'),
    ('''The schema language and its compatibility rules are language-neutral. The lockfile written by
          <code>gravenc</code> already records every table's layout, schema id and field presence, which is what a code
          generator for another language needs to emit readers and writers.''',
     '''schema 语言及其兼容性规则与编程语言无关。<code>gravenc</code> 写入的 lockfile
          已经记录了每张表的布局、schema id 和字段存在性，这正是其他语言的代码生成器生成读取器和写入器所需要的。'''),
    ('<h2>Getting started with Rust</h2>', '<h2>使用 Rust 快速上手</h2>'),
    ('Using the reference implementation needs only the Rust toolchain.', '使用参考实现只需要 Rust 工具链。'),
    ('<h3>Install the compiler</h3>', '<h3>安装编译器</h3>'),
    ('<h3>Write a schema and create its lockfile</h3>', '<h3>编写 schema 并创建 lockfile</h3>'),
    ('<h3>Generate code from <code>build.rs</code></h3>', '<h3>在 <code>build.rs</code> 中生成代码</h3>'),
    ('<h3>Write and read</h3>', '<h3>写入与读取</h3>'),
    ('''The <a href="../docs/getting-started.html">getting started guide</a> and the
      <a href="../docs/graven-language.html">schema language reference</a> cover the full workflow.''',
     '''<a href="../docs/getting-started.html">快速上手指南</a>和
      <a href="../docs/graven-language.html">schema 语言参考</a>（英文）涵盖完整的工作流程。'''),
    ('Graven · format v1 · MIT license', 'Graven · 格式 v1 · MIT 许可证'),
    ('aria-label="Footer"', 'aria-label="页脚"'),
    ('aria-label="Main"', 'aria-label="主导航"'),
    ('aria-label="Language"', 'aria-label="语言"'),
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


def main():
    out = sys.argv[1]
    shutil.copytree(os.path.join(HERE, "assets"), os.path.join(out, "assets"), dirs_exist_ok=True)
    with open(os.path.join(out, "index.html"), "w", encoding="utf8") as f:
        f.write(SRC)
    for code, pairs in LANGS.items():
        os.makedirs(os.path.join(out, code), exist_ok=True)
        with open(os.path.join(out, code, "index.html"), "w", encoding="utf8") as f:
            f.write(render(pairs, cjk=(code == "zh")))
    print("landing: en, " + ", ".join(LANGS))
    import docs  # site/docs.py
    pages = docs.build(out)
    print(f"docs: {len(pages)} pages")


if __name__ == "__main__":
    main()
