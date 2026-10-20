# Modelli

`u2netp.nnef.tar`: U²-Net nella versione piccola, per ritagliare il soggetto delle foto quando si crea
uno sticker (crate `rondine-ritaglio`). Pesi di Xuebin Qin et al.,
[github.com/xuebinqin/U-2-Net](https://github.com/xuebinqin/U-2-Net), licenza Apache-2.0
(`LICENSE-U-2-Net.txt`); conversione ONNX dal progetto rembg (MIT).

Come è stato fatto, da `u2netp.onnx` (SHA-256
`309c8469258dda742793dce0ebea8e6dd393174f89934733ecc8b14c76f4ddd8`, release v0.0.0 di rembg):

1. nei nodi `Resize`, `coordinate_transformation_mode` da `pytorch_half_pixel` a `half_pixel`, che
   tract conosce (stesso risultato quando l'uscita è più grande di 1 pixel, come qui), con il
   pacchetto Python `onnx`;
2. conversione nel formato NNEF di tract (tract-onnx e tract-nnef 0.23): ingresso fissato a
   1×3×320×320, `into_typed()`, `into_decluttered()`, `write_to_tar`. Così l'app non porta il
   lettore ONNX.
