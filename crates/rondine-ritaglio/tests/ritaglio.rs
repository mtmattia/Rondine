//! Il modello vero (quello dentro l'app) su un'immagine sintetica: un disco rosso al centro di
//! uno sfondo grigio a macchie. Il soggetto è il disco.

use rondine_ritaglio::Cutter;

#[test]
fn trova_il_soggetto_al_centro() -> Result<(), Box<dyn std::error::Error>> {
    let model = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../android/app/src/main/assets/modelli/u2netp.nnef.tar");
    let cutter = Cutter::from_nnef(&std::fs::read(model)?)?;
    let (w, h) = (400usize, 300usize);
    let mut rgb = Vec::with_capacity(w * h * 3);
    let mut seed = 7u32;
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f32 - 200.0, y as f32 - 150.0);
            if dx * dx + dy * dy < 80.0 * 80.0 {
                rgb.extend_from_slice(&[220, 30, 40]);
            } else {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let g = 110 + (seed >> 16) as u8 % 30;
                rgb.extend_from_slice(&[g, g, g]);
            }
        }
    }
    let mask = cutter.mask(&rgb, w, h)?;
    assert_eq!(mask.len(), w * h);
    let at = |x: usize, y: usize| mask[y * w + x];
    assert!(at(200, 150) > 200, "centro: {}", at(200, 150));
    for (x, y) in [(5, 5), (394, 5), (5, 294), (394, 294)] {
        assert!(at(x, y) < 50, "angolo {x},{y}: {}", at(x, y));
    }
    assert!(cutter.mask(&rgb[..10], w, h).is_err());
    Ok(())
}
