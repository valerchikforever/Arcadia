#!/bin/bash
# Builds Arcadia for Windows from Linux: Rust std is built from Ubuntu's rust-1.91-src (build-std),
# linked with MinGW; then the NSIS installer and the portable zip.
# One-time setup: apt install rustc-1.91 cargo-1.91 rust-1.91-src gcc-mingw-w64-x86-64 nsis \
#   libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev zip; npm install
# (+ rsbegin.o/rsend.o in /opt/rtstart, see README "Сборка")
set -e
cd "$(dirname "$0")/src-tauri"
export RUSTC=/usr/bin/rustc-1.91 RUSTC_BOOTSTRAP=1
cargo-1.91 build --release --target x86_64-pc-windows-gnu -Zbuild-std=std,panic_abort
npx tauri bundle --bundles nsis --target x86_64-pc-windows-gnu
V=$(node -p "require('./tauri.conf.json').version")
OUT=../dist; mkdir -p $OUT/portable
cp target/x86_64-pc-windows-gnu/release/bundle/nsis/Arcadia_${V}_x64-setup.exe $OUT/Arcadia-Setup-$V.exe
cp target/x86_64-pc-windows-gnu/release/Arcadia.exe target/x86_64-pc-windows-gnu/release/WebView2Loader.dll $OUT/portable/
printf 'Arcadia portable\r\nЗапустите Arcadia.exe. WebView2Loader.dll должен лежать рядом.\r\nДанные хранятся в %%APPDATA%%\\Arcadia\r\n' > $OUT/portable/README.txt
(cd $OUT/portable && rm -f ../Arcadia-Portable-$V.zip && zip -q -9 ../Arcadia-Portable-$V.zip *)
ls -la $OUT
