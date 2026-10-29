# AppImage packaging

## Why private host libraries leak into the build

Tauri's Linux AppImage target uses `linuxdeploy` plus
`linuxdeploy-plugin-gtk.sh`. The plugin asks `pkg-config` for the library
directories and then runs **recursive** searches, for example:

```bash
gdk_libdir="$(pkg-config --variable=libdir gdk-pixbuf-2.0)" # /usr/lib
find "$gdk_libdir" \( -type l -o -type f \) -name "libgdk_pixbuf-*.so*"
```

`find /usr/lib` also descends into vendor-specific directories. On hosts with
VMware Workstation installed, it finds:

```text
/usr/lib/vmware/lib/libgdk_pixbuf-2.0.so.0/libgdk_pixbuf-2.0.so.0
```

That is an old private copy of gdk-pixbuf. Its `DT_NEEDED` includes
`libcroco-0.6.so.3`, which lives in another private VMware directory that is not
in `/etc/ld.so.conf` or the `ldconfig` cache. `linuxdeploy` then fails with:

```text
failed to run linuxdeploy
```

The app does not need `libcroco`, and the system gdk-pixbuf does not need it
either. The failure is caused by `linuxdeploy` being handed the wrong vendor
copy.

Tauri does not currently expose an option to exclude a directory from the GTK
plugin's search. Alternatives such as `appimagetool` only package an AppDir;
they do not remove the dependency-deployment problem. The framework therefore
hides vendor directories from the bundler process instead of modifying the host.

## Local builds on contaminated hosts

Use `--isolate-host-libs`. The CLI runs `cargo tauri build` inside
[bubblewrap](https://github.com/containers/bubblewrap) with empty tmpfs mounts
over `/usr/lib/vmware` and `/usr/lib64/vmware`. Full host access is preserved;
only those directories are masked. No root, no chroot, no Docker, no system
change.

```bash
owasidea bundle --bundles appimage --isolate-host-libs
```

The CLI prints the directories it hides:

```text
Hiding /usr/lib/vmware from the bundler
```

Requirements and limits:

- Linux only; `bwrap` must be installed (`pacman -S bubblewrap`,
  `apt install bubblewrap`).
- Opt-in. Without the flag the bundler runs normally.
- Other vendor directories can be hidden with `--mask-path <dir>` (repeatable).
- If `bwrap` is missing or user namespaces are disabled, the command fails with
  a clear message; use the chroot recipe in
  [`CLEAN_BUILD.md`](CLEAN_BUILD.md) instead.

This produces an AppImage that bundles the system gdk-pixbuf (the variant that
does not need `libcroco`). Do not "fix" the error by installing `libcroco` or by
adding VMware's private library directory to the global linker path: that would
package the wrong vendor library.

If you do not need AppImage locally, use the native packages:

```bash
owasidea bundle --bundles deb,rpm
```

They use Tauri's deb/rpm writers and do not invoke the GTK linuxdeploy plugin.

## Hermetic release builds

For a reproducible release, build AppImage on a clean CI runner (Ubuntu
22.04/24.04) or in a clean package-manager chroot; both are documented in
[`CLEAN_BUILD.md`](CLEAN_BUILD.md). End users of the generated app need none of
this.

```bash
owasidea build
owasidea bundle --no-build --bundles appimage
```

GitHub Actions example for a clean runner:

```yaml
name: appimage
on:
  workflow_dispatch:
  push:
    tags: ["v*"]
jobs:
  appimage:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-wasip1,wasm32-wasip2
      - uses: actions/setup-node@v4
        with: { node-version: 22 }
      - uses: actions/setup-go@v5
        with: { go-version: "1.27.1" }
      - uses: actions/setup-dotnet@v4
        with: { dotnet-version: "10.0.x" }
      - run: |
          sudo apt-get update
          sudo apt-get install -y build-essential pkg-config libssl-dev \
            libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev \
            libsoup-3.0-dev libgtk-3-dev libayatana-appindicator3-dev \
            librsvg2-dev libxdo-dev patchelf file
      - run: dotnet workload install wasi-experimental
      - run: cargo install cargo-component wasm-tools tauri-cli --locked
      - run: cargo install --path crates/cli --force
      - run: owasidea doctor --install-wasi-sdk
      - run: cd examples/shipyard && owasidea build
      - run: cd examples/shipyard && owasidea bundle --no-build --bundles appimage
      - run: cd examples/shipyard && owasidea verify dist/
      - uses: actions/upload-artifact@v4
        with:
          name: shipyard-appimage
          path: examples/shipyard/dist/*.AppImage
```

`bundle` stages the manifests, components and UI assets into `dist/`, so the
verification step checks the payload that ships with the AppImage. The repository CI uses `--isolate-host-libs` on the runner, installs
`bubblewrap`, and skips the AppImage job with a message when `bwrap` cannot
create a user namespace (see `.github/workflows/ci.yml`).

Other Linux distributions and versions of WebKit/GTK may produce different
AppImages; the Ubuntu runner makes the result reproducible.
