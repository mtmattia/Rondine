//! Ritaglio del soggetto di una foto, per fare uno sticker (SPEC §7.5): tutto sul telefono, con
//! il modello U²-Net piccolo ("u2netp", Apache-2.0, 4,5 MB) eseguito da tract. Nessun servizio
//! esterno: la foto non lascia il dispositivo. Il modello è nel formato NNEF di tract (convertito
//! una volta dall'ONNX, vedi `android/app/src/main/assets/modelli/LEGGIMI.md`).
//!
//! [`Cutter::mask`] riceve i pixel RGB e restituisce la maschera del soggetto, un byte per pixel
//! (255 = soggetto, 0 = sfondo), della stessa misura della foto.

use tract_nnef::prelude::*;

/// Lato dell'immagine che il modello si aspetta.
const SIDE: usize = 320;
/// Normalizzazione di ImageNet, come in addestramento.
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("modello di ritaglio non valido: {0}")]
    Model(String),
    #[error("immagine non valida: {0}")]
    Image(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Il modello caricato e ottimizzato: si prepara una volta e si riusa.
pub struct Cutter {
    plan: std::sync::Arc<TypedRunnableModel>,
}

impl Cutter {
    /// Carica il modello (U²-Net in NNEF, archivio tar, ingresso 1×3×320×320).
    pub fn from_nnef(bytes: &[u8]) -> Result<Self> {
        let err = |e: TractError| Error::Model(e.to_string());
        let plan = tract_nnef::nnef()
            .model_for_read(&mut std::io::Cursor::new(bytes))
            .map_err(err)?
            .into_optimized()
            .map_err(err)?
            .into_runnable()
            .map_err(err)?;
        Ok(Self { plan })
    }

    /// Maschera del soggetto per una foto RGB (`width × height × 3` byte).
    pub fn mask(&self, rgb: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
        if width == 0 || height == 0 || rgb.len() != width * height * 3 {
            return Err(Error::Image(format!(
                "{} byte per {width}×{height}",
                rgb.len()
            )));
        }
        let small = resize(rgb, width, height, 3, SIDE, SIDE);
        let max = f32::from(small.iter().copied().max().unwrap_or(255).max(1));
        let input: Tensor =
            tract_ndarray::Array4::from_shape_fn((1, 3, SIDE, SIDE), |(_, c, y, x)| {
                (f32::from(small[(y * SIDE + x) * 3 + c]) / max - MEAN[c]) / STD[c]
            })
            .into();
        let out = self
            .plan
            .run(tvec!(input.into()))
            .map_err(|e| Error::Model(e.to_string()))?;
        let first = out
            .first()
            .ok_or_else(|| Error::Model("nessuna uscita".into()))?;
        let pred = first
            .to_plain_array_view::<f32>()
            .map_err(|e| Error::Model(e.to_string()))?;
        let values: Vec<f32> = pred.iter().copied().collect();
        if values.len() != SIDE * SIDE {
            return Err(Error::Model(format!("uscita di {} valori", values.len())));
        }
        let (lo, hi) = values
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        let span = (hi - lo).max(1e-6);
        // Bordi più netti: sotto 0,3 è sfondo, sopra 0,7 soggetto, in mezzo sfumato.
        let small_mask: Vec<u8> = values
            .iter()
            .map(|&v| {
                let n = ((v - lo) / span - 0.3) / 0.4;
                let s = n.clamp(0.0, 1.0);
                (s * s * (3.0 - 2.0 * s) * 255.0).round() as u8
            })
            .collect();
        Ok(resize(&small_mask, SIDE, SIDE, 1, width, height))
    }
}

/// Ridimensionamento bilineare di un'immagine a `channels` byte per pixel.
fn resize(src: &[u8], w: usize, h: usize, channels: usize, nw: usize, nh: usize) -> Vec<u8> {
    let mut out = vec![0u8; nw * nh * channels];
    let sx = w as f32 / nw as f32;
    let sy = h as f32 / nh as f32;
    for y in 0..nh {
        let fy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, (h - 1) as f32);
        let y0 = fy.floor() as usize;
        let y1 = (y0 + 1).min(h - 1);
        let ty = fy - y0 as f32;
        for x in 0..nw {
            let fx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, (w - 1) as f32);
            let x0 = fx.floor() as usize;
            let x1 = (x0 + 1).min(w - 1);
            let tx = fx - x0 as f32;
            for c in 0..channels {
                let p = |xx: usize, yy: usize| f32::from(src[(yy * w + xx) * channels + c]);
                let top = p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx;
                let bottom = p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx;
                out[(y * nw + x) * channels + c] = (top * (1.0 - ty) + bottom * ty).round() as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ridimensiona_mantenendo_i_colori() {
        let src = [10u8, 20, 30].repeat(4 * 4);
        let out = resize(&src, 4, 4, 3, 2, 3);
        assert_eq!(out.len(), 2 * 3 * 3);
        assert!(out.chunks(3).all(|p| p == [10, 20, 30]));
    }
}
