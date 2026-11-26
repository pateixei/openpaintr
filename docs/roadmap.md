# Roadmap do OpenPaintR

**Status** — Roadmap aprovado em 25 de novembro de 2026. Última atualização em 26 de novembro de 2026.

O primeiro envio do código ao GitHub usa o nome **OpenPaintR**, com executável `openpaintr`. A edição em camadas, a seleção e geometria da fase 2 e as ferramentas da fase 3 estão implementadas. A validação de compatibilidade entre versões do Paint.NET continua na fase 1. A próxima fase de implementação é a infraestrutura de plugins e o catálogo de ajustes e efeitos. [Repositório no GitHub](https://github.com/pateixei/openpaintr)

A suíte tem uma falha conhecida no teste de arraste do canto inferior direito depois de entrar no redimensionamento da tela com Ctrl e clique duplo. O teste ainda precisa passar para concluir a validação desse gesto.

A sequência começa por medir a experiência atual no Linux e transformar a importação de PDN em um fluxo de edição com salvamento em camadas. Os resultados da medição orientam o escopo e o momento do redesenho da interface. Em seguida entram seleção, ferramentas e transformações, os 13 ajustes e os 48 efeitos exibidos nas telas do Paint.NET 5.2, e a ampliação de formatos, desempenho e extensibilidade. Os marcos indicam dependências e critérios de aceite, sem datas de lançamento.

O OpenPaintR já cria imagens, abre PNG e PDN, exporta PNG, oferece pincel e borracha básicos, zoom, adição de camadas, visibilidade, opacidade e desfazer/refazer. A importação preserva propriedades de camada, incluindo o modo de mesclagem. A abertura com visualização das camadas foi confirmada pelo usuário em um arquivo. A compatibilidade por versão e a equivalência de composição continuam sendo critérios a medir. [Estado atual](../README.md)

**Regra de interoperabilidade.** O OpenPaintR grava documentos editáveis em formatos de intercâmbio. PDN deve ser validado no Paint.NET e PSD no Adobe Photoshop. As versões e os recursos preservados fazem parte dos critérios de aceite.

**Regra de antialiasing.** Linhas, curvas, traços, preenchimentos e seleções usam supersampling **SSAA (4×4)** ativo 100% do tempo, tanto na prévia quanto nos pixels gravados na camada.

**Regra de plugins e efeitos.** Todos os efeitos do menu **Efeitos** devem ser implementados como plugins no formato portátil multiplataforma do OpenPaintR (`.rpdn-plugin`: **WebAssembly + shader `WGSL` opcional + UI declarativa**), consumindo a mesma API pública destinada a autores externos por meio da biblioteca mínima `rustpdn-plugin-sdk`. A especificação técnica e o guia de criação estão em [Arquitetura de plugins](arquitetura-plugins.md) e [Guia de extensibilidade](extensibilidade.md).

## Fase 1 — Documentos, camadas e persistência

- [x] **Arquivo e área de trabalho.** Completar Novo, Abrir, Abrir recentes e limpar a lista, Salvar, Salvar como, opções de salvamento com prévia, Exportar, Fechar, Fechar todos, Sair e confirmação por documento com alterações. Abrir vários documentos em abas reordenáveis com miniaturas, alternar para o próximo/anterior e oferecer lista de documentos. Manter histórico, seleção, zoom e posição independentes por documento.
- [x] **Camadas.** Completar adicionar, excluir, duplicar, mesclar com a camada abaixo, alternar visibilidade e importar de arquivo. Navegar para a primeira, anterior, próxima e última camada. Mover para o topo, subir, descer e mover para a base, incluindo reordenação por arraste. Exibir miniaturas e oferecer propriedades de nome, visibilidade, opacidade e modo de mesclagem.
- [ ] **Composição.** Validar os modos Normal, Multiply, Additive, Color Burn, Color Dodge, Reflect, Glow, Overlay, Difference, Negation, Lighten, Darken, Screen e Xor com amostras de alfa parcial, camadas ocultas e várias sobreposições. Medir as diferenças de arredondamento entre v3 e v5.2 para estabelecer a referência por versão. [Modos de mesclagem](https://paint.net/doc/latest/BlendModes.html)
- [x] **Histórico.** Desfazer e refazer operações de documento, camada e seleção, navegar para qualquer estado pelo painel, consolidar gestos em uma entrada e limitar o uso de memória. Avaliar armazenamento temporário de diffs para documentos grandes.
- [ ] **Persistência editável.** Gravar arquivos PDN com substituição atômica para Salvar e Salvar como. A reabertura deve preservar dimensões, pixels RGBA, ordem, nomes, visibilidade, opacidade e modos de mesclagem e permitir continuar a edição. Rastrear a última revisão salva por documento, independentemente do histórico de desfazer. Preservar o documento aberto e a última gravação válida em caso de falha ou cancelamento. Definir quais estados de sessão, como zoom, seleção e histórico, serão persistidos.
- [ ] **Compatibilidade e exportação.** Testar a importação de PDN 3.x, 4.x e 5.x com amostras versionadas e verificar separadamente a composição e o alfa do PNG exportado. Preservar o original ao usar Salvar como. Validar a reabertura dos arquivos gerados no OpenPaintR e nas versões de Paint.NET declaradas como compatíveis.
- [ ] **Interface de trabalho.** Definir o acesso às operações entregues nesta fase. Usar o inventário de menus, painéis, barra contextual e barra de estado para planejar o redesenho. Avaliar a posição, a visibilidade e a restauração dos painéis, com ícones e identidade próprios, depois da medição inicial. Expor os comandos de seleção, ajustes e efeitos à medida que estiverem disponíveis.
- [ ] **Medição inicial no Linux.** Fixar uma versão do editor atual e verificar se todas as tarefas funcionam antes das sessões. Registrar resultados de usabilidade e de desempenho separadamente, conforme o plano abaixo.

**Aceite** — Abrir um PDN, reorganizar, duplicar e mesclar camadas e desfazer as mudanças. Testar Salvar, Fechar e Reabrir como um fluxo próprio, conferindo pixels e propriedades das camadas. Comparar o PNG exportado com imagens de referência produzidas no Paint.NET. Registrar a medição inicial no Linux e as dificuldades observadas para decidir o escopo e o momento do redesenho. [Menus e ações do v3](https://github.com/nakijun/OpenPDN-1/tree/cca476b0df2a2f70996e6b9486ec45327631568c/src/Menus) · [Documentação do v5](https://paint.net/doc/latest/index.html)

### Primeira entrega da fase 1

O editor grava PDN com camadas. Salvar e Salvar como usam gravação atômica e atualizam a revisão salva após o sucesso. O histórico, a camada ativa e o zoom pertencem à sessão. O Paint.NET 5.2 abriu o PDN de 958 × 810 pixels gerado a partir do caso de recuperação com quatro camadas. A verificação pixel a pixel e das propriedades das camadas foi feita na reabertura pelo OpenPaintR. A matriz para outras versões do Paint.NET continua em validação.

A primeira medição técnica no Linux usou o executável de depuração de referência, preservado em `target/baseline/rustpdn-pre-persistencia` (SHA-256 `c5ab51a1d99e08c4d7b4eb91ebd2d07757172b62442ff2c6c3f19ceea4ab6d0c`). Em WSL2 6.18.33.2, Core i7-13700K, foi medido o tempo total do comando `rustpdn pdn entrada.pdn saída.png`, com uma execução de aquecimento e dez medidas por arquivo. As medições incluem abertura, composição, exportação e o custo do processo.

| Amostra (800 × 600) | Camadas | Mediana | Máximo |
| --- | ---: | ---: | ---: |
| Untitled | 1 | 175,6 ms | 182,8 ms |
| Untitled3 | 2 | 168,7 ms | 172,7 ms |
| oldPDN3510 | 2 | 249,7 ms | 340,7 ms |
| FlattenBlendTest | 14 | 297,4 ms | 317,5 ms |

A comparação do PNG de `oldPDN3510` com a referência local registrou inicialmente 45 pixels divergentes (ImageMagick, métrica AE) por diferença de arredondamento em `Pixel::over`. Após unificar `PDNB_NORMAL` com `bm_row_inplace` (`jflib-gfx` / `UserBlendOps`), a comparação passou a registrar **0 pixels divergentes** (`tests/pdn_compat.rs`). A medição interativa e as sessões de usabilidade continuam pendentes.

### Usabilidade, desempenho e redesenho

Antes de testar com participantes, registrar a versão do aplicativo, as amostras, o ambiente Linux, a escala de tela e o dispositivo de entrada. Usar documentos equivalentes com camadas visíveis e ocultas, transparência e resultados de referência. Definir sucesso, início e fim, limite de tempo e regras de ajuda para cada tarefa.

| Fluxo | Resultado esperado |
| --- | --- |
| Abrir e entender um projeto | Abrir um PDN, localizar a camada solicitada e identificar a camada oculta. |
| Editar uma camada | Alterar a opacidade da camada correta, fazer um traço e desfazê-lo, mantendo a mudança de opacidade. |
| Navegar e editar um detalhe | Alcançar uma região com o zoom e a rolagem disponíveis, editar o detalhe e voltar à visão geral. |
| Exportar para PNG | Gerar um arquivo no destino indicado e conferir a composição das camadas visíveis e a transparência. |
| Retomar um projeto editável | Salvar, fechar e reabrir o projeto, conferindo as camadas e continuando a edição. Incluir na comparação quando as duas versões oferecerem o fluxo. |

- [ ] **Usabilidade.** Registrar conclusão sem ajuda, tempo e erros por tarefa. Comparar layouts com as mesmas funcionalidades e amostras equivalentes. Se cada participante testar as duas versões, alternar a ordem. Aplicar o questionário SUS de 10 itens uma vez após as tarefas de cada versão, antes da conversa sobre o teste. A meta do redesenho é **SUS médio acima de 80/100**. Interpretar a média com o tamanho da amostra, o intervalo de incerteza e os resultados por tarefa. Definir os participantes e o tamanho da amostra antes do estudo de aceite. [Questionário SUS original](https://germanupa.de/sites/default/files/2021-12/brooke1996sus.pdf) · [Versão validada em português brasileiro](https://aquichan.unisabana.edu.co/index.php/aquichan/article/view/16497)
- [ ] **Desempenho interativo.** No mesmo equipamento Linux, repetir medições com projetos típicos e grandes. Registrar tempo de abertura até a imagem ficar utilizável, latência do traço até a apresentação na tela, tempos de quadro no zoom e na rolagem, duração da exportação e pico de memória. Fixar condições comparáveis, como hardware, compositor, escala, dispositivo de entrada e versão do aplicativo. Usar a medição inicial para escolher metas de desempenho e localizar gargalos antes de escolher otimizações.
- [ ] **Redesenho.** Usar as dificuldades que mais prejudicam a conclusão das tarefas para decidir entre ajustes localizados e um redesenho mais amplo. Preservar a identidade visual própria e reaproveitar as operações de documento, edição e histórico nos protótipos. Definir em qual fase o redesenho entra depois de avaliar os resultados iniciais e repetir os testes nas versões comparáveis.

## Fase 2 — Seleção, área de transferência, visualização e geometria

- [x] **Seleção.** Retângulo, elipse, laço, varinha mágica, tolerância, amostragem local ou composta, seleção contígua ou global, substituir, adicionar, subtrair, intersectar e combinar por XOR. Mover e transformar a máscara separadamente dos pixels. Mover, redimensionar e girar pixels selecionados com alças, restrição de proporção e escolha de reamostragem, incluindo vizinho mais próximo com antialiasing.
- [x] **Feather de seleção.** Controle compartilhado por retângulo, elipse, laço e varinha mágica. A 0%, preservar a seleção SSAA original. A 100%, reduzir a cobertura linearmente até zero fora da região, em até metade da menor dimensão da caixa delimitadora. Aplicar ao novo gesto antes de combinar com uma seleção existente. Mostrar a cobertura parcial na tela e preservá-la ao preencher, apagar, copiar e mover a máscara.
- [x] **Editar.** Recortar, copiar, copiar a composição achatada, colar, colar em nova camada, colar em nova imagem, copiar a máscara da seleção e colar a máscara com os modos de combinação aplicáveis. Apagar a seleção, preenchê-la, invertê-la, selecionar tudo e limpar a seleção. Oferecer atalhos para esses comandos. Resolver o conflito entre o refazer atual em Ctrl+Shift+Z e Girar/Zoom do conjunto de atalhos de referência.
- [x] **Exibir e navegar.** Aumentar e reduzir zoom, ajustar à janela, ajustar à seleção, tamanho real, controle contínuo de zoom, ferramenta Zoom, mão para panorâmica e atalho temporário com a barra de espaço, grade de pixels e réguas. Alternar pixels, polegadas e centímetros com base na resolução do documento.
- [x] **Imagem.** Recortar à seleção, redimensionar com reamostragem e proporção opcional, alterar tamanho da tela com âncora, inverter horizontal e verticalmente, girar 90° horário, 90° anti-horário e 180°, e achatar a imagem. O comando Perfil de cor entra na fase 5.
- [x] **Geometria de camada.** Inverter horizontal e verticalmente, girar 180° e oferecer Girar/Zoom com prévia, deslocamento, escala e rotação. Validar interpolação, bordas, transparência e diferença entre operações de camada e de documento.

**Validação** — Em Linux/X11, o fluxo de interface selecionou uma região irregular com o laço, preencheu a seleção pelo teclado, copiou a composição e a abriu em outra aba. Um cliente X11 separado recebeu o PNG da área de transferência. Também foram verificados o movimento da máscara sem modificar os pixels, a prévia e a aplicação da transformação de pixels, o desfazer, as réguas e o zoom ajustado à seleção. Os testes automatizados cobrem os modos de combinação, a varinha, DPI, rotação, âncoras, proporção, transparência em reamostragem e recorte e a preservação do histórico por aba. A integração de área de transferência em Wayland nativo segue na validação de plataformas da fase 5.

**Aceite** — Selecionar uma região irregular, copiar a composição para outra imagem, mover a máscara sem alterar pixels, transformar os pixels em outra operação e desfazer cada passo. Verificar seleção, DPI, âncora e dimensões com testes de borda. [Menu Imagem do Paint.NET](https://paint.net/doc/latest/ImageMenu.html) · [Ferramentas do v5](https://paint.net/doc/latest/ToolsWindow.html) · [Código-fonte do v3](https://github.com/nakijun/OpenPDN-1/tree/cca476b0df2a2f70996e6b9486ec45327631568c/src)

## Fase 3 — Ferramentas e cores

- [x] **Traço.** Pincel, lápis (padrão de `1 px`) e borracha, com tamanho (`1..=96 px`). Onde aplicável, oferece dureza (`0–100%`), espaçamento (`1–200%`), padrão de preenchimento (sólido, listras, diagonais, xadrez, pontos, grade), antialiasing SSAA 4×4 contínuo e composição do traço (`Mesclar` / `Sobrescrever`). Usa botões primário e secundário para as duas cores e suporta amostragem rápida no pincel (`Ctrl+Clique` no Paint.NET / `Alt+Clique` no Adobe).
- [x] **Balde de preenchimento contíguo.** Preencher a região da camada ativa com a mesma cor RGBA do pixel clicado, conectada por bordas. Respeitar a cobertura SSAA da seleção e registrar a ação no histórico. Integrar ícone e atalhos aos perfis. A tolerância de cor e o modo global ficam na entrega abaixo.
- [x] **Preenchimento e amostragem.** Balde com alcance contíguo/global (`Shift+Clique` para modo global no Paint.NET), padrão de preenchimento e composição, amostragem de camada ou imagem composta, gradiente linear, refletido, losango, radial, cônico e espiral (horária/anti-horária) com modos de repetição, canal Cor/Transparência, dithering, SSAA 4×4 e alças arrastáveis; conta-gotas com amostragem de camada ou composição (`Ctrl`/`Alt`), permanência ou retorno à ferramenta anterior; carimbo de clonagem (`Ctrl+Clique` no Paint.NET / `Alt+Clique` no Adobe) com indicador de origem na tela e cópia congelada do início do traço; e recolorir com preservação de sombreamento.
- [x] **Tolerância de cor.** Compartilha o controle contextual de `0–100%` e o modo de alfa (`Alfa pré-multiplicado` / `Alfa direto`) entre varinha mágica, balde de tinta e recolorir. A 0%, exige igualdade segundo o modo de alfa em uso; a varinha e o balde comparam com a amostra do clique e o recolorir compara os pixels sob o traço com a cor-alvo amostrada (`Ctrl`/`Alt`) ou secundária, mantendo separação limpa dos lados de traços SSAA 4×4. [Barra de ferramentas do Paint.NET](https://paint.net/doc/latest/Toolbar.html) · [Recolorir no Paint.NET](https://paint.net/doc/latest/RecolorTool.html)
- [x] **Desenho geométrico.** Linha/curva (`CubicBezier` e `Spline` com 4 pontos de controle arrastáveis, espessura, tracejados e extremidades incluindo setas), além de formas com contorno, preenchimento ou ambos, padrão e mesclagem, cobrindo 26 formas agrupadas por família (básicas incl. retângulo arredondado com raio ajustável, polígonos e estrelas, setas, símbolos e balões) e a **Forma livre** do Paint.NET v3 em SSAA 4×4 com prévia, alças de redimensionamento/rotação e confirmação via `Enter`/clique ou cancelamento via `Esc`.
- [x] **Suavização das ferramentas existentes.** Pincel, borracha, seleções e preenchimento ou apagamento elíptico usam supersampling **SSAA (4×4)** ativo 100% do tempo e preservam o alfa parcial no histórico e na gravação PDN.
- [x] **Antialiasing SSAA contínuo.** O editor aplica SSAA 4×4 de forma permanente no contorno e no preenchimento (na prévia e nos pixels gravados na camada), sem modos **Nenhum** ou **SMAA**.
- [x] **Texto.** Posicionamento por clique e arraste da âncora na tela, edição direta por teclado (`Shift+Enter` quebra linha, `Enter` confirma, `Esc` cancela) ou campo na barra contextual, seletor de família (incl. fontes do sistema detectadas e embutidas), tamanho (`6..=240 px`), estilos negrito/itálico/sublinhado/tachado, alinhamento esquerda/centro/direita e rasterização em SSAA 4×4 gravada em uma única entrada de histórico.
- [x] **Cores.** Cores primária e secundária, alternar (`X`) e restaurar padrões (`D`), roda HSVA + barras de brilho e opacidade, modo compacto/expandido (`Mais`/`Menos` com controles numéricos RGB, HSV, Hex e Alfa sincronizados), 32 amostras editáveis (adicionar `+`, remover com botão direito), presets (`Padrão`, `Pastel`, `Tons de cinza`), importação/exportação `.txt` (`AARRGGBB`) compatível com Paint.NET e gravação automática em `paleta.txt`.

**Aceite** — Cada ferramenta respeita a camada ativa e a seleção segundo suas regras, mantém a prévia durante o gesto e gera uma edição reversível na confirmação. Testar amostragem com camadas ocultas e traços com pixels semitransparentes. [Ferramentas e configurações](https://paint.net/doc/latest/ToolsWindow.html) · [Histórico do Paint.NET 5.2](https://paint.net/roadmap.html)

### Atalhos de teclado e mouse configuráveis

- [x] **Registro de comandos.** Centralizar os atalhos existentes e associar cada ferramenta e comando a um identificador estável. Cada ferramenta nova deve incluir um ponto de integração para o atalho configurável e atualizar o esquema JSON com sua ação.
- [x] **Perfis.** Oferecer em **Configurações → Atalhos de teclado e mouse** a seleção, criação e edição de múltiplos perfis. Incluir **Paint.NET** como perfil padrão, com os atalhos correspondentes do Paint.NET, e **Adobe**, com os atalhos correspondentes do Adobe Photoshop. Os perfis adotam as combinações de referência para ações equivalentes já disponíveis e sinalizam conflitos de atribuição. [Paint.NET](https://paint.net/doc/latest/KeyboardMouseCommands.html) · [Adobe Photoshop](https://helpx.adobe.com/content/dam/help/en/photoshop/using/default-keyboard-shortcuts/photoshop-keyboard-shortcuts.pdf)
- [x] **Persistência local.** Gravar os perfis, seus mapeamentos e o perfil ativo em `atalhos.json` na mesma pasta do executável do aplicativo. Usar o [esquema JSON versionado](atalhos.schema.json) e preservar os mapeamentos personalizados quando novas ferramentas forem adicionadas. Informar falhas de leitura ou gravação no menu de configuração.

- [x] **Gestos de mouse.** Compartilhar o perfil ativo e o arquivo `atalhos.json` entre teclado e mouse. A aba **Mouse** permite remapear os gestos cadastrados, incluindo traço, seleção, roda de cores, navegação, transformações e comandos de camada. O esquema diferencia os identificadores de gestos dos identificadores de teclado.
- [x] **Espessura pela roda.** Usar `Ctrl+Shift+Wheel` nos perfis Paint.NET e Adobe para mudar a largura em passos de 1 px no canvas. Aplicar o gesto configurável a toda ferramenta de traço em pixels e atualizar a prévia de linhas, curvas e formas em edição.
- [x] **Paridade de gestos de referência.** Gestos equivalentes do Paint.NET e Photoshop implementados e configuráveis em `atalhos.json` e `atalhos.schema.json`: zoom por região (`zoom.marquee` no Paint.NET) e contínuo (`zoom.scrubby` no Adobe), realocação de seleção durante criação (`selection.relocate`: botão direito no Paint.NET, `Space+Left` no Adobe), conta-gotas, amostragem rápida no pincel, balde global, origem do carimbo, amostra do recolorir e manipulação com alças em gradiente/curva/forma/texto. [Paint.NET](https://paint.net/doc/latest/KeyboardMouseCommands.html) · [Adobe Photoshop](https://helpx.adobe.com/photoshop/desktop/get-started/learn-the-basics/zoom-images.html)

**Aceite dos atalhos** — Alternar entre Paint.NET e Adobe, criar um perfil personalizado, alterar um atalho, reiniciar o aplicativo e conferir a restauração do perfil ativo. Atualizar o esquema com uma ação nova e verificar que os atalhos personalizados continuam funcionando.

A troca para Adobe, a criação de um perfil personalizado e a restauração de seu atalho após reiniciar foram verificadas na interface Linux/X11.

## Fase 4 — Motor de plugins, ajustes e catálogo de efeitos

- [ ] **Infraestrutura comum e motor de plugins.** Implantar o formato portátil único **`.rpdn-plugin`** (mesmo artefato para Windows, macOS e Linux em `x86_64` e `aarch64`), o runtime híbrido em sandbox (**`wasmtime`** com `simd128` + execução opcional de compute shaders **`WGSL`** via `wgpu`/`naga` e *fallback* CPU determinístico), o contrato WIT `rustpdn:plugin@1.0.0`, o montador **`PluginUiHost`** em `egui` para a UI declarativa (`UiSchema`, respeitando automaticamente os temas Claro/Escuro) e a biblioteca mínima **`rustpdn-plugin-sdk`**. Operações recebem camada, seleção `TMask` (SSAA 4×4), parâmetros declarativos e semente, oferecendo prévia assíncrona por tiles com cancelamento, confirmação em uma única entrada de histórico, restauração de padrões, CLI de verificação (`plugin check/pack/test`) e repetição do último efeito (`Ctrl+F`).
- [ ] **4A — Ajustes.** Níveis automáticos, preto e branco, brilho/contraste, curvas, exposição, realces/sombras, matiz/saturação, inverter alfa, inverter cores, níveis, posterizar, sépia e temperatura/matiz (compartilhando o `PluginUiHost`, prévia assíncrona e histórico único).
- [ ] **4B — Primeiros plugins de efeitos.** Implementar os primeiros efeitos como pacotes `.rpdn-plugin` oficiais usando exclusivamente a API pública e `rustpdn-plugin-sdk`: desfoque gaussiano (híbrido Wasm + WGSL separável em 2 passes), desfoque quadrado, nitidez, pixelização, adicionar ruído (com `Seed` determinística) e detecção de bordas. Esses casos exercitam kernels, halo, bordas, seleção SSAA, UI declarativa, prévia assíncrona e paridade CPU/GPU.
- [ ] **4C — Catálogo completo das telas (via plugins).** Concluir todos os 48 efeitos do catálogo abaixo como plugins `.rpdn-plugin` carregados pelo mesmo registro usado por plugins externos. Os efeitos da fase 4B já estão contados neste catálogo.

| Categoria | Efeitos das telas do Paint.NET 5.2 |
| --- | --- |
| Artístico (7) | Esboço a tinta, linogravura, mosaico, pintura a óleo, esboço a lápis, pontilhismo, vitral. |
| Desfoques (10) | Bokeh, fragmentação, gaussiano, mediana, movimento, radial, esboço, quadrado, superfície, zoom. |
| Cor (2) | Dithering ordenado, quantização com dithering. |
| Distorção (11) | Abaulamento, cristalização, amassados, vidro fosco, morfologia, pixelização, inversão polar, esferização, reflexão em ladrilhos, torção, ondas. |
| Ruído (2) | Adicionar ruído, reduzir ruído. |
| Objeto (1) | Sombra projetada. |
| Foto (6) | Brilho difuso, remoção de olhos vermelhos, nitidez, suavização de retrato, endireitar, vinheta. |
| Renderização (4) | Nuvens, fractal de Julia, fractal de Mandelbrot, turbulência. |
| Estilização (5) | Detectar bordas, emboss, meio-tom, contorno, relevo. |

- [ ] **4D — Compatibilidade histórica.** Avaliar Unfocus como variante histórica de desfoque. Registrar o blur obsoleto como legado da implementação gaussiana e conferir a exposição de classes como Color Fill e Color Difference antes de atribuir comandos de menu. Girar/Zoom é entregue na fase 2. [Código dos efeitos v3](https://github.com/nakijun/OpenPDN-1/tree/cca476b0df2a2f70996e6b9486ec45327631568c/src/Effects)

**Aceite** — Cada ajuste e efeito possui parâmetros ou ação direta, respeita seleção `TMask` (SSAA 4×4) e alfa, permite desfazer em uma única entrada e tem amostras de referência em configurações representativas. Todo efeito do menu **Efeitos** deve rodar a partir do mesmo arquivo `.rpdn-plugin` em Windows, macOS e Linux, possuir caminho CPU Wasm de referência para execução sem GPU, manter paridade numérica dentro da tolerância documentada entre WGSL e Wasm e permitir compilar um plugin externo do zero seguindo o [Guia de extensibilidade](extensibilidade.md) e a biblioteca `rustpdn-plugin-sdk`. Para ruído e efeitos procedurais, testar com semente controlada. Comparar resultados com o v5.2 para definir tolerâncias por operação.

## Fase 5 — Cor, formatos, integração e extensibilidade

- [ ] **Cor e metadados.** Perfil de cor no menu Imagem com atribuição e conversão, sRGB, Adobe RGB, Display P3, ProPhoto RGB e ICC/ICM importados. Preservar perfis, resolução de impressão e metadados nos formatos que os suportam. Cobrir texto PNG, orientação EXIF e CICP. Avaliar conversão HDR para SDR nos codecs relevantes como recurso de importação.
- [ ] **Formatos comuns.** Completar opções de PNG e a matriz leitura/escrita para JPEG, BMP/DIB/RLE, GIF, TGA e TIFF. Testar paleta, transparência, qualidade, compressão e metadados conforme as capacidades de cada formato. Fixar política explícita para arquivos animados, incluindo GIF e APNG.
- [ ] **Formatos adicionais.** DDS, WebP, AVIF, HEIC/HEIF, JPEG XL e JPEG XR. Registrar direção suportada, variantes, dependências disponíveis no Linux e critérios de round-trip por codec. [Formatos do v5](https://paint.net/doc/latest/index.html)
- [ ] **Desktop e suporte.** Impressão, aquisição de scanner/câmera via integrações disponíveis no Linux, abertura por associação e arraste, configurações e padrões de ferramentas, idiomas e temas, ajuda, tutoriais, sobre, links de suporte e feedback. Tratar atualização pelo canal de distribuição Linux. Incluir gerenciador visual de plugins instalados, indicação clara de erros ao carregar ou executar extensões `.rpdn-plugin` e acesso ao catálogo de extensões.
- [ ] **Extensibilidade avançada (Codecs e Ajustes).** Expandir a API versionada `rustpdn:plugin@1.x` e `rustpdn-plugin-sdk` (entregue para Efeitos na Fase 4) para suportar **Codecs de arquivo (`FileTypes`)** e **Ajustes** de terceiros dentro do mesmo formato `.rpdn-plugin`. Avaliar formatos de pixels e operações do framework 5.2 (extração/substituição de canais, conversões, interpolação) e suportar `RGBA16`, `RGBA32Float`, CMYK e HDR na fronteira de importação conforme o codec exigir. Compatibilidade binária com plug-ins `.NET` legados permanece decisão separada de escopo.
- [ ] **Desempenho.** Ampliar as medições interativas iniciadas na Fase 1 para composição, área de transferência, efeitos e pilhas de camadas grandes. Priorizar atualização por regiões, cache e paralelismo a partir dos gargalos observados. Medir a necessidade de composição FP32, SIMD e backend GPU portátil com base em precisão e benchmarks. [Mudanças de renderização, cor e extensões na versão 5.2](https://paint.net/roadmap.html)

**Aceite** — Publicar a matriz por formato e a cobertura de metadados, testar a integração de arquivos e de área de transferência em Wayland e X11 e registrar benchmarks reproduzíveis de sessão interativa.

### Formatos exibidos no diálogo Abrir do Paint.NET 5.2

A captura mostra 13 famílias e 24 extensões. A opção **Todos os tipos de imagem** reúne as extensões da tabela. O diálogo documenta os filtros de abertura. A disponibilidade de gravação e o suporte a variantes de cada formato devem ser verificados separadamente.

| Família | Extensões exibidas | OpenPaintR hoje |
| --- | --- | --- |
| Paint.NET | `.pdn` | Importa e grava camadas |
| PNG | `.png` | Importa PNG estático e exporta a composição |
| JPEG | `.jpg`, `.jpeg`, `.jpe`, `.jfif`, `.exif` | Previsto na fase 5 |
| JPEG XL | `.jxl` | Previsto na fase 5 |
| AV1 (AVIF) | `.avif` | Previsto na fase 5 |
| HEIC | `.heic`, `.heif`, `.hif` | Previsto na fase 5 |
| WebP | `.webp` | Previsto na fase 5 |
| DirectDraw Surface (DDS) | `.dds` | Previsto na fase 5 |
| TIFF | `.tiff`, `.tif` | Previsto na fase 5 |
| GIF | `.gif` | Previsto na fase 5 |
| BMP | `.bmp`, `.dib`, `.rle` | Previsto na fase 5 |
| TGA | `.tga` | Previsto na fase 5 |
| JPEG XR | `.jxr`, `.wdp`, `.wmp` | Previsto na fase 5 |

### Photoshop PSD

- [ ] **Importação PSD na fase 5.** Começar por documentos RGB de 8 bits por canal. Preservar pixels, transparência, posição, ordem, nomes, visibilidade e opacidade das camadas raster. Mapear os modos de mesclagem que o OpenPaintR já representa. Validar a composição e as propriedades com amostras geradas no Photoshop. [Especificação Adobe do PSD](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/)
- [ ] **Compatibilidade adicional.** Dar aviso explícito para conteúdo que não puder ser preservado, como texto editável, objetos inteligentes, máscaras, grupos e camadas de ajuste. Quando a composição incorporada estiver disponível, oferecer abertura achatada como opção. Avaliar gravação PSD, arquivos PSB e outras profundidades de cor em etapas posteriores de compatibilidade.

## Evidência e decisões de escopo

- **Revisão do roadmap.** A síntese dos sete revisores registra as alternativas de layout, o plano de avaliação e as decisões que dependem da medição inicial. [Brief de revisão](../concepts_brief.md)
- **Telas 5.2.** As 16 capturas fornecidas cobrem a área de trabalho, Editar, Exibir, Imagem, Camadas, Ajustes, os nove submenus de Efeitos e o filtro de formatos em Arquivo → Abrir. Os demais comandos de Arquivo, Configurações, Ajuda e opções completas das ferramentas foram complementados pelo código histórico e pela documentação pública. Os submenus internos, parâmetros e formas que não aparecem abertos nas capturas entram na conferência funcional de suas fases.
- **Versão 3.** O OpenPDN-1 3.36.7 fornece os menus, ações, ferramentas, codecs e efeitos históricos. Exemplos de rastreabilidade incluem `CopyToClipboardAction`, `PasteInToNewLayerAction`, `CanvasSizeAction`, `CloneStampTool`, `RotateZoomEffect` e `PdnFileType`. [Revisão consultada](https://github.com/nakijun/OpenPDN-1/tree/cca476b0df2a2f70996e6b9486ec45327631568c)
- **Investigação v5.2.** Os artefatos locais em `target/windows-tools/paintnet-rea-evidence` registram o build 5.200.9810.35277. `PaintDotNet.Data.dll.members.json` confirma `Document`, `BitmapLayer` e `LayerList`. `PaintDotNet.Core.dll.members.json` inclui tipos de modos de mesclagem. `PaintDotNet.FileTypes.dll.members.json` registra os contratos de codecs. `PaintDotNet.Effects.Gpu.dll.members.json` confirma famílias de efeitos, incluindo Bokeh, Linocut, Drop Shadow, Morphology e Waves. Tipos que atendem ao mesmo efeito contam uma vez no catálogo. Os resultados são de análise estática REA, com cobertura parcial em parte dos assemblies. O log Ghidra disponível registra a importação do executável OpenPaintR. As telas e os testes com arquivos próprios sustentam as afirmações sobre comportamento do Paint.NET. [Inventário técnico anterior](referencias.md)
- **Validação de PDN.** As amostras em `tests/fixtures/pdn` (`Untitled`, `Untitled3`, `oldPDN3510` e `FlattenBlendTest`) integram a suíte `tests/pdn_compat.rs`, verificando composição exata contra `oldPDN3510-reference.png` e round-trip das 14 camadas de mesclagem. Ampliar o conjunto na fase 1 conforme novas versões forem testadas.
- **Limite da proposta.** A meta cobre os recursos de edição identificados nas fontes citadas. Grupos de camadas, camadas de texto, máscaras e camadas de ajuste pertencem ao roadmap posterior do Paint.NET e podem ser avaliados após a paridade proposta com o 5.2. [Roadmap oficial](https://paint.net/roadmap.html)
