# OpenPaintR

Editor de imagens raster em Rust para Linux, com interface própria e fluxos de edição inspirados no Paint.NET. A interface nativa usa o mesmo núcleo de documentos e camadas da linha de comando.

## Interface gráfica

Use **Claro** ou **Escuro** na barra de menu superior (estilo Windows) para alternar entre o tema pastel claro e o tema escuro. Abaixo da barra de comando contextual fica a **Color Ribbon** horizontal (preview da cor ativa à esquerda, espectro completo de matizes com indicador e botão chevron à direita para abrir o painel flutuante **Cores** com roda HSVA, barras de brilho/opacidade, modo expandido RGB/HSV/Hex/Alfa e paletas); a coluna esquerda reúne a faixa vertical de ferramentas; o centro exibe as abas de documentos com o botão `+` sobre a tela; a coluna direita exibe os painéis **Camadas** e **Histórico**; e a barra inferior concentra dimensões, coordenadas e controle contínuo de zoom.

Em uma sessão Linux com Wayland ou X11 e OpenGL, execute:

```sh
cargo run --locked
```

Use **Novo** para definir as dimensões da imagem ou **Abrir** (ou **Arquivo → Abrir recentes**) para abrir um documento Paint.NET (`.pdn`) ou importar um PNG em uma nova aba. As abas exibem miniaturas e permitem reordenar por arraste e alternar com `Ctrl+Tab` / `Ctrl+Shift+Tab`. O painel flutuante **Cores** (aberto pelo chevron da Color Ribbon) oferece roda HSVA, barras de brilho e opacidade, modo expandido (`Mais`/`Menos`) com RGB/HSV/Hex/Alfa numéricos, 32 amostras editáveis, presets e importação/exportação `.txt` (`AARRGGBB`, com gravação em `paleta.txt`). A paleta de ferramentas reúne **Pincel**, **Lápis**, **Borracha**, **Conta-gotas**, **Carimbo de clonagem**, **Recolorir**, **Balde de tinta**, **Gradiente**, **Linha / Curva**, **Formas** (26 formas + **Forma livre**), **Texto**, **Retângulo**, **Elipse**, **Laço**, **Varinha mágica**, **Mão**, **Zoom** e **Mover/transformar pixels**, com controles contextuais na barra superior. O painel **Camadas** permite adicionar, excluir, duplicar, mesclar com a camada abaixo, reordenar por arraste, importar de arquivo, alterar visibilidade, ajustar opacidade e escolher o modo de mesclagem ou propriedades completas da camada.

O painel **Histórico** registra traços, seleções e alterações de camadas, permitindo clicar em qualquer estado para voltar ou avançar. Use **Ctrl+Z** para desfazer e **Ctrl+Y** (no perfil Paint.NET) ou **Ctrl+Shift+Z** (no perfil Adobe) para refazer. **Salvar** (`Ctrl+S`), **Salvar como** (`Ctrl+Shift+S`) e **Exportar PNG** oferecem prévia do resultado e opções de compressão/profundidade, gravando arquivos Paint.NET `.pdn` de forma atômica com pixels RGBA, ordem, nomes, visibilidade, opacidade e modos de mesclagem das camadas. O asterisco ao lado do nome indica alterações desde a última gravação. Ao fechar abas ou sair do aplicativo, cada documento com alterações pede confirmação antes de descartar.

Use **Retângulo**, **Elipse**, **Laço** ou **Varinha** para selecionar pixels. Durante seleções retangulares/elípticas, segure o botão direito (Paint.NET) ou `Espaço+Clique` (Adobe) para deslocar o contorno em tempo real. A varinha, o balde de tinta e a ferramenta de recolorir compartilham o controle de **Tolerância de cor (0–100%)** e o modo de alfa (`Alfa pré-multiplicado` ou `Alfa direto`). As ferramentas de seleção oferecem os modos Substituir, Adicionar, Subtrair, Intersectar e XOR, **Feather (0–100%)** e modificadores de mouse (`Shift` para quadrado/círculo, `Ctrl`/`Alt` para combinar). Pincel e borracha oferecem **Dureza**, **Espaçamento**, **Padrão de preenchimento** e modo **Mesclar/Sobrescrever**, respeitando a seleção ativa. O menu **Seleção** reúne selecionar tudo (`Ctrl+A`), limpar (`Ctrl+D` ou `Enter`), inverter (`Ctrl+I`), preencher com a cor atual (`Backspace`) e apagar (`Delete`).

Com uma seleção ativa, clique fora dela na área de edição ou pressione **Esc** para limpar. Se estiver transformando pixels/máscara ou editando um **Gradiente**, **Linha / Curva**, **Forma** ou **Texto**, arraste as alças na tela (ou mova com as setas), pressione **Enter** para confirmar ou **Esc** para cancelar e reverter sem alterar a camada. O **Balde de tinta** preenche de forma contígua ou global (`Shift+Clique` no Paint.NET), respeita a cobertura SSAA da seleção, amostra a camada ativa ou a composição e recompõe transições SSAA de bordas. No **Carimbo de clonagem**, use `Ctrl+Clique` (Paint.NET) ou `Alt+Clique` (Adobe) para definir a origem. **Gradiente** suporta 7 formas (Linear, Refletido, Losango, Radial, Cônico e Espirais), repetição, canal Cor/Transparência e dithering. **Texto** permite digitar diretamente na tela, escolher fonte, tamanho, estilos e alinhamento, e confirmar em uma única entrada de histórico. No perfil **Paint.NET**, `Z` arrasta caixa de ampliação (`zoom.marquee`); no **Adobe**, arrasta zoom contínuo (`zoom.scrubby`).

Em **Editar**, copie a camada ou a composição e cole na camada ativa (com alças de posicionamento e transformação antes de confirmar), em uma nova camada ou em uma nova imagem. Colar em nova imagem abre uma aba e preserva o documento de origem. As máscaras podem ser copiadas e combinadas dentro do aplicativo. Para mover ou transformar, escolha **Mover/transformar máscara** ou **Mover/transformar pixels** e use as alças na tela (ou arraste com `Ctrl`/`Alt` para copiar ou escalar pelo centro e `Shift` para proporção ou passos de 15°).

**Exibir** reúne zoom (inclusive `Ctrl+roda` no Paint.NET ou `Alt+roda` no Adobe), grade e réguas. Use **Mão**, botão do meio ou mantenha Espaço pressionado enquanto arrasta para navegar. Duplo clique no ícone da Mão ajusta à janela e no ícone do Zoom volta ao tamanho real (no perfil Adobe). Réguas e coordenadas podem usar pixels, polegadas ou centímetros. Documentos sem resolução física válida usam 96 DPI.

**Imagem** reúne recorte à seleção, redimensionamento, tamanho da tela com âncora (também acessível diretamente na tela via `Ctrl+Duplo-clique` no canto inferior direito do canvas, arrastando o novo limite e confirmando com `Enter` ou `Ctrl` / cancelando com `Esc`), inversão, rotação e achatamento. **Camada** oferece operações completas na camada ativa, incluindo **Girar/Zoom** com prévia. No perfil Paint.NET, `Ctrl+Shift+Z` abre Girar/Zoom e `Ctrl+Y` refaz. No perfil Adobe, `Ctrl+Shift+Z` refaz e `Ctrl+T` abre a transformação livre.

O editor rasteriza traços, seleções e bordas preenchidas sempre com **SSAA 4×4** ativo, preservando a suavização tanto na prévia quanto nos pixels gravados na camada.

Em **Configurações → Atalhos de teclado e mouse**, selecione **Paint.NET** (padrão), **Adobe** ou duplique um perfil para personalizar atalhos de **Teclado** e gestos de **Mouse**. Digite combinações como `Ctrl+Shift+B` no teclado ou `Ctrl+Wheel` no mouse, separe alternativas com `;` e deixe o campo vazio para remover o atalho. Use **Salvar perfis** para aplicar as mudanças. Os perfis e o perfil ativo ficam em `atalhos.json`, ao lado do executável. Em instalações sem permissão de escrita nessa pasta, o menu informa a falha de gravação. [Esquema JSON](docs/atalhos.schema.json)

Sobre o canvas, **Ctrl+Shift+roda para cima** aumenta a espessura e **Ctrl+Shift+roda para baixo** diminui. Cada passo da roda altera 1 px. O gesto funciona com Pincel, Lápis, Borracha, Carimbo de clonagem, Recolorir, Linha / Curva e Formas. Linhas, curvas e formas em edição mostram a mudança na prévia. O lápis começa com 1 px e tem tamanho próprio.

**Exportar PNG** grava a composição visível em um arquivo novo, com escolha de profundidade de cor e compressão e prévia estimada. Os diálogos de arquivos usam o portal do ambiente de desktop no Linux.

O OpenPaintR usa formatos interoperáveis. Documentos com camadas são gravados em PDN para o Paint.NET. Quando o suporte a PSD for implementado, a referência de compatibilidade será o Adobe Photoshop. A exportação PNG guarda a composição.

## Núcleo e linha de comando

O núcleo oferece uma tela RGBA de 8 bits por canal, camadas de dimensões iguais às da tela, edição de pixels e composição de baixo para cima nos 14 modos de mesclagem do Paint.NET acelerada por kernels SIMD (`jflib-gfx`). Cada camada tem nome, visibilidade, opacidade e modo de mesclagem. A demonstração gera uma imagem PAM de 320 × 240 pixels com duas formas sobrepostas sobre fundo transparente.

Execute com Rust 1.98 ou posterior e Cargo. A leitura e a escrita de PNG usam o crate `png`. Após baixar as dependências com `cargo fetch --locked`, os comandos podem ser executados offline.

```sh
cargo test --locked --offline
cargo run --locked --offline --no-default-features -- demo openpaintr-demo.pam
cargo run --locked --offline --no-default-features -- demo-png openpaintr-demo.png
cargo run --locked --offline --no-default-features -- png entrada.png saida.png
cargo run --locked --offline --no-default-features -- pdn entrada.pdn saida.png
```

Use um caminho de saída novo para cada execução. A CLI preserva arquivos existentes.

A importação de PNG estático cria uma camada raster e converte a imagem para RGBA de 8 bits por canal. A exportação grava a composição das camadas visíveis com transparência. PNG não armazena as camadas do documento. PNG animado (APNG) ainda não é suportado.

A leitura e a gravação de PDN usam o crate Rust `jflib-img-pdn`. A interface preserva a ordem, os nomes, a visibilidade, a opacidade e os modos de mesclagem. Um PNG exportado guarda a imagem composta. O fluxo PDN preserva o perfil ICC, a resolução e os metadados de documento e camada disponibilizados pelo codec. A exportação PNG grava apenas a composição RGBA.

## Referências e marcos

O [estudo inicial](docs/referencias.md) documenta as classes examinadas do OpenPDN-1, o inventário do Paint.NET instalado e os experimentos de compatibilidade a executar. O sistema de extensões multiplataforma (`.rpdn-plugin` em **WebAssembly + WGSL opcional + UI declarativa**, obrigatório para todos os itens do menu **Efeitos**) é especificado em [Arquitetura de plugins](docs/arquitetura-plugins.md) e no [Guia de extensibilidade](docs/extensibilidade.md).

1. Núcleo e CLI com edição raster e exportação de PNG.
2. Primeira interface com tela, pincel, camadas, histórico e fluxos de PNG.
3. Medir o comportamento de composição do Paint.NET e ampliar a cobertura de modos de mesclagem com casos de teste verificáveis.
4. Ampliar a cobertura da importação `.pdn` com amostras produzidas por diferentes versões do Paint.NET antes de definir o escopo de escrita.
