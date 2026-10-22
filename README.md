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

## Painéis de rede e volume

Abrem embaixo do ícone, no tema da barra, sem tirar o foco da janela em uso. Fecham com clique fora, Esc ou outro clique no ícone.

- Volume: nível geral (arrastar, clicar no trilho ou rodar; o glifo liga o mudo), saída de som (vira a padrão nos três papéis, pela `IPolicyConfig`) e volume de cada programa que está com som aberto.
- Rede: as placas físicas, com o IP ou "desconectada", e as VPNs ligadas. Abaixo, o Bluetooth: interruptor do rádio e os dispositivos pareados (clássicos e BLE). Fone e caixa conectam e desconectam ali (pedido ao filtro de áudio Bluetooth, como o ToothTray); os outros abrem as Configurações de Bluetooth.
- O Bluetooth é lido numa thread à parte e relido a cada 3 s com o painel aberto.

## Visão geral

Abre com Alt+Tab, Win+Tab ou pelo menu da barra. Mostra todos os grupos ao mesmo tempo, com miniaturas ao vivo do DWM no lugar onde cada janela fica.

- Alt+Tab: Tab anda pela ordem de uso (Shift volta); soltar o Alt vai para a janela escolhida. Toque rápido troca para a janela anterior sem mostrar nada.
- Clique numa janela vai para ela; clique no cartão vai para o grupo; "Novo grupo" cria um.
- Arrastar uma janela para outro cartão muda o grupo dela; para "Novo grupo", cria um grupo com ela.
- Digitar filtra pelo título; setas andam entre as janelas; Enter abre; Esc fecha.
- Duplo clique no nome do grupo renomeia.
- Com janela de programa rodando como administrador em foco, ou jogo/vídeo em tela cheia, o Alt+Tab do Windows continua.

## Barra do Windows e bandeja

"Esconder a barra do Windows" (ligado por padrão) passa a barra dele para ocultar automaticamente e a esconde. Os ícones da bandeja dos outros programas vêm para a barra pelo mesmo truque do ManagedShell/RetroBar: uma janela `Shell_TrayWnd` invisível, à frente da do Explorer, recebe os `Shell_NotifyIcon` e repassa tudo ao Explorer. A propriedade `TaskbandHWND` nela recebe progresso e selo do `ITaskbarList3`.

Ao sair, a barra do Windows volta como estava (estado guardado em `HKCU\Software\barra`) e os programas mandam os ícones de novo para ela.

## Cores do Windows

"Aplicar o tema às cores do Windows" (ligado por padrão) troca a cor de destaque do Windows pela `destaque` do tema e o modo claro ou escuro pelo do tema. Os programas que seguem o Windows (Configurações, Explorador, apps WinUI, navegadores) trocam na hora. A cor vai pelo mesmo caminho das Configurações (`SetUserColorPreference` da uxtheme), e o Windows gera a paleta de destaque a partir dela.

- A política "Forçar uma cor de fundo e de destaque" (`PersonalColors_*` em `HKLM\Software\Policies\Microsoft\Windows\Personalization`) faz o Windows ignorar a troca. Enquanto o tema está aplicado, ela fica desligada, o que exige a barra rodando como administrador.
- Os valores de antes ficam em `HKCU\Software\barra\CoresDoWindows` e voltam ao sair, ao desligar a opção e, depois de uma queda, na próxima saída.

## Segurança das janelas estacionadas

- Ao sair, desligar ou dar erro, todas as janelas voltam para a tela.
- A lista das estacionadas fica em `%LOCALAPPDATA%\barra\estacionadas.txt`; se a barra morrer, a próxima execução traz todas de volta.
- `barra --sair` fecha a barra que está rodando.

## Build

```
cargo build --release
```

`barra --preview <arquivo.bmp>` desenha uma barra de exemplo em BMP, sem abrir nada na tela.
