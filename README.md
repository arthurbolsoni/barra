# barra

Barra de tarefas própria para o Windows 11, no lugar da do Windows, com grupos de janelas no lugar das áreas de trabalho virtuais.

Só as janelas do grupo ativo ficam na tela. As dos outros grupos ficam estacionadas fora dos monitores, sem minimizar, e voltam ao mesmo lugar quando o grupo volta.

## Barra

| Onde | Clique | Botão direito | Roda |
| --- | --- | --- | --- |
| Grupo | troca para o grupo | renomear, novo grupo, apagar grupo | grupo seguinte/anterior |
| `+` | cria um grupo vazio e vai para ele | | grupo seguinte/anterior |
| Janela | traz para a frente; se já está na frente, minimiza | mover para outro grupo, novo grupo com ela, fechar | |
| Ícone da bandeja | repassa o clique ao programa (duplo clique também) | menu do programa | |
| Rede | painel de rede e Bluetooth | | |
| Volume | painel de volume | | sobe/desce 2% (clique do meio: mudo) |
| Relógio | central (agenda, rotinas, pendências); também Win+C | | |
| Espaço vazio | | visão geral, tema, tema nas janelas dos outros programas, tema nas cores do Windows, esconder a barra do Windows, Alt+Tab abre a visão geral, iniciar com o Windows, juntar tudo num grupo, sair | |

- Janela nova entra no grupo ativo. Ao abrir a barra, tudo o que está aberto vai para o grupo 1.
- Janela de outro grupo que vem para a frente troca para o grupo dela.
- Janela que pede atenção fica laranja; progresso (download, cópia) aparece numa faixa no pé do botão, e o selo do programa no canto do ícone.
- Passar o mouse num botão ou ícone da bandeja mostra o título/dica.
- Janelas em outras áreas de trabalho virtuais ficam de fora.
- A barra é uma AppBar: o Windows desconta a faixa dela da área útil.
- Programa em tela cheia por cima no monitor da barra (vídeo, jogo, F11): a barra fica atrás dele até ele sair de tela cheia ou outra janela vir para a frente.

## Painéis de rede e volume

Abrem embaixo do ícone, no tema da barra, sem tirar o foco da janela em uso. Fecham com clique fora, Esc ou outro clique no ícone.

- Volume: nível geral (arrastar, clicar no trilho ou rodar; o glifo liga o mudo), saída de som (vira a padrão nos três papéis, pela `IPolicyConfig`) e volume de cada programa que está com som aberto.
- Rede: as placas físicas, com o IP ou "Desconectada", e as VPNs ligadas. Abaixo, o Bluetooth: interruptor do rádio e os dispositivos pareados (clássicos e BLE). Fone e caixa conectam e desconectam ali (pedido ao filtro de áudio Bluetooth, como o ToothTray); os outros abrem as Configurações de Bluetooth.
- O Bluetooth é lido numa thread à parte e relido a cada 3 s com o painel aberto.

## Visão geral

Abre com Alt+Tab, Win+Tab ou pelo menu da barra. Mostra todos os grupos ao mesmo tempo, com miniaturas ao vivo do DWM no lugar onde cada janela fica.

- Alt+Tab: Tab anda pela ordem de uso (Shift volta); soltar o Alt vai para a janela escolhida. A visão geral aparece já no primeiro Tab (o primeiro quadro sai em menos de 10 ms); toque rápido troca para a janela anterior.
- Clique numa janela vai para ela; clique no cartão vai para o grupo; "Novo grupo" cria um.
- Arrastar uma janela para outro cartão muda o grupo dela; para "Novo grupo", cria um grupo com ela.
- Digitar filtra pelo título; setas andam entre as janelas; Enter abre; Esc fecha.
- Duplo clique no nome do grupo renomeia.
- Com janela de programa rodando como administrador em foco, o Alt+Tab do Windows continua.

## Central

Abre pelo relógio ou com Win+C, embaixo do relógio. Reúne agenda, rotinas, pendências e prazos de várias fontes; fecha com Esc, novo clique no relógio ou clique fora.

- Dia: linha do tempo com os compromissos à esquerda e as execuções das rotinas à direita, com a hora atual em destaque. Clique num item abre o detalhe no pé da coluna (descrição ou resumo da execução, registro, executar de novo, entrar na reunião).
- Semana: domingo a sábado, com os compromissos e o resumo das execuções de cada dia; clique num dia abre o dia.
- Pendências: o quadrado marca como concluída; o resto da linha abre o arquivo ou endereço da pendência. Fonte que oferece comando e não rodou hoje aparece como pendência, com o botão para executar. Sessão do Claude aguardando permissão também.
- Prazos dos próximos 60 dias; mês com marca nos dias que têm itens (vermelha com execução com falha); rotinas com a última execução e a próxima; sessões do Claude em andamento ou concluídas na última meia hora; fontes com erro.
- Teclas: ←→ dia (semana, na vista de semana), Tab alterna dia e semana, Home volta para hoje, texto filtra pelo título, Esc limpa o filtro ou fecha.
- Clique direito em qualquer item (compromisso, execução, detalhe aberto, pendência, concluída, prazo, rotina, sessão do Claude, fonte com erro, dia do calendário) abre o menu: "Copiar" põe o texto do item (título, data, local, link, descrição ou resumo) na área de transferência; "Abrir com o Claude" abre o Claude Code (`claude --dangerously-skip-permissions`) num pwsh novo no console do Windows, na pasta da fonte (na da sessão, para uma sessão do Claude), com esse texto já enviado. O dia do calendário copia os compromissos, execuções e prazos dele. Ctrl+C copia o item aberto no detalhe ou o que está sob o cursor. Clicar na linha de uma pendência ou de um prazo não abre o arquivo dela.
- Na barra, ao lado do relógio: o próximo compromisso nos 30 minutos que o antecedem (amarelo nos últimos 5) e um quadrado vermelho quando há execução de hoje com falha ou sessão do Claude aguardando permissão.

### Fontes

Cada fonte grava `%LOCALAPPDATA%\barra\central\<fonte>.json`; a barra relê a pasta quando algum arquivo muda.

| Fonte | Quem grava |
| --- | --- |
| `agenda` | `barra-central`, a partir do Google Agenda e da agenda do Outlook.com (iCal) |
| `soul` | skill `rotinas` do soul (`D:\Desktop\soul`), no começo e no fim de cada execução |
| `agentes` | `barra-central`, a partir das tarefas e execuções do claude-hooks (`~/.claude-hooks`) |
| `claude` | hooks do Claude Code, um arquivo por sessão em `central\claude\` |

Formato de `<fonte>.json`:

```json
{
  "versao": 1,
  "fonte": "soul",
  "descricao": "soul · D:\Desktop\soul",
  "atualizado": "2026-10-23T10:30:00-03:00",
  "erro": "texto, quando a última leitura falhou (opcional)",
  "executar": { "rotulo": "Executar as rotinas", "pasta": "D:\Desktop\soul", "comando": "claude \"/rotinas\"" },
  "itens": [
    { "tipo": "evento", "id": "…", "titulo": "…", "inicio": "2026-10-23T09:00", "fim": "2026-10-23T09:15", "local": "…", "descricao": "…", "link": "…", "reuniao": "…" },
    { "tipo": "execucao", "id": "…", "titulo": "…", "inicio": "2026-10-23T07:30", "fim": "…", "estado": "ok | falha | executando | agendada", "resumo": "…", "registro": "caminho ou endereço", "executar": { "…": "…" } },
    { "tipo": "pendencia", "id": "…", "titulo": "…", "origem": "…", "prazo": "2026-10-30", "link": "caminho ou endereço", "executar": { "…": "…" } },
    { "tipo": "prazo", "id": "…", "titulo": "…", "data": "2026-11-20", "link": "…" },
    { "tipo": "rotina", "id": "…", "titulo": "…", "quando": "diária", "ultima": { "data": "2026-10-23", "estado": "ok" }, "proxima": "2026-10-24", "executar": { "…": "…" } }
  ]
}
```

- Datas: `AAAA-MM-DD` (dia inteiro), `AAAA-MM-DDTHH:MM` no horário local ou ISO 8601 com fuso. Campos fora `tipo`, `titulo` e as datas obrigatórias de cada tipo são opcionais.
- `executar` abre o comando (sintaxe do PowerShell) num pwsh novo no console do Windows, na `pasta`, sem elevação (pelo Explorer, mesmo com a barra rodando como administrador). O comando vai num `.ps1` em `%LOCALAPPDATA%\barra\terminal`, apagado depois de um dia.
- Pendência marcada na central vai para `central\acoes.jsonl`, uma linha por ação: `{"quando", "fonte", "id", "titulo", "acao": "concluir" | "reabrir"}`. A fonte aplica no registro dela e apaga as linhas que já aplicou; até lá, a barra mostra a pendência riscada.

### Serviço `barra-central`

A barra copia `barra-central.exe` para `%LOCALAPPDATA%\barra\bin` e abre o serviço ao iniciar (sem janela, uma instância só). Quando o executável compilado muda, a barra encerra o serviço, troca a cópia e abre de novo.

- Agenda: lida pelo endereço iCal de cada agenda (o "endereço secreto no formato iCal" do Google Agenda, o calendário publicado do Outlook.com), só leitura. Expande eventos recorrentes (`RRULE`, `EXDATE`, exceções por `RECURRENCE-ID`) e converte o fuso do `VTIMEZONE` para a hora local. Sincroniza a cada 5 minutos, de 14 dias atrás a 60 dias à frente, e também quando a central abre e a última sincronização tem mais de um minuto. Configuração em `%LOCALAPPDATA%\barra\agenda-config.json`: `{ "agendas": [{ "nome": "Google", "ical": "https://…" }], "intervalo_min": 5 }`; os endereços não vão para o log nem para as mensagens de erro. Agenda com erro fica com os eventos da última leitura e o erro aparece em "Fontes".
- `barra-central agenda [de] [ate]` lê as agendas na hora e imprime um evento JSON por linha (padrão: de hoje a 7 dias); a skill `agenda` do Claude Code usa esse comando.
- Agentes: relê o claude-hooks a cada 30 segundos. Tarefa vira rotina (com `claude-hooks run <tarefa>`), execução dos últimos 14 dias vira execução, e a execução mais recente de cada tarefa que terminou com `acao_necessaria` vira pendência. Sem `~/.claude-hooks`, a fonte não aparece.
- `barra-central hook`: chamado pelos hooks do Claude Code (`SessionStart`, `UserPromptSubmit`, `Notification`, `PostToolUse`, `Stop`, `SessionEnd` em `~/.claude/settings.json`) com o evento no stdin. Estados: `trabalhando`, `permissao`, `concluida`; `SessionEnd` apaga o arquivo da sessão.
- `barra-central sair` encerra o serviço; `barra-central agentes` lê o claude-hooks uma vez. Registro em `%LOCALAPPDATA%\barra\central.log`.

## Barra do Windows e bandeja

"Esconder a barra do Windows" (ligado por padrão) passa a barra dele para ocultar automaticamente e a esconde. Os ícones da bandeja dos outros programas vêm para a barra pelo mesmo truque do ManagedShell/RetroBar: uma janela `Shell_TrayWnd` invisível, à frente da do Explorer, recebe os `Shell_NotifyIcon` e repassa tudo ao Explorer. A propriedade `TaskbandHWND` nela recebe progresso e selo do `ITaskbarList3`.

Ao sair, a barra do Windows volta como estava (estado guardado em `HKCU\Software\barra`) e os programas mandam os ícones de novo para ela.

## Cores do Windows

"Aplicar o tema às cores do Windows" (ligado por padrão) troca a cor de destaque do Windows pela `destaque` do tema e o modo claro ou escuro pelo do tema. Os programas que seguem o Windows (Configurações, Explorador, apps WinUI, navegadores) trocam na hora. A cor vai pelo mesmo caminho das Configurações (`SetUserColorPreference` da uxtheme), e o Windows gera a paleta de destaque a partir dela.

- A política "Forçar uma cor de fundo e de destaque" (`PersonalColors_*` em `HKLM\Software\Policies\Microsoft\Windows\Personalization`) faz o Windows ignorar a troca. Enquanto o tema está aplicado, ela fica desligada, o que exige a barra rodando como administrador.
- Os valores de antes ficam em `HKCU\Software\barra\CoresDoWindows` e voltam ao sair, ao desligar a opção e, depois de uma queda, na próxima saída.

## Tema nos outros programas

"Aplicar o tema às janelas e aos menus dos outros programas" (ligado por padrão) leva o tema para dentro dos outros programas: molduras, botões da barra de título, menus Win32, controles (botões, caixas de seleção, campos, abas, listas, barras de rolagem, caixas de mensagem) e apps XAML. O desenho está no DESIGN.md.

- A barra instala ganchos globais (`SetWindowsHookEx`) com a `barra_gancho.dll`, que o Windows carrega em cada programa com janela. Ela carrega lá a `barra_menus.dll`, que faz o trabalho: desvia as funções de cor e de desenho dos controles (crate `retour`), converte os menus em desenho próprio e põe os botões da barra de título.
- As DLLs usadas são cópias em `%LOCALAPPDATA%\barra\gancho` e `%LOCALAPPDATA%\barra\menus`, com o hash do conteúdo no nome: os programas seguram a DLL até fechar, e o arquivo do build fica livre. Ao abrir, a barra apaga as cópias que nenhum programa segura e desliga as que ainda estão em algum.
- O Windows guarda as DLLs de gancho global numa tabela de 32 posições por sessão, e uma posição só vaga quando todos os programas que carregaram a DLL a soltam. Por isso o gancho fica na `barra_gancho.dll`, que quase nunca muda e ocupa sempre a mesma posição; a `barra_menus.dll`, que muda a cada versão, entra por `LoadLibrary`, fora da tabela. Com a tabela cheia, o `SetWindowsHookEx` falha com o erro 126; a barra registra a falha em `%LOCALAPPDATA%\barra\barra.log` e tenta de novo a cada 30 s. A tabela esvazia ao sair da sessão do Windows.
- Alcança programas de 64 bits. Com a barra rodando como administrador, alcança também os programas que rodam como administrador. As pastas das cópias dão leitura e execução a "ALL APPLICATION PACKAGES" e "ALL RESTRICTED APPLICATION PACKAGES"; sem isso os apps da Loja (AppContainer) não carregam a DLL.
- Apps XAML (UWP com WinUI 2, e WinUI 3): quando o programa carrega o XAML (`Windows.UI.Xaml.dll` ou `Microsoft.Internal.FrameworkUdk.dll`), a `barra_menus.dll` se registra como TAP do XAML Diagnostics do próprio processo (`InitializeXamlDiagnosticsEx`), como os mods "Styler" do Windhawk, e troca a cor dos pincéis de tema nos dicionários de `Application.Resources`. Os pincéis do WinUI vão pelo nome; os do próprio app e os do framework, pela cor original. O pincel selado do framework (texto padrão) é trocado por outro com o `ReplaceResource`, o mesmo do Hot Reload. Com o XAML já carregado (programa aberto antes da barra), a DLL entra na hora, mas as janelas XAML que já existiam recebem só os pincéis dos dicionários.
- Apps com pacote que abrem numa `CoreWindow` (Configurações e outros UWP): o Windows não carrega DLL de gancho global neles. Quando uma janela dessas vem para o primeiro plano, a barra chama o `InitializeXamlDiagnosticsEx` de fora, com a `barra_menus.dll` como TAP. Suspenso (minimizado), o app não responde; a barra tenta de novo na próxima vez que ele vier para a frente. Uma conexão só por processo: se o gancho já conectou, a segunda é ignorada.
- A raiz de cada janela XAML (o `Border` sob o `RootScrollViewer`) ganha o fundo do tema: ela é transparente, e por baixo ficava o Mica ou o cinza #202020 que o substitui.
- O XAML Diagnostics só avisa os elementos da primeira ilha XAML do processo. As outras (no Gerenciador de Tarefas: cabeçalho, busca e corpo das páginas) têm pincéis próprios fora dos dicionários; para elas a DLL desvia o `put_Color` e o `CreateColorBrushWithColor` da composição (`dcomp.dll`, endereços tirados da vtable de um `Compositor` criado no processo) e troca, no modo escuro, os cinzas #191919, #1C1C1C e #202020 por `fundo`, #2B2B2B e #2C2C2C por `fundo-claro` e o branco por `texto`, mantendo o alfa.
- DirectUI (`DUI70.dll`, as páginas do Gerenciador de Tarefas): cores do código e das folhas de estilo desviadas em `Value::CreateColor` e `Element::SetBackgroundColor`/`SetForegroundColor`/`SetBorderColor`. No modo escuro, fundo #191919 em `fundo`, texto branco em `texto`, #CDCDCD em `texto-apagado`, o azul do mapa de calor em `destaque`; o alfa fica.
- "Explorador de Arquivos clássico, sem abas" (menu da barra) anula no HKCU as classes da interface XAML do Explorador (`{2aa9162e-…}` e `{6480100b-…}`, `InprocServer32` vazio): as janelas novas abrem com a faixa de opções, sem abas, todas em Win32 e DirectUI. Desligar apaga as duas chaves.
- Faixa de opções (`UIRibbon`, no Explorador clássico): as cores globais do framework (`UI_PKEY_GlobalBackgroundColor`, `HighlightColor`, `TextColor`) recebem o fundo, o destaque e o texto do tema. Os métodos do `IUIFramework` são desviados pela vtable de um framework criado numa linha STA; as cores vão no `LoadUI` e na primeira invalidação de comando de cada framework, e de novo quando o Explorador grava as do modo escuro por cima. O brilho do HSB sai pela reta dos valores do próprio Explorador (fundo #202020 com 15, texto branco com 250).
- Cinzas do modo escuro desenhados com pincel ou cor próprios (`FillRect` com pincel sólido e `SetBkColor` em #191919, #1C1C1C, #202020, #2B2B2B, #2C2C2C), como o painel da esquerda do Explorador, e as classes de tema `AddressBand` e `SearchBox` (barra de endereço e busca) também ganham as cores do tema.
- O Explorer e os processos da shell (Visão de Tarefas, Iniciar, pesquisa, notificações) ficam de fora do tema XAML, assim como processo com os dois frameworks XAML: com o diagnóstico ligado o Explorer caiu (o CoreMessaging encerra o processo por um callback com erro no `Windows.UI.Xaml.dll`).
- Desligar a opção ou sair devolve o padrão do Windows.

## Segurança das janelas estacionadas

- Ao sair, desligar ou dar erro, todas as janelas voltam para a tela.
- A lista das estacionadas fica em `%LOCALAPPDATA%\barra\estacionadas.txt`; se a barra morrer, a próxima execução traz todas de volta.
- `barra --sair` fecha a barra que está rodando.

## Início com o Windows

"Iniciar com o Windows" cria a tarefa agendada `barra`, que abre a barra como administrador no logon (sem pedir o UAC), sem limite de tempo e com prioridade normal. Sem elevação o Windows não entrega à barra as teclas de um programa elevado em primeiro plano (o Alt+Tab e o Win+Tab voltam a ser os do Windows) nem deixa os ganchos do tema entrarem nele. Ligada numa barra sem elevação, a opção usa a chave `Run` do registro; na próxima vez que a barra abrir como administrador, troca a chave pela tarefa.

## Build

```
cargo build --release
```

Gera `barra.exe`, `barra_menus.dll`, `barra_gancho.dll` e `barra-central.exe` em `target\release`; as DLLs e o serviço ficam ao lado do exe.

`barra --preview <arquivo.bmp>` desenha uma barra de exemplo em BMP, sem abrir nada na tela.
