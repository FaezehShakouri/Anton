# App icons

Tauri 2 expects the icon set referenced from `tauri.conf.json`
(`bundle.icon`). The committed `32x32.png`, `128x128.png`, and
`128x128@2x.png` here are minimal **placeholder** transparent PNGs so
`cargo check` and `tauri dev` work out of the box on a fresh clone.

When the real brand artwork is ready, regenerate the full icon set with:

```bash
pnpm tauri icon path/to/source-icon.png
```

That command produces `32x32.png`, `128x128.png`, `128x128@2x.png`,
`icon.icns`, and `icon.ico` here. After regenerating, add the
platform-specific entries (`icon.icns`, `icon.ico`, etc.) back to the
`bundle.icon` array in `tauri.conf.json`.
