#!/usr/bin/env bash
# Pubblica una versione dal PC (docs/PUBBLICAZIONE.md, "Pubblicare dal PC"): compila e firma gli
# APK con la chiave del progetto, crea il tag e la Release su GitHub con le note della versione.
# Stessi file e stessi nomi della pubblicazione automatica (.github/workflows/apk.yml), che
# trovando la Release già completa si ferma.
#
#   scripts/pubblica.sh          APK foss e play (arm64)
#   scripts/pubblica.sh --aab    anche l'AAB per Google Play (arm64 e x86_64)
#   scripts/pubblica.sh --prova  compila e controlla la firma, senza pubblicare niente
set -euo pipefail

cd "$(dirname "$0")/.."
aab=false
prova=false
for a in "$@"; do
  case "$a" in
    --aab) aab=true ;;
    --prova) prova=true ;;
    *) echo "Uso: scripts/pubblica.sh [--aab] [--prova]" >&2; exit 2 ;;
  esac
done

# Impronta SHA-256 del certificato della chiave unica (docs/PUBBLICAZIONE.md).
FIRMA="3a189eb837f97a5fecd90e5277bd80065cf33e920b66e254de339919ee622fa6"

errore() { echo "✗ $*" >&2; exit 1; }

gradle_file=android/app/build.gradle.kts
name=$(grep -oP 'versionName = "\K[^"]+' "$gradle_file")
code=$(grep -oP 'versionCode = \K\d+' "$gradle_file")
tag="v$name"
news="fastlane/metadata/android/it-IT/changelogs/${code}.txt"

# 1. Si pubblica solo ciò che è su GitHub: main pulito e allineato, versione nuova.
if ! $prova; then
  [ -z "$(git status --porcelain)" ] || errore "ci sono modifiche non salvate: fai prima il commit"
  [ "$(git branch --show-current)" = main ] || errore "si pubblica da main"
  git fetch -q origin main --tags
  [ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] \
    || errore "main non è allineato a GitHub: fai push (o pull) prima"
  if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
    errore "il tag $tag esiste già: alza versionCode e versionName in $gradle_file"
  fi
  if gh release view "$tag" >/dev/null 2>&1; then
    errore "la Release $tag esiste già su GitHub"
  fi
  [ -f "$news" ] || errore "mancano le novità della versione: $news (e la versione en-US)"
fi

echo "→ Rondine $tag (versionCode $code)"

# 2. Compilazione firmata: chiave e password da ~/.gradle/gradle.properties (rondine.firma.*).
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
(
  cd android
  ./gradlew -q -Prondine.abis=arm64-v8a assembleFossRelease assemblePlayRelease
)
cp android/app/build/outputs/apk/foss/release/app-foss-release.apk "$out/rondine-${tag}.apk"
cp android/app/build/outputs/apk/play/release/app-play-release.apk "$out/rondine-${tag}-play.apk"
files=("$out/rondine-${tag}.apk" "$out/rondine-${tag}-play.apk")
if $aab; then
  (cd android && ./gradlew -q -Prondine.negozio=true bundlePlayRelease)
  cp android/app/build/outputs/bundle/playRelease/app-play-release.aab "$out/rondine-${tag}-play.aab"
  files+=("$out/rondine-${tag}-play.aab")
fi

# 3. La firma deve essere quella del progetto, se no gli aggiornamenti non si installano.
sdk="${ANDROID_HOME:-$HOME/Android/Sdk}"
apksigner=$(ls "$sdk"/build-tools/*/apksigner | sort -V | tail -1)
for apk in "$out"/*.apk; do
  got=$("$apksigner" verify --print-certs "$apk" | grep -oP 'certificate SHA-256 digest: \K\S+' | head -1)
  [ "$got" = "$FIRMA" ] || errore "$(basename "$apk") non ha la firma del progetto ($got)"
done
echo "✓ firma verificata"
if $prova; then
  ls -l "$out"
  echo "Prova finita: niente pubblicato."
  exit 0
fi

# 4. Note: prima le novità (le mostra anche l'app), poi lo stesso testo della pubblicazione
#    automatica.
notes="$out/note.md"
{
  cat "$news"
  echo
  echo "Rondine ${tag}. Due versioni, stessa firma: si installano una sopra l'altra senza perdere niente. rondine-${tag}.apk è la versione libera (come su F-Droid): notifiche con ntfy. rondine-${tag}-play.apk è la versione Google: se manca ntfy, notifiche attraverso Google (FCM)."
  if $aab; then echo "L'AAB è per Google Play."; fi
} > "$notes"

# 5. Release (crea anche il tag sul commit di main). Le versioni 0.x sono anteprime.
pre=()
case "$tag" in v0.*) pre=(--prerelease) ;; esac
gh release create "$tag" "${files[@]}" "${pre[@]}" \
  --target "$(git rev-parse HEAD)" --title "Rondine ${tag}" --notes-file "$notes"
git fetch -q origin --tags
echo "✓ pubblicata: $(gh release view "$tag" --json url --jq .url)"
