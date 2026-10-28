#!/usr/bin/env bash
set -e

echo "🔨 [1/4] Kompilacja Linux Game Hub w trybie --release..."
cargo build --release

echo "📁 [2/4] Przygotowywanie struktury AppDir..."
rm -rf AppDir
mkdir -p AppDir/usr/bin

cp target/release/Fiksiarski_game_hub AppDir/usr/bin/linux_game_hub

cat << 'APPRUN' > AppDir/AppRun
#!/usr/bin/env bash
HERE="$(dirname "$(readlink -f "${0}")")"
exec "${HERE}/usr/bin/linux_game_hub" "$@"
APPRUN
chmod +x AppDir/AppRun

cat << 'DESKTOP' > AppDir/linux_game_hub.desktop
[Desktop Entry]
Name=Fiksiarski Game Hub
Comment=Centrum gier karcianych i planszowych
Exec=linux_game_hub
Icon=linux_game_hub
Type=Application
Categories=Game;BoardGame;CardGame;
Terminal=false
DESKTOP

cat << 'ICON' > AppDir/linux_game_hub.svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128">
  <rect width="128" height="128" rx="26" fill="#1e1e2e"/>
  <rect x="14" y="14" width="100" height="100" rx="18" fill="#313244" stroke="#89b4fa" stroke-width="4"/>
  <text x="64" y="82" font-size="64" text-anchor="middle" fill="#cdd6f4">🎮</text>
</svg>
ICON

if [ ! -f appimagetool-x86_64.AppImage ]; then
    echo "🌐 [3/4] Pobieranie narzędzia appimagetool..."
    curl -L -o appimagetool-x86_64.AppImage https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
    chmod +x appimagetool-x86_64.AppImage
fi

echo "📦 [4/4] Pakowanie do FiksiarskiGameHub-x86_64.AppImage..."
ARCH=x86_64 ./appimagetool-x86_64.AppImage --appimage-extract-and-run AppDir FiksiarskiGameHub-x86_64.AppImage

echo "✅ GOTOWE! Twój plik to: ./FiksiarskiGameHub-x86_64.AppImage"
