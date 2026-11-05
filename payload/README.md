# payload/

Poné acá el paquete de Capyro antes de compilar y queda **adentro** del instalador
(instalador *offline*):

- **`capyro-windows-x64.zip`**: Windows.
- **`capyro-macos-arm64.zip`** / **`capyro-macos-x64.zip`**: la Mac con Apple Silicon / con
  Intel (cada arquitectura del instalador lleva el suyo; con `CAPYRO_PAYLOAD=...` se elige otro).

Se bajan de las releases `windows` y `macos` del repo de Capyro, que arma su CI:

```sh
gh release download windows --repo CapyIsInsano/Capyro --pattern capyro-windows-x64.zip --dir payload
gh release download macos --repo CapyIsInsano/Capyro --pattern 'capyro-macos-*.zip' --dir payload
```

o se arma a mano desde el repo de Capyro (ver su README, sección *Windows*):

```sh
cmake --install build-win --prefix dist/Capyro
(cd dist/Capyro && zip -r -9 ../../capyro-windows-x64.zip .)
```

Sin el archivo el instalador es *online*: baja la última versión de GitHub al instalar.
El `.zip` no se sube a git (está en `.gitignore`).
