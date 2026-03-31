# Barlo

Barlo ist ein macOS Menu Bar Manager — ähnlich wie Bartender oder Ice. Er ermöglicht es, Icons in der macOS Menüleiste auszublenden und in einer eigenen "Barlo Bar" zu verwalten.

## Features

- **Dots-Icon** (`⋯`) in der Menüleiste — Klick blendet Icons links davon aus/ein
- **Overlay** überdeckt die versteckten Icons mit einem Wallpaper-Screenshot, damit die Lücke nicht sichtbar ist
- **Barlo Bar** — ein transparentes Floating-Fenster das die ausgeblendeten Icons als klickbare Buttons anzeigt
- Klick auf ein Icon in der Barlo Bar sendet den Klick an die Original-Position in der Menüleiste
- **Tray-Menü** zum Ein-/Ausblenden der Barlo Bar und zum Öffnen der Settings

## Voraussetzungen

- [Node.js](https://nodejs.org/) (v18+)
- [Rust](https://rustup.rs/)
- Xcode Command Line Tools (`xcode-select --install`)
- macOS 13+

## Entwicklung

```bash
npm install
npm run tauri dev
```

Beim ersten Start fragt macOS nach der **Screen Recording** Permission — diese muss erteilt werden damit das Overlay funktioniert.

## Build

```bash
npm run tauri build
```

## Tech Stack

- [Tauri v2](https://tauri.app/) (Rust Backend)
- [React](https://react.dev/) + TypeScript (Frontend)
- [Vite](https://vitejs.dev/) (Build Tool)
