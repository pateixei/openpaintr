# Referências técnicas do OpenPaintR

## OpenPDN-1 3.36.7

O repositório indicado preserva o código histórico do Paint.NET 3.36.7. A tabela identifica os arquivos consultados e como eles orientaram o primeiro protótipo. [Repositório](https://github.com/nakijun/OpenPDN-1)

| Fonte | Observação | Aplicação no OpenPaintR |
| --- | --- | --- |
| [Document.cs](https://github.com/nakijun/OpenPDN-1/blob/master/src/Data/Document.cs) | O documento possui dimensões e uma coleção de camadas. | `Document` gerencia as dimensões e a ordem das camadas. |
| [Layer.cs](https://github.com/nakijun/OpenPDN-1/blob/master/src/Data/Layer.cs) | A camada tem dimensões fixas e propriedades de apresentação. | Cada camada armazena pixels do tamanho da tela e expõe nome, visibilidade e opacidade no núcleo. |
| [BitmapLayer.cs](https://github.com/nakijun/OpenPDN-1/blob/master/src/Data/BitmapLayer.cs) | A camada de bitmap mantém uma superfície raster e aplica a opacidade na composição. | A implementação em Rust combina o alfa do pixel com a opacidade da camada usando a composição normal *source-over*. |

A licença histórica usa o texto MIT com exceções para logo, ícones, outros recursos e GPC. O protótipo usa implementação própria e gráficos gerados pelo código. Se um trecho histórico for incorporado, registre o arquivo e a revisão de origem, confira as condições aplicáveis e preserve os avisos exigidos. [Licença do OpenPDN-1](https://github.com/nakijun/OpenPDN-1/blob/master/src/Resources/Files/License.txt)

## Paint.NET instalado

A instalação em `C:\Program Files\Paint.NET` expõe as DLLs abaixo como assemblies .NET, todos com versão de arquivo e de assembly `5.200.9810.35277`. O inventário usou metadados e SHA-256 dos arquivos locais. A documentação da série 5.2 registra mudanças no motor de renderização e nos plug-ins de formatos. [Histórico oficial](https://www.paint.net/roadmap.html)

| Arquivo | SHA-256 |
| --- | --- |
| `paintdotnet.dll` | `18dd5b2df9045ee93d9d1b616289dc6549181f3f720a6e304b2f14b4014159ca` |
| `PaintDotNet.Core.dll` | `318dfccda29120438c5edeb9ed6bbe73a7b24e5b339fe10a322224589568d109` |
| `PaintDotNet.Data.dll` | `a929f7f420c3bf146a47152bdf6727baf28e840802fb37f7b17b4fd86a64dbfb` |
| `PaintDotNet.FileTypes.dll` | `db99d6bec46ade8d56e892210f583f3e5b06b4c16740f610ed844009b058cb39` |
| `PaintDotNet.Effects.Gpu.dll` | `44124de7f2c09819f311a5944d156e5cf984638c48a882366b01c341a7514f60` |

A licença da versão moderna restringe adaptações e obras derivadas do software. Para especificar compatibilidade, use a documentação pública e os resultados observados com documentos de teste próprios. A implementação da interface, dos recursos visuais e do código do OpenPaintR segue um desenho próprio. [Licença do Paint.NET](https://www.getpaint.net/license.html)

## Estudo com Ghidra e REA

Ghidra e REA não foram encontrados nos locais e comandos verificados no ambiente inicial. O inventário acima já identifica quais arquivos são assemblies gerenciados. Para uma primeira sessão integrada, use um componente nativo compilado de fonte autorizada, confirme o resultado no Ghidra e registre versão, hash, arquitetura e evidência devolvida pelo REA. Avalie separadamente o suporte a IL gerenciada antes de aplicar o fluxo a um alvo .NET cuja análise esteja autorizada. [Ghidra](https://github.com/NationalSecurityAgency/ghidra) · [REA](https://github.com/Darwin-Liang/REA)

## Experimentos de compatibilidade

| Experimento | Evidência a salvar |
| --- | --- |
| Sobrepor duas cores opacas no modo Normal com opacidade da camada de 0%, 50% e 100%. | Valores RGBA da área sobreposta em PNG exportado, com versão do Paint.NET e valores usados. |
| Sobrepor pixels semitransparentes sobre fundo transparente e alternar a visibilidade da camada superior. | Valores RGBA resultantes, usados para verificar a interação entre alfa, opacidade e visibilidade. |
| Salvar um documento `.pdn` mínimo com duas camadas com nomes diferentes e uma camada oculta. | Arquivo de teste, propriedades visíveis ao reabrir e imagem exportada para comparação. |

As expectativas dos testes atuais foram calculadas pela fórmula *source-over*. A equivalência pixel a pixel com o Paint.NET depende das medições acima. [Especificação de composição](https://www.w3.org/TR/compositing-1/) · [Modos do Paint.NET](https://www.getpaint.net/doc/latest/BlendModes.html)
