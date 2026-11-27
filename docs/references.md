# OpenPaintR Technical References

## OpenPDN-1 3.36.7

This repository preserves the historical Paint.NET 3.36.7 source. The table lists the files consulted and how they informed the first prototype. [Repository](https://github.com/nakijun/OpenPDN-1)

| Source | Observation | Application in OpenPaintR |
| --- | --- | --- |
| [Document.cs](https://github.com/nakijun/OpenPDN-1/blob/master/src/Data/Document.cs) | A document has dimensions and a collection of layers. | `Document` manages dimensions and layer order. |
| [Layer.cs](https://github.com/nakijun/OpenPDN-1/blob/master/src/Data/Layer.cs) | A layer has fixed dimensions and display properties. | Each layer stores canvas-sized pixels and exposes its name, visibility, and opacity in the core. |
| [BitmapLayer.cs](https://github.com/nakijun/OpenPDN-1/blob/master/src/Data/BitmapLayer.cs) | A bitmap layer owns a raster surface and applies opacity during compositing. | The Rust implementation combines pixel alpha and layer opacity using normal *source-over* compositing. |

The historical license uses the MIT text with exceptions for the logo, icons, other assets, and GPC. The prototype uses its own implementation and code-generated graphics. If you incorporate a historical excerpt, record its source file and revision, verify the applicable terms, and preserve required notices. [OpenPDN-1 license](https://github.com/nakijun/OpenPDN-1/blob/master/src/Resources/Files/License.txt)

## Installed Paint.NET build

The installation at `C:\Program Files\Paint.NET` exposes the following DLLs as .NET assemblies, all with file and assembly version `5.200.9810.35277`. The inventory used metadata and SHA-256 hashes from the local files. The 5.2 changelog records changes to the rendering engine and file format plugins. [Official changelog](https://www.paint.net/roadmap.html)

| File | SHA-256 |
| --- | --- |
| `paintdotnet.dll` | `18dd5b2df9045ee93d9d1b616289dc6549181f3f720a6e304b2f14b4014159ca` |
| `PaintDotNet.Core.dll` | `318dfccda29120438c5edeb9ed6bbe73a7b24e5b339fe10a322224589568d109` |
| `PaintDotNet.Data.dll` | `a929f7f420c3bf146a47152bdf6727baf28e840802fb37f7b17b4fd86a64dbfb` |
| `PaintDotNet.FileTypes.dll` | `db99d6bec46ade8d56e892210f583f3e5b06b4c16740f610ed844009b058cb39` |
| `PaintDotNet.Effects.Gpu.dll` | `44124de7f2c09819f311a5944d156e5cf984638c48a882366b01c341a7514f60` |

The modern license restricts adaptations and derivative works. Use public documentation and observations from our own test documents to specify compatibility. OpenPaintR has its own interface, visual assets, and implementation. [Paint.NET license](https://www.getpaint.net/license.html)

## Compatibility experiments

| Experiment | Evidence to retain |
| --- | --- |
| Overlay two opaque colors in Normal mode at 0%, 50%, and 100% layer opacity. | RGBA values of the overlap in the exported PNG, with the Paint.NET version and settings. |
| Overlay semitransparent pixels on a transparent background and toggle the top layer's visibility. | Resulting RGBA values to verify the interaction between alpha, opacity, and visibility. |
| Save a minimal `.pdn` document with two differently named layers, one hidden. | Test file, visible properties after reopening, and an exported image for comparison. |

Current test expectations come from the *source-over* formula. Use the measurements above to establish pixel-for-pixel equivalence with Paint.NET. [Compositing specification](https://www.w3.org/TR/compositing-1/) · [Paint.NET blend modes](https://www.getpaint.net/doc/latest/BlendModes.html)
