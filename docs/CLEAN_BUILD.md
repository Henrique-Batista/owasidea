# Clean build environments (without Docker)

Tauri's AppImage target runs `linuxdeploy`, which scans `/usr/lib` recursively and
can pick up private vendor libraries installed by the host (for example
`/usr/lib/vmware`). There are two ways to keep those libraries out of the build.
Neither is required to run the generated app, and neither adds Docker as a
framework dependency.

## Option A: bubblewrap (recommended, no root)

Mask the vendor directories for the bundler process only:

```bash
owasidea build
owasidea bundle --bundles appimage --isolate-host-libs
```

The CLI runs `cargo tauri build` inside `bwrap --dev-bind / /` with empty tmpfs
mounts over `/usr/lib/vmware` and `/usr/lib64/vmware`. It prints what it hides:

```text
Hiding /usr/lib/vmware from the bundler
```

- Linux only, requires `bwrap` (`pacman -S bubblewrap`, `apt install bubblewrap`).
- Opt-in; without the flag the bundler runs normally.
- Add other vendor directories with `--mask-path <dir>` (repeatable).
- No root, no chroot, no system modification, no `libcroco`, no patched
  `linuxdeploy`.

See [`APPIMAGE.md`](APPIMAGE.md) for why this is necessary.

## Profile release builds

Release channels use bundle profiles instead of hand-editing `wasm-app.toml`:
`wasm-app.<profile>.toml` is merged over the base config and the artifacts go to
`dist/<profile>`. Build the production channel with the same clean environment:

```bash
owasidea bundle --profile prod --bundles deb,rpm
owasidea bundle --profile prod --bundles appimage --isolate-host-libs
```

The overlay is read inside the chroot like any other project file, so keep it in
the repository and never commit generated `dist/` output. See
[`CLI.md`](CLI.md#bundle-profiles) for the merge rules and what each profile
changes (host, CSP, Tauri identity, runtime descriptor, artifact directory).

## Reproducible SBOMs

`owasidea bundle` writes `sbom.cdx.json` and `sbom.spdx.json` next to the
artifacts (RL-04). For a release you can reproduce, export the commit timestamp
before building:

```bash
export SOURCE_DATE_EPOCH="$(git log -1 --format=%ct)"
owasidea build
owasidea bundle --provenance
owasidea sbom --check
```

The SBOM content and the build id depend only on the lockfiles, the commit and
the epoch, so two runs with the same `SOURCE_DATE_EPOCH` are byte-identical.
The binaries themselves are only as reproducible as Tauri and `linuxdeploy`;
compare `provenance.intoto.json` subjects instead of whole bundles. See
[`RELEASE.md`](RELEASE.md#41-sbom-and-provenance-rl-04).

## Option B: clean chroot (hermetic release)

Use this when you want a build root that contains only official packages, for
example to reproduce a release artifact. On Arch/CachyOS the tool is
`mkarchroot` from `devtools`; `makepkg` alone does **not** create a chroot.

```bash
sudo pacman -S --needed devtools
```

### Create the chroot

```bash
export OWASIDEA_CHROOT="$HOME/.cache/owasidea/chroot"
export OWASIDEA_ROOT="$OWASIDEA_CHROOT/root"

# Remove a previous or broken root first. arch-chroot mounts /proc, /sys,
# /dev and /run inside it, so unmount before deleting.
sudo umount -R "$OWASIDEA_ROOT" 2>/dev/null || true
sudo rm -rf "$OWASIDEA_ROOT"

# mkarchroot creates the directory itself and refuses to run if it already
# exists. Do not pre-create $OWASIDEA_ROOT.

# CachyOS/Arch pacman.conf includes files that mkarchroot does not copy; pass
# every mirrorlist it includes explicitly with -f.
sudo mkarchroot -C /etc/pacman.conf \
  -f /etc/pacman.d/mirrorlist \
  -f /etc/pacman.d/cachyos-mirrorlist \
  -f /etc/pacman.d/cachyos-v4-mirrorlist \
  "$OWASIDEA_ROOT" base-devel
```

Check which includes your `pacman.conf` uses and pass all of them:

```bash
grep '^Include' /etc/pacman.conf
```

After `mkarchroot` succeeds, make the root a mountpoint so `arch-chroot` stops
warning and `pacman`/`findmnt` see an accurate hierarchy:

```bash
sudo mount --bind "$OWASIDEA_ROOT" "$OWASIDEA_ROOT"
```

### Install the Tauri toolchain

WASI plugins and the UI are built on the host first, so Go, .NET, wasi-sdk and
Node do not need to be installed inside the chroot. The chroot only needs Rust
and the GTK/WebKit stack:

```bash
sudo arch-chroot "$OWASIDEA_ROOT" pacman -Syu --needed --noconfirm \
  rust nodejs npm webkit2gtk-4.1 gtk3 libsoup3 librsvg \
  patchelf file openssl pkgconf git

# Match the host UID so files in the bind-mounted checkout stay writable.
sudo arch-chroot "$OWASIDEA_ROOT" \
  useradd -m -u "$(id -u)" builder || true

# The rust package installs cargo in /usr/bin. Pass HOME explicitly: with
# `arch-chroot -u builder` the login shell would still try /root/.bash_profile.
sudo arch-chroot -u builder "$OWASIDEA_ROOT" \
  env HOME=/home/builder PATH=/usr/bin:/usr/local/bin \
  bash -c 'cargo install tauri-cli --version "^2" --locked'
```

### Build the AppImage

Build the WASI components, manifests, signing keys and UI on the host, from the
repository root:

```bash
export OWASIDEA_REPO="$PWD"
owasidea --config examples/shipyard/wasm-app.toml build
```

Bind the checkout into the chroot. Mount the repository root, not only the
example, because `examples/shipyard/desktop` depends on `crates/host` by
relative path:

```bash
sudo mkdir -p "$OWASIDEA_ROOT/build"
sudo mount --bind "$OWASIDEA_REPO" "$OWASIDEA_ROOT/build"

sudo arch-chroot -u builder "$OWASIDEA_ROOT" \
  env HOME=/home/builder PATH=/home/builder/.cargo/bin:/usr/bin:/usr/local/bin \
  bash -c '
    set -e
    cd /build
    cargo build --release -p owasidea-cli
    cd examples/shipyard
    export PATH=/build/target/release:$PATH
    export CARGO_TARGET_DIR=/build/target/clean
    owasidea bundle --no-build --bundles appimage
  '
```

The CLI signs manifests, rebuilds the UI and copies the artifact to
`examples/shipyard/dist/`. Because `CARGO_TARGET_DIR` is inside the mounted
repository, the AppImage is visible on the host:

```text
examples/shipyard/dist/Shipyard_0.1.0_amd64.AppImage
```

Unmount when done:

```bash
sudo umount "$OWASIDEA_ROOT/build"
sudo umount -R "$OWASIDEA_ROOT"
```

Inside the chroot there is no `/usr/lib/vmware`, so linuxdeploy only sees the
official gdk-pixbuf, which does not need `libcroco`.

### Troubleshooting the chroot

| Error | Cause | Fix |
| --- | --- | --- |
| `Working directory '...' already exists` | `mkarchroot` creates the root itself | `sudo rm -rf "$OWASIDEA_ROOT"` and rerun without `mkdir` |
| `config file /etc/pacman.d/cachyos-*-mirrorlist could not be read` | `mkarchroot` copied `pacman.conf` but not its includes | pass each file with `mkarchroot -f` |
| `... is not a mountpoint` | warning only | `sudo mount --bind "$OWASIDEA_ROOT" "$OWASIDEA_ROOT"` after creation |
| `mount: .../root/proc: mount point does not exist` | the chroot is empty because `mkarchroot` never ran | recreate the root; do not run `arch-chroot` before `mkarchroot` finishes |
| `/root/.bash_profile: Permission denied` | `HOME` is still `/root` with `-u builder` | `env HOME=/home/builder PATH=... bash -c '...'` (no `-l`) |
| `cargo: command not found` | `rust` not installed, or PATH lacks `/usr/bin` | install `rust` and pass an explicit `PATH` |

## Debian/Ubuntu

The equivalent tools are `debootstrap`, `sbuild` and `pbuilder`/`cowbuilder`.
For an arbitrary Tauri build, a `debootstrap` root with `chroot` or
`systemd-nspawn` is the simplest:

```bash
sudo apt-get install -y debootstrap
sudo debootstrap --variant=buildd stable /tmp/owasidea-chroot http://deb.debian.org/debian
```

Install Rust, WebKitGTK 4.1, GTK3, libsoup3, `patchelf`, `file` and the Tauri
CLI inside the root, bind-mount the checkout, and run the same
`owasidea bundle --no-build --bundles appimage`.

## Notes

- This is a **release build** technique. Users of the generated app need no
  chroot, Docker, `devtools`, `bwrap` or `libcroco`.
- The chroot is reusable; update it with `pacman -Syu`, or rebuild it when the
  toolchain changes.
- Use `target/clean` for chroot builds so the host's normal target directory is
  not mixed with the clean one.
