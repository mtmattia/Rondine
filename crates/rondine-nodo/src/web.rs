//! Pagine per i link condivisi (SPEC §3.2, §4.1, §11): aprono l'app con il link `rondine:`.
//! Non salvano niente e non ricevono il biglietto, che nell'invito sta dopo `#`.

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use rondine_core::username::normalize_name;

use crate::AppState;

/// `GET /@{nome}`: il nome è normalizzato (solo `a-z 0-9 . _`), quindi sicuro dentro l'HTML.
pub async fn name_page(State(s): State<AppState>, Path(at_name): Path<String>) -> Response {
    let Some(name) = at_name
        .strip_prefix('@')
        .and_then(|n| normalize_name(n).ok())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let link = format!("rondine:@{name}@{}", s.0.host);
    page(
        &format!("@{name} ti invita a scrivere su Rondine."),
        &link,
        "",
    )
}

/// `GET /invito`: pagina fissa; lo script legge il biglietto dal frammento `#…`.
pub async fn invite_page() -> Response {
    let script = r#"<script>
var h = location.hash.slice(1);
var a = document.getElementById("apri");
if (/^[A-Za-z0-9_-]+$/.test(h)) { a.href = "rondine:invito/" + h; location.href = a.href; }
else { a.remove(); document.getElementById("testo").textContent = "Questo invito è incompleto: chiedi di mandartelo di nuovo."; }
</script>"#;
    page("Ti hanno invitato a scrivere su Rondine.", "#", script)
}

fn page(text: &str, link: &str, script: &str) -> Response {
    let html = format!(
        r#"<!doctype html>
<html lang="it"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Rondine</title>
<style>
body{{margin:0;font-family:system-ui,sans-serif;background:#f7f4e4;color:#111b17;display:flex;min-height:100vh;align-items:center;justify-content:center}}
main{{max-width:28rem;padding:2rem;text-align:center}}
h1{{color:#0d4f3c}}
.btn{{display:inline-block;margin:1rem 0;padding:.9rem 2rem;border-radius:2rem;background:#0d4f3c;color:#fff;text-decoration:none;font-weight:600}}
small{{color:#55655d}}
</style></head>
<body><main>
<h1>Rondine</h1>
<p id="testo">{text}</p>
<a class="btn" id="apri" href="{link}">Apri in Rondine</a>
<p><small>Messaggi cifrati da telefono a telefono. Non hai ancora Rondine?
<a href="https://github.com/mtmattia/Rondine">Scaricala</a>.</small></p>
</main>{script}</body></html>
"#
    );
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}
