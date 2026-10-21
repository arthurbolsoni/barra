# barra

Barra de tarefas própria para o Windows 11, com grupos de janelas no lugar das áreas de trabalho virtuais.

Só as janelas do grupo ativo ficam na tela. As dos outros grupos ficam estacionadas fora dos monitores, sem minimizar, e voltam ao mesmo lugar quando o grupo volta.

## Uso

| Onde | Clique | Botão direito | Roda |
| --- | --- | --- | --- |
| Iniciar | abre o menu Iniciar do Windows | | |
| Grupo | troca para o grupo | novo grupo, apagar grupo | grupo seguinte/anterior |
| `+` | cria um grupo vazio e vai para ele | | grupo seguinte/anterior |
| Janela | traz para a frente; se já está na frente, minimiza | mover para outro grupo, novo grupo com ela, fechar | |
| Rede/volume | configurações rápidas (Win+A) | | |
| Relógio | notificações e calendário (Win+N) | | |
| Espaço vazio | | esconder a barra do Windows, iniciar com o Windows, juntar tudo num grupo, sair | |

- Janela nova entra no grupo ativo. Ao abrir a barra, tudo o que está aberto vai para o grupo 1.
- Janela de outro grupo que vem para a frente (Alt+Tab, barra do Windows, notificação) troca para o grupo dela.
- Janela que pede atenção fica laranja.
- Janelas em outras áreas de trabalho virtuais ficam de fora.
- A barra é uma AppBar: o Windows desconta a faixa dela da área útil.

## Segurança das janelas estacionadas

- Ao sair, desligar ou dar erro, todas as janelas voltam para a tela.
- A lista das estacionadas fica em `%LOCALAPPDATA%\barra\estacionadas.txt`; se a barra morrer, a próxima execução traz todas de volta.
- `barra --sair` fecha a barra que está rodando.
- "Esconder a barra do Windows" passa a barra dele para ocultar automaticamente e a esconde; ao sair, volta como estava (estado guardado em `HKCU\Software\barra`).

## Build

```
cargo build --release
```

`barra --preview <arquivo.bmp>` desenha uma barra de exemplo em BMP, sem abrir nada na tela.
