# payload/

Poné acá **`capyro-windows-x64.zip`** (el paquete de Capyro para Windows) antes de
compilar y queda **adentro** del instalador (instalador *offline*).

Se baja de la release `windows` del repo de Capyro, que arma su CI:

```sh
gh release download windows --repo CapyIsInsano/Capyro --pattern capyro-windows-x64.zip --dir payload
```

o se arma a mano desde el repo de Capyro (ver su README, sección *Windows*):

```sh
cmake --install build-win --prefix dist/Capyro
(cd dist/Capyro && zip -r -9 ../../capyro-windows-x64.zip .)
```

Sin este archivo el instalador es *online*: baja la última versión de GitHub al instalar.
El `.zip` no se sube a git (está en `.gitignore`).
