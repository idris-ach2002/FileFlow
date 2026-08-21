# FileFlow visual identity

- `src-tauri/icons/icon-master.png`: official static launcher icon source.
- `src/assets/branding/fileflow-mark.svg`: static in-app mark.
- `src/assets/branding/fileflow-mark-animated.svg`: infinite in-app/splash animation.
- `src-tauri/branding/tray.png`: monochrome tray/menu-bar icon.
- `src-tauri/branding/tray-frames/`: 8 optional activity frames.

The OS launcher icon stays static; animation belongs inside the app/splash/tray activity state.
The animated SVG respects `prefers-reduced-motion`.
