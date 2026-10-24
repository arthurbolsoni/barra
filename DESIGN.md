---
name: barra
description: Barra de tarefas do Windows no estilo Waybar/Omarchy, com grupos de janelas e visão geral com miniaturas ao vivo.
colors:
  fundo: "#1a1b26"
  fundo-claro: "#24283b"
  selecao: "#292e42"
  mudo: "#414868"
  texto: "#a9b1d6"
  texto-apagado: "#7e84a1"
  texto-escuro: "#565f89"
  texto-forte: "#c0caf5"
  destaque: "#7aa2f7"
  vermelho: "#f7768e"
  amarelo: "#e0af68"
  verde: "#9ece6a"
typography:
  headline:
    fontFamily: "CaskaydiaMono Nerd Font"
    fontSize: "15px"
    fontWeight: 700
  title:
    fontFamily: "CaskaydiaMono Nerd Font"
    fontSize: "12px"
    fontWeight: 700
  body:
    fontFamily: "CaskaydiaMono Nerd Font"
    fontSize: "12px"
    fontWeight: 400
  label-cartao:
    fontFamily: "CaskaydiaMono Nerd Font"
    fontSize: "13px"
    fontWeight: 400
  glifo-sistema:
    fontFamily: "CaskaydiaMono Nerd Font"
    fontSize: "14px"
    fontWeight: 400
  glifo-janela:
    fontFamily: "CaskaydiaMono Nerd Font"
    fontSize: "16px"
    fontWeight: 400
rounded:
  none: "0px"
spacing:
  borda-barra: "6px"
  folga-grupo: "7px"
  separacao: "14px"
  celula-glifo: "24px"
  celula-janela: "28px"
  icone: "16px"
  visao-topo: "70px"
  visao-margem: "20px"
  vao: "10px"
  cabecalho-cartao: "32px"
  pe-cartao: "30px"
  rotulo-lista: "26px"
components:
  barra:
    backgroundColor: "{colors.fundo}"
    textColor: "{colors.texto}"
    typography: "{typography.body}"
    height: "26px"
    rounded: "{rounded.none}"
  grupo:
    textColor: "{colors.texto}"
    typography: "{typography.body}"
    padding: "0 7px"
    height: "26px"
  grupo-ativo:
    textColor: "{colors.destaque}"
    typography: "{typography.title}"
  grupo-vazio:
    textColor: "{colors.texto-apagado}"
  grupo-hover:
    backgroundColor: "{colors.fundo-claro}"
  botao-janela:
    width: "{spacing.celula-janela}"
    height: "26px"
    size: "{spacing.icone}"
  botao-janela-hover:
    backgroundColor: "{colors.fundo-claro}"
  botao-titulo:
    backgroundColor: "{colors.fundo}"
    textColor: "{colors.texto}"
    typography: "{typography.glifo-sistema}"
    width: "{spacing.celula-janela}"
  botao-titulo-hover:
    backgroundColor: "{colors.fundo-claro}"
    textColor: "{colors.texto-forte}"
  botao-titulo-fechar-hover:
    backgroundColor: "{colors.fundo-claro}"
    textColor: "{colors.vermelho}"
  item-bandeja:
    textColor: "{colors.texto}"
    typography: "{typography.glifo-sistema}"
    width: "{spacing.celula-glifo}"
    size: "{spacing.icone}"
  dica:
    backgroundColor: "{colors.fundo}"
    textColor: "{colors.texto}"
    typography: "{typography.body}"
    height: "26px"
    padding: "0 11px"
    rounded: "{rounded.none}"
  titulo-visao:
    textColor: "{colors.texto-forte}"
    typography: "{typography.headline}"
  cartao-grupo:
    backgroundColor: "{colors.fundo-claro}"
    textColor: "{colors.texto}"
    typography: "{typography.title}"
    padding: "0 12px"
    rounded: "{rounded.none}"
  cartao-grupo-ativo:
    textColor: "{colors.destaque}"
  cartao-criar-grupo:
    textColor: "{colors.texto}"
    typography: "{typography.label-cartao}"
    rounded: "{rounded.none}"
  faixa-minimizada:
    backgroundColor: "{colors.selecao}"
    textColor: "{colors.texto}"
    typography: "{typography.body}"
    height: "22px"
    rounded: "{rounded.none}"
  rotulo-alt-tab:
    textColor: "{colors.texto}"
    typography: "{typography.body}"
    height: "{spacing.rotulo-lista}"
  rotulo-alt-tab-selecionado:
    textColor: "{colors.texto-forte}"
---

# Design System: barra

## Overview

**Creative North Star: "A Waybar transplantada"**

A barra é a Waybar do Omarchy (Hyprland) levada para o Windows 11: uma faixa de 26 px no alto do monitor, informação como texto e glifos da CaskaydiaMono Nerd Font, cores tiradas do tema Omarchy escolhido pelo usuário. Não há botão em forma de pílula, cantos arredondados, sombra, título de janela na barra nem botão Iniciar. A barra é quieta; a visão geral (Win+Tab e Alt+Tab) concentra a informação.

A densidade é de ferramenta de desenvolvedor: tudo em 12 px, peso como único eixo de hierarquia dentro da barra, estado marcado por cor do tema e por linhas de 2 px. A visão geral reproduz os quadros do Hyprland: cada grupo é um quadro de borda reta de 2 px com as miniaturas ao vivo do DWM no lugar onde as janelas ficam na tela, separado dos vizinhos por vãos de 10 px.

O mesmo vocabulário sai da barra e chega às janelas de outros programas: barra de título na cor de fundo do tema, com o título na cor de texto e os botões de minimizar, maximizar e fechar em glifos do tema, sem ícone; borda de destaque na janela em foco e apagada nas outras; cantos retos. A cor de destaque e o modo claro ou escuro do Windows também passam a ser os do tema.

**Key Characteristics:**
- Faixa de 26 px no topo, como AppBar; todas as medidas multiplicadas pela escala de DPI.
- Uma única família tipográfica (CaskaydiaMono Nerd Font) para texto e ícones de sistema.
- Cores exclusivamente por papel de tema; 23 temas do Omarchy selecionáveis, padrão tokyo-night.
- Zero arredondamento; bordas de 2 px no lugar de sombra.
- Sem animação: os estados trocam na hora.
- Textos em português formal (pt-BR).

## Colors

A paleta é um conjunto de papéis, não de valores: os hex abaixo são os do tema padrão tokyo-night, e cada um dos 23 temas de `src/tema.rs` (tokyo-night, catppuccin, catppuccin-latte, ethereal, everforest, flexoki-light, gruvbox, hackerman, kanagawa, last-horizon, lumon, lupine, matte-black, miasma, nord, osaka-jade, retro-82, ristretto, rose-pine, solitude, starship, vantablack, white) preenche os mesmos papéis. Cinco temas são claros (`claro: true`) e mudam a opacidade de ícones minimizados e o modo escuro das molduras.

### Primary
- **Destaque** (`destaque`): marca o estado atual. Nome do grupo ativo, linha sob a janela em foco, borda do cartão do grupo ativo, borda da janela selecionada na visão geral, borda da dica, texto do filtro digitado e borda DWM da janela em primeiro plano.

### Secondary
- **Amarelo** (`amarelo`): atenção. Janela piscando (fundo da célula em 28% de opacidade e linha inferior de 2 px), cartão de destino durante o arrasto (borda e texto do "Criar grupo") e progresso pausado.
- **Verde** (`verde`): progresso normal do ITaskbarList3.
- **Vermelho** (`vermelho`): progresso em erro, volume mudo e glifo do botão de fechar sob o mouse.

### Neutral
- **Fundo** (`fundo`): fundo da barra, da dica e da visão geral; barra de título das janelas tematizadas.
- **Fundo claro** (`fundo-claro`): realce de hover na barra, área de miniaturas do cartão de grupo, fundo do quadro de janela minimizada no Alt+Tab.
- **Seleção** (`selecao`): trilho da linha de progresso e faixa das janelas minimizadas no pé do cartão.
- **Mudo** (`mudo`): borda dos cartões e quadros em repouso; borda DWM das janelas sem foco.
- **Texto** (`texto`): texto e glifos padrão; borda do quadro de janela em hover; texto das barras de título tematizadas; cor do glifo `>_` dos terminais.
- **Texto apagado** (`texto-apagado`): valor derivado, `texto` levado 30% na direção de `fundo`. Grupos vazios, glifo de "+" na barra, nome do grupo no rótulo do Alt+Tab.
- **Texto escuro** (`texto-escuro`): borda de cartão em hover na visão geral.
- **Texto forte** (`texto-forte`): título da visão geral, borda do cartão escolhido pelo teclado, título da janela selecionada no Alt+Tab.

### Named Rules
**A Regra do Papel de Tema.** Toda cor desenhada vem de um papel do `Tema` ativo ou de uma mistura entre dois papéis (`mistura`). Hex literal no desenho não entra.

**A Regra do Apagado Legível.** O estado "apagado" usa `texto` misturado 30% com `fundo`, não `texto_escuro`, que fica abaixo de 3:1 sobre o fundo em vários temas. Glifo de janela minimizada usa a mistura de 50%.

**A Regra dos Três Estados.** Amarelo é atenção, verde é progresso, vermelho é erro. Nenhuma das três aparece como decoração.

## Typography

**Display Font:** CaskaydiaMono Nerd Font
**Body Font:** CaskaydiaMono Nerd Font
**Label/Mono Font:** CaskaydiaMono Nerd Font (texto e glifos de ícone)

**Character:** Monoespaçada de terminal, a mesma da Waybar do Omarchy. Hierarquia por peso, quase nunca por tamanho.

DirectWrite com localidade `pt-br`, sem quebra de linha, corte por caractere com reticências ("…"), alinhamento vertical centralizado no retângulo. Texto e glifos usam antisserrilhado em escala de cinza, como no Omarchy (o ClearType deixa franja colorida); nos menus desenhados pelo GDI, a fonte é criada com `ANTIALIASED_QUALITY` e o nome de família "CaskaydiaMono NF".

### Hierarchy
- **Headline** (negrito, 15 px): título da visão geral, a janela escolhida. Único texto acima de 12 px fora do cartão "Criar grupo".
- **Title** (negrito, 12 px): nome do grupo no cabeçalho do cartão.
- **Body** (normal, 12 px): grupos, relógio, dica, linha de atalhos da visão geral, contagem de janelas, títulos nas faixas e rótulos.
- **Label cartão** (normal, 13 px): "+  Criar grupo" no cartão de novo grupo.
- **Glifo sistema** (normal, 14 px): rede, volume e glifos da bandeja.
- **Glifo janela** (normal, 16 px): glifo que substitui o ícone do programa na barra, do mesmo tamanho do ícone.

### Named Rules
**A Regra da Fonte Única.** Texto, número e ícone de sistema saem da CaskaydiaMono Nerd Font. O negrito marca "ativo" nos cartões da visão geral e nos itens marcados dos menus; na barra não há negrito. O tamanho só sobe no título da visão geral.

**A Regra do Glifo Monocromático.** Glifos da Nerd Font (`f067` mais, `f0200` rede, `f057e` volume, `f120` terminal, `f11c` teclado, `f0e4` Gerenciador de Tarefas) são desenhados numa cor de papel do tema, nunca coloridos. Eles substituem o ícone colorido apenas do Gerenciador de Tarefas, dos terminais (Windows Terminal, conhost, OpenConsole, wt, pwsh, powershell, cmd) e do AutoHotkey; os outros programas mantêm o próprio ícone.

## Layout

Todas as medidas são em px a 96 dpi, multiplicadas pela escala do monitor.

**Barra.** Faixa de 26 px no topo, registrada como AppBar. Esquerda, a partir de 6 px da borda: grupos lado a lado, cada um com a largura do nome (mínimo 7 px) mais 14 px; o "+" numa célula de 24 px; 14 px de separação; as janelas do grupo ativo em células de 28 px com ícone de 16 px centralizado. Direita, a 6 px da borda: volume, rede e o botão do navegador (glifo `f059f`, abre uma janela nova do navegador padrão) em células de 28 px; 6 px antes delas, os ícones da bandeja em células de 24 px, da direita para a esquerda. Centro: dia e hora, com 8 px de folga em cada lado; o relógio some quando as janelas ou a bandeja chegariam até ele.

**Dica.** Abaixo da barra, 6 px de distância, centralizada no alvo e presa a 4 px das bordas do monitor. Altura 26 px, largura do texto mais 22 px, limite de 640 px de texto.

**Visão geral (Win+Tab).** Ocupa o monitor. Título entre 12 e 38 px do alto, linha de atalhos entre 38 e 58 px, ambos com 40 px de margem lateral. A grade começa em 70 px do alto, com 20 px de margem e 10 px de vão. Há um cartão por grupo mais o "Criar grupo", todos do mesmo tamanho; o número de colunas é o que deixa as miniaturas maiores, e a grade fica centralizada. Cada cartão tem cabeçalho de 32 px, área de miniaturas recuada 10 px dos lados e pé de 30 px para as janelas minimizadas (faixas de 22 px de altura, até 220 px de largura, 4 px entre elas). As miniaturas guardam a posição relativa das janelas no monitor.

**Lista (Alt+Tab).** Mesma moldura (topo 70, margem 20, vão 10). Uma célula por janela na ordem de uso, sem sobreposição, com rótulo de 26 px embaixo. A proporção da célula é a mediana das janelas (limitada entre 0,35 e 1,6) e a célula não passa de 80% da altura útil; em empate vence o arranjo com mais colunas. A última linha, incompleta, fica centralizada.

**Filtro.** Janelas que não casam com o filtro ficam com miniatura em 45/255 de opacidade e o desenho delas em 30%.

## Elevation & Depth

Plano. Não há sombra em nenhuma superfície. Profundidade vem de camada tonal (`fundo` sob `fundo-claro` ou `selecao`) e de borda; a dica e a visão geral são janelas de topo, separadas do resto pela borda de 2 px ou pelo fundo inteiro.

### Named Rules
**A Regra da Borda no Lugar da Sombra.** Separação e foco são bordas retas de 2 px (1 px em quadro de janela em repouso). Sombra, brilho e desfoque não entram.

## Shapes

Retângulos de canto vivo (0 px) em tudo: células da barra, dica, cartões, quadros de janela, faixas de minimizadas e, via `DWMWCP_DONOTROUND`, as janelas dos outros programas. Bordas de cartão e dica ficam por dentro do retângulo; bordas de quadro de janela ficam por fora, como no Hyprland, porque a miniatura do DWM cobre o lado de dentro. Marcas de estado na barra são linhas de 2 px recuadas 4 px das laterais da célula: embaixo para foco e atenção, em cima para progresso.

## Components

### Navigation (grupos na barra)
Nomes de grupo como os workspaces da Waybar: só texto, sem fundo.
- **Padrão:** `texto`, normal.
- **Ativo:** glifo `f14fb` (󱓻) no lugar do número, em `texto-forte`, como o workspace ativo da Waybar do Omarchy; grupo renomeado mostra o nome. Não recebe realce de hover.
- **Vazio:** `texto-apagado`.
- **Hover:** fundo `fundo-claro` na célula inteira.
- **"+":** glifo em `texto-apagado` numa célula de 24 px; mesmo hover.
- **Dica:** "Grupo {nome}: nenhuma janela / 1 janela / {n} janelas".

### Botão de janela
Só ícone, sem título.
- **Ícone:** 16 px, reduzido com filtro bicúbico e alinhado ao pixel; ou o glifo da Nerd Font em `texto`.
- **Foco:** fundo `selecao` na célula inteira; glifo em `texto-forte`.
- **Piscando:** fundo `amarelo` a 28% e linha `amarelo` de 2 px no pé.
- **Hover:** fundo `fundo-claro`.
- **Minimizada:** ícone em 40% de opacidade (55% em tema claro); glifo na mistura de 50% entre `texto` e `fundo`.
- **Selo:** ícone de 9 px no canto inferior direito do ícone.
- **Progresso:** linha de 2 px no alto da célula sobre trilho `selecao`; `verde` normal, `amarelo` pausado, `vermelho` erro; indeterminado ocupa a largura toda a 50%.
- **Dica:** título da janela.

### Bandeja, sistema e relógio
- **Bandeja:** ícones de 16 px (ou glifo de 14 px em `texto`) em células de 24 px; hover `fundo-claro`.
- **Rede:** glifo de 14 px em `texto` numa célula de 28 px: cabo (`f0200`), Wi-Fi (`f05a9`) ou VPN (`f0582`), conforme a placa por onde sai a internet; sem conexão, `f0319` em `texto-apagado`. Dica "{placa}: {IPv4}".
- **Volume:** glifo de 14 px numa célula de 28 px, pelo nível: `f057f` até 33%, `f0580` até 66%, `f057e` acima; mudo `f0581` em `vermelho`. Dica "Volume: {n}%" ou "Volume: mudo".
- **Painel aberto:** o ícone fica com fundo `fundo-claro` e glifo em `texto-forte`.
- **Relógio:** "{Dia da semana} HH:MM" ("Quarta-feira 23:22"), dia com inicial maiúscula e sem vírgula, como o "Friday 12:13" do Omarchy; `texto`, 12 px. Dica com a data por extenso e a semana ISO ("Terça-feira, 21 de outubro de 2026 · semana 43"). Clique abre a central; com ela aberta, fundo `fundo-claro` e texto `texto-forte`.
- **Aviso ao lado do relógio:** "{compromisso} em {n} min" nos 30 minutos antes do próximo compromisso, em `texto` misturado 30% ao `fundo` (`amarelo` nos últimos 5 minutos), 10 px depois da hora; quadrado de 6 px em `vermelho` quando há execução de hoje com falha ou sessão do Claude aguardando permissão. Os dois entram na área de clique do relógio, que continua centrado.

### Dica (tooltip)
- **Corner Style:** reto (0 px).
- **Background:** `fundo`.
- **Border:** 2 px `destaque`, por dentro.
- **Internal Padding:** 8 px de área de texto em cada lado; texto centralizado, 12 px, `texto`.

### Painel de rede e de volume
Abre colado embaixo da barra, alinhado ao ícone (12 px antes dele) e preso à borda direita do monitor. Não pega o foco; fecha com clique fora, Esc ou novo clique no ícone.
- **Corner Style:** reto (0 px). **Shadow:** nenhuma.
- **Tamanho:** 300 px de largura; altura pela soma das linhas.
- **Background:** `fundo`. **Border:** 1 px `mudo` nos lados e embaixo; em cima fica a barra.
- **Título (32 px):** nome em negrito `texto-forte` a 12 px da esquerda; à direita, o nível em `texto` misturado 30% ao `fundo`.
- **Nível (32 px):** glifo de volume de 15 px (clique liga o mudo; `vermelho` no mudo), trilho de 6 px em `mudo` com a parte cheia em `destaque` (`texto-forte` no hover, `texto-escuro` no mudo) e número em `texto-forte` à direita.
- **Rótulo de seção (26 px):** linha de 1 px `selecao` em cima e texto em minúsculas, `texto` misturado 45% ao `fundo`. O do Bluetooth tem 30 px, texto em `texto` e interruptor de 26×12 px à direita: ligado, caixa `destaque` com quadrado `fundo` à direita; desligado, caixa `mudo` com quadrado apagado à esquerda.
- **Item (26 px):** glifo de 14 px numa coluna de 26 px, texto a partir de 40 px, detalhe à direita (IP, "conectado" em `destaque`, "conectando…" em `amarelo`, "desconectada" apagado) e marca `f00c` em `destaque` na saída padrão. Item escolhido: texto `texto-forte`, glifo `destaque`. Hover `fundo-claro`.
- **Programa (22 px):** ícone da janela do programa (14 px) ou `f08c6`, nome apagado até 112 px e trilho de 4 px até a borda.
- **Atalhos:** "Configurações de …" no pé, com linha `selecao` em cima, glifo `f0493` em `texto-escuro` e texto apagado.

### Central
Abre colada embaixo da barra, centrada no relógio: até 1560 px de largura, limitada ao monitor, e altura da barra até a base do monitor. Pega o foco (teclado para filtro e navegação) e fecha quando perde o foco, com Esc ou novo clique no relógio.
- **Corner Style:** reto (0 px). **Shadow:** nenhuma. **Background:** `fundo`. **Border:** 2 px `destaque` nos lados e embaixo, como os quadros da visão geral; em cima fica a barra.
- **Cabeçalho (44 px):** dia por extenso em negrito 15 px `texto-forte` ("Sexta-feira, 23 de outubro"), "hoje · semana 43" apagado, setas `f104`/`f105` em células de 26 px, abas "dia" e "semana" (escolhida em negrito `destaque` com linha de 2 px embaixo), filtro em `amarelo` ou "texto para filtrar" em `texto-escuro`. Linha de 1 px `selecao` embaixo.
- **Colunas:** linha do tempo (57,5% do que sobra), pendências e uma coluna fixa de 330 px; separadas por 1 px `selecao`, margem interna de 16 px. Na vista de semana, a semana ocupa as duas primeiras.
- **Linha do tempo:** horas de 7 às 23 (ampliadas se algum item cai fora) em 11 px `texto-escuro`; grade em `fundo` misturado 55% a `selecao`. Faixa "agenda" (55%) com os compromissos em blocos `fundo-claro` de borda 1 px `mudo` (o que está acontecendo, 2 px `destaque`; o que já passou, texto apagado; sobrepostos dividem a largura), título em negrito e "09:00–10:00 · local" quando cabe. Faixa "rotinas" com uma linha de 20 px por execução: glifo de estado (`f00c` `verde` concluída, `f00d` `vermelho` com falha, `f110` `amarelo` em execução, `f017` `texto-escuro` agendada), hora apagada e título; execuções próximas descem para não se sobrepor. Hora atual: linha de 2 px `destaque` por trás das execuções e a hora em negrito `destaque` na coluna das horas. Compromisso de dia inteiro: bloco de 22 px acima da grade.
- **Detalhe:** preso no pé da linha do tempo, linha de 2 px `destaque` em cima; título em negrito, estado ou horário à direita, fonte e duração apagadas, texto em poço (`fundo` 25% mais escuro) com quebra de linha e altura pelo texto (até 10 linhas), botões `selecao` (hover `mudo`, texto `texto-forte`).
- **Semana:** sete colunas com "sex" e o número no cabeçalho (hoje em negrito `destaque` com linha de 2 px), compromissos em blocos de 40 px (hora apagada e título), execuções resumidas ("4 execuções", ou "4 · 1 com falha" em `vermelho`).
- **Pendências:** quadrado de 12 px com borda apagada (concluída: cheio em `verde`, título riscado em `texto-escuro`, no mesmo lugar até a central fechar; depois vai para a aba "concluídas"); título `texto-forte`; prazo à direita ("atrasado" `vermelho`, "hoje" `amarelo`, "amanhã", "ter 27", "30/10" apagados, datas sempre com dois dígitos); segunda linha com glifo da fonte e origem apagadas (partes que são caminho de arquivo ficam de fora); terceira linha só com comando, em `destaque`. Hover `fundo-claro` na linha. Sessão aguardando permissão e fonte sem execução hoje entram no topo com `f071`/glifo da fonte em `amarelo`. Abaixo do título "Pendências", a coluna (pendências e prazos) rola com a roda do mouse; fora dela a roda troca o dia. Barra de posição de 3 px em `selecao` na borda direita quando o conteúdo passa da altura. Abas "em aberto N" e "concluídas N" à direita do título, no estilo das abas dia/semana; "concluídas" lista as marcadas nos últimos 30 dias (`concluidas.jsonl`), com a hora (hoje) ou a data, e o quadrado reabre.
- **Prazos:** rótulo "prazos" e "próximos 60 dias" na cor dos rótulos de seção, linhas de 24 px com glifo da fonte, título `texto` e data relativa.
- **Mês:** o calendário do mês em células de 26 px (iniciais "D S T Q Q S S", fim de semana apagado, meses vizinhos em `texto-escuro`, hoje em negrito `destaque` com linha de 2×16 px, dia escolhido com borda 1 px `mudo`), marca de 4 px no canto dos dias com itens (apagada; `vermelho` com falha).
- **Rotinas:** agrupadas em "diárias", "semanais", "mensais", "anuais" e "outras" (pelo `quando` ou pela expressão cron), cada grupo com o nome em negrito apagado; duas linhas de 18 px: glifo do estado da última execução, título e próxima data à direita; nome da fonte e o resto da periodicidade ("dia 10") apagados, com o comando em `destaque` à direita quando a fonte oferece.
- **Sessões do Claude:** pasta da sessão e estado ("em execução há 12 min", "aguarda permissão", "concluída às 11:07").
- **Fontes:** só as com erro, glifo em `amarelo`, mensagem apagada com quebra de linha e a última sincronização.
- **Pé (28 px):** linha `selecao` em cima; teclas apagadas e o que fazem em `texto-escuro`.

### Cards / Containers (cartão de grupo)
- **Corner Style:** reto (0 px).
- **Background:** `fundo` no cabeçalho e no pé; `fundo-claro` na área de miniaturas.
- **Shadow Strategy:** nenhuma (ver Elevation & Depth).
- **Border:** 2 px por dentro, por prioridade: `amarelo` destino do arrasto, `texto-forte` escolhido pelo teclado, `destaque` grupo ativo, `texto-escuro` hover, `mudo` repouso.
- **Cabeçalho:** nome do grupo em negrito à esquerda (`destaque` se ativo, senão `texto`) e contagem à direita ("Nenhuma janela", "1 janela", "{n} janelas"), 12 px de recuo.

### Cartão "Criar grupo"
Último cartão da grade, do mesmo tamanho dos outros, só borda (mesma prioridade de cor) e "+  Criar grupo" centralizado em 13 px, `texto` (`amarelo` quando é destino de arrasto). O contrato de direção previa uma faixa de 56 px à direita; a build entregou o cartão de tamanho cheio.

### Quadro de janela (visão geral e Alt+Tab)
- **Miniatura:** DWM ao vivo, inclusive de janelas estacionadas fora do monitor.
- **Borda por fora:** `destaque` 2 px selecionada, `texto` 2 px em hover, `mudo` 1 px em repouso.
- **Minimizada na visão geral:** faixa de 22 px em `selecao` no pé do cartão, ícone de 14 px a 80% e título em `texto`.
- **Rótulo do Alt+Tab:** ícone de 16 px, título em `texto` (`texto-forte` selecionada) e nome do grupo alinhado à direita em `texto-apagado`. Minimizada mostra fundo `fundo-claro` e o ícone de 48 px no centro a 70%.

### Linha de atalhos da visão geral
Texto de 12 px em `texto`, centralizado, itens no formato "Tecla: ação" separados por "  ·  ". Win+Tab e Alt+Tab têm listas próprias. Com filtro ativo, a linha vira "Filtro: {texto}" em `destaque`.

### Moldura das outras janelas
Aplicada pelo DWM: modo escuro conforme o tema, barra de título `fundo`, texto `texto`, borda `destaque` na janela em primeiro plano e `mudo` nas outras, cantos retos, sem Mica nem Acrylic (`DWMWA_SYSTEMBACKDROP_TYPE` em nenhum; nos apps XAML, por baixo do conteúdo transparente fica a cor da barra de título). Terminais recebem como ícone da barra de título o glifo `>_` (`f120`) em negrito, na cor `texto`, gerado em 16 e 32 px. Tudo volta ao padrão do Windows ao desligar a opção ou sair.

A barra pinta de fora as janelas com botão na barra. Diálogos com dona e janelas de ferramentas (Variáveis de Ambiente, caixas de mensagem de um programa) recebem a mesma moldura de dentro do programa, pela `barra_menus.dll`, com a borda em `destaque` enquanto estão ativas.

**Botões da barra de título.** Minimizar, maximizar (restaurar, com a janela maximizada) e fechar viram os glifos do menu de sistema no tema (`f2d1`, `f2d0`, `f2d2`, `f00d`), em 14 px, cada um numa célula de 28 px, colados à borda direita e da borda de cima até a área do programa. Fundo `fundo`, o mesmo da barra de título; glifo em `texto` com a janela ativa e em `texto-apagado` sem ela. Sob o mouse, fundo `fundo-claro` e glifo `texto-forte`, e o fechar com o glifo em `vermelho`; apertado, fundo `selecao`. Sem dica.

A `barra_menus.dll` faz isso dentro do programa: tira o `WS_SYSMENU`, e o DWM deixa de desenhar os botões e o ícone (o glifo `>_` dos terminais não aparece); por cima fica uma janela da própria janela (owned popup), que acompanha posição, tamanho, foco e minimização. O clique manda o mesmo `WM_SYSCOMMAND` dos botões do Windows; duplo clique no título maximiza e Alt+F4 fecha. Sem o menu de sistema, Alt+Espaço e o clique direito no título não abrem nada. Vale para as janelas com botão na barra em que os botões são do Windows: barra de título desenhada pelo Windows (área do programa começando abaixo da altura de título), ou área do programa estendida até o alto com os botões do DWM por cima, como nos apps XAML (Gerenciador de Tarefas, Configurações). Nesse caso os botões ficam no lugar e na altura dos do DWM (`DWMWA_CAPTION_BUTTON_BOUNDS`), e a janela só entra se o `DwmDefWindowProc` disser que o ponto do fechar é do DWM. Navegadores e Windows Terminal desenham os próprios botões e ficam como estão, e os programas que a DLL não alcança (32 bits, administrador) ficam com os do Windows. Desligar o tema das janelas ou sair devolve os botões do Windows.

### Menus
Menus de contexto do Win32 (teclado, submenus e posição do Windows), com os itens desenhados pela barra no tema.
- **Janela:** cantos retos, sem sombra e sem animação de abertura; moldura de 3 px com borda de 2 px em `destaque` e o resto em `fundo`.
- **Item:** 26 px de altura, texto de 12 px em `texto` a partir de 28 px da esquerda; selecionado com fundo `selecao` e texto `texto-forte`; desabilitado em `mudo`.
- **Marcado:** glifo `f00c` em `destaque` na coluna de 28 px e texto em negrito.
- **Atalho:** depois do texto, alinhado à direita, em `texto-apagado`.
- **Submenu:** glifo `f105` em `texto-apagado` numa coluna de 24 px à direita.
- **Separador:** linha de 1 px em `mudo`, recuada 10 px, em item de 9 px.
- **Textos:** infinitivo ou imperativo formal: "Criar grupo", "Renomear grupo", "Mover para o grupo", "Fechar janela", "Aplicar o tema às bordas das janelas", "Sair".

Menus de outros programas (bandeja, clique direito, menu de sistema) e a barra de menus clássica ("Arquivo  Editar…") seguem as mesmas regras de item, cor e moldura. A barra instala ganchos globais e a `barra_menus.dll` faz o trabalho dentro de cada programa: modo claro ou escuro do uxtheme (`SetPreferredAppMode`), itens convertidos em desenho próprio só enquanto o menu está aberto, e a faixa da barra de menus pintada pelas mensagens `WM_UAHDRAWMENU`/`WM_UAHDRAWMENUITEM`. Itens com ícone pedido ao programa (`HBMMENU_CALLBACK`) são convertidos do mesmo jeito, e o ícone continua vindo do programa. Menus que o próprio programa desenha pelo tema do Windows (o de contexto do Explorador, que faz os itens owner-draw e pinta pela classe `Menu`) recebem as mesmas cores pela classe: fundo `fundo`, item selecionado `selecao`, separador e seta em `mudo` e `texto-apagado`, borda de 2 px em `destaque`. Programas de 32 bits ficam de fora; os que rodam como administrador também, quando a barra não roda como administrador.

### Controles das outras janelas
Controles Win32 dos outros programas planos e de canto vivo, nos papéis do tema, como nos mods Translucent Windows e Win32 UI Modernizer do Windhawk. A `barra_menus.dll` desvia, dentro de cada programa, as funções em que os controles escolhem cor e se desenham.
- **Cores do sistema** (`GetSysColor`, `GetSysColorBrush`, `FillRect` com pincel de índice): janela, botão, menu e barras de título em `fundo`; texto em `texto`; texto desabilitado e bordas 3D em `mudo`; seleção em `selecao`; link em `destaque`. Vale só dentro do programa, sem `SetSysColors`.
- **Fundo e texto padrão de janelas e diálogos** (`WM_CTLCOLOR*`, `WM_ERASEBKGND`): troca só o que veio do padrão do Windows; cores escolhidas pelo programa ficam. A textura das abas nos diálogos de propriedades sai.
- **Botão:** `fundo-claro` com borda `mudo`; sob o mouse `selecao`; apertado `selecao` com borda `destaque`; botão padrão com borda `destaque`.
- **Caixa de seleção e opção:** caixa com borda `texto-apagado` (`texto` sob o mouse); marcada, preenchida em `destaque` com o glifo `f00c` em `fundo`; opção com os glifos `f10c` e `f192`.
- **Campo e combo:** `fundo` com borda `mudo`, `destaque` no foco; seta da lista no glifo `f107`.
- **Abas:** `fundo` com borda `mudo`; a escolhida sem a linha de baixo e com 2 px de `destaque` em cima.
- **Lista, árvore e cabeçalho:** `fundo-claro` sob o mouse, `selecao` no item escolhido; setas da árvore e de ordenação em glifos `texto-apagado`.
- **Barra de rolagem:** trilho `fundo`, polegar `mudo` numa faixa de metade da largura (`texto-apagado` sob o mouse, `texto` arrastando), setas em glifos.
- **Progresso:** trilho `fundo-claro`, preenchimento `destaque`, `vermelho` no erro e `texto-apagado` em pausa, sem o brilho que corre por cima.
- **Dica:** como a da barra, `fundo` com borda de 2 px em `destaque`.
- **Caixa de tarefa e de mensagem:** área principal em `fundo`; faixa de baixo, com os botões, em `fundo-claro` com uma linha `mudo` em cima.

- **Apps XAML (WinUI 2 e 3):** os pincéis de tema do WinUI nos mesmos papéis. Fundos (`SolidBackgroundFillColor*`, `LayerFillColor*`, `ApplicationPageBackgroundThemeBrush`) em `fundo`, cartões e camadas elevadas em `fundo-claro`; botão (`ControlFillColorDefault`) em `fundo-claro`, sob o mouse e apertado em `selecao`; bordas e divisórias em `mudo`; texto em `texto`, secundário em `texto-apagado`, desabilitado em `mudo`; destaque em `destaque` e texto sobre ele em `fundo`; erro em `vermelho`. Pincel do próprio app com a cor de um desses ganha o mesmo papel. No modo escuro, pincel sem papel com os cinzas do Windows (#191919, #1C1C1C, #202020) fica em `fundo`, #2B2B2B e #2C2C2C em `fundo-claro`, e o branco em `texto` com o mesmo alfa. A raiz da janela, transparente, recebe `fundo`.
- **DirectUI (Gerenciador de Tarefas):** fundo #191919 em `fundo`, texto branco em `texto`, #CDCDCD em `texto-apagado`, seleção e bordas em branco translúcido passam a `texto` com o mesmo alfa, mapa de calor em `destaque` com o mesmo alfa; as cores dos gráficos ficam.

Trocar o tema refaz as janelas abertas (`WM_SYSCOLORCHANGE`, `WM_THEMECHANGED` e redesenho; nos apps XAML, o temporizador de cada linha de execução da interface vê a troca em até 1 s). Programas sem os controles comuns versão 6 (sem manifesto) desenham os botões no estilo clássico, com bordas 3D que ainda saem nas cores do Windows.

### Caixa de renomear grupo
Campo de texto do Win32 sobre o cabeçalho do cartão: CaskaydiaMono Nerd Font 12 px em negrito, `texto-forte` sobre `fundo-claro`.

## Do's and Don'ts

### Do:
- **Tire** toda cor de um papel do `Tema` ativo e confira a tela em pelo menos um tema claro (catppuccin-latte, flexoki-light, lupine, rose-pine, white).
- **Use** CaskaydiaMono Nerd Font em 12 px para texto novo, com negrito para "ativo" fora da barra; o único tamanho maior é o título de 15 px da visão geral.
- **Marque** foco e seleção com `destaque` em linha ou borda de 2 px.
- **Use** `texto` misturado 30% com `fundo` para estado apagado.
- **Multiplique** toda medida pela escala de DPI.
- **Escreva** rótulos em português formal, com contagem por extenso no zero ("Nenhuma janela") e singular no um ("1 janela").

### Don't:
- **Não arredonde** cantos nem adicione sombra, brilho ou desfoque.
- **Não ponha** título de janela na barra; botão de janela é só ícone.
- **Não ponha** logotipo do Windows ou botão Iniciar na barra.
- **Não desenhe** botões em forma de pílula na barra; grupos e itens são texto ou ícone sobre o fundo, com realce só no hover.
- **Não use** `texto_escuro` para texto apagado.
- **Não pinte** glifos da Nerd Font fora dos papéis do tema, nem troque por glifo o ícone de programas fora da lista de `janelas::glifo`.
- **Não use** amarelo, verde ou vermelho como decoração.
