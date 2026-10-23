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
| Relógio | notificações e calendário (Win+N) | | |
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
- Rede: as placas físicas, com o IP ou "desconectada", e as VPNs ligadas. Abaixo, o Bluetooth: interruptor do rádio e os dispositivos pareados (clássicos e BLE). Fone e caixa conectam e desconectam ali (pedido ao filtro de áudio Bluetooth, como o ToothTray); os outros abrem as Configurações de Bluetooth.
- O Bluetooth é lido numa thread à parte e relido a cada 3 s com o painel aberto.

## Visão geral

Abre com Alt+Tab, Win+Tab ou pelo menu da barra. Mostra todos os grupos ao mesmo tempo, com miniaturas ao vivo do DWM no lugar onde cada janela fica.

- Alt+Tab: Tab anda pela ordem de uso (Shift volta); soltar o Alt vai para a janela escolhida. A visão geral aparece já no primeiro Tab (o primeiro quadro sai em menos de 10 ms); toque rápido troca para a janela anterior.
- Clique numa janela vai para ela; clique no cartão vai para o grupo; "Novo grupo" cria um.
- Arrastar uma janela para outro cartão muda o grupo dela; para "Novo grupo", cria um grupo com ela.
- Digitar filtra pelo título; setas andam entre as janelas; Enter abre; Esc fecha.
- Duplo clique no nome do grupo renomeia.
- Com janela de programa rodando como administrador em foco, o Alt+Tab do Windows continua.

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

Gera `barra.exe`, `barra_menus.dll` e `barra_gancho.dll` em `target\release`; as DLLs ficam ao lado do exe.

`barra --preview <arquivo.bmp>` desenha uma barra de exemplo em BMP, sem abrir nada na tela.
