<p align="center">
  <img src="resources/icons/hicolor/scalable/apps/com.github.exepta.cosmic-task-monitor.svg" alt="Cosmic Task Monitor icon" width="128" />
</p>
<h1 align="center">Cosmic Task Monitor</h1>

> [!IMPORTANT]
> By using this software, you fully accept all responsibility for any data loss or corruption caused whilst using this software.

> [!WARNING]
> This software is currently in early beta, and has not been tested against many drive type, partition type, and partition scheme combinations yet.
---

Monitor running applications and system performance.

## Use Without IDE

### Quick run (terminal only)

```sh
cargo build --release
./target/release/cosmic-task-monitor
```

### Install for current user (desktop launcher + icon)

```sh
./scripts/install-user.sh
```

This installs to `~/.local`:
- binary: `~/.local/bin/cosmic-task-monitor`
- desktop entry: `~/.local/share/applications/com.github.exepta.cosmic-task-monitor.desktop`
- icon: `~/.local/share/icons/hicolor/scalable/apps/com.github.exepta.cosmic-task-monitor.svg`

Alternative with `just`:

```sh
just install-user
```

### System-wide install

```sh
just install
```

## Development Commands

A [justfile](./justfile) is included for the [casey/just][just] runner.

- `just` builds the app (`build-release`)
- `just run` builds and runs the app
- `just install` installs the project system-wide
- `just install-user` installs into `~/.local`
- `just vendor` creates a vendored tarball
- `just build-vendored` compiles with vendored dependencies
- `just check` runs `clippy`
- `just check-json` emits JSON diagnostics for IDE/LSP

## Translators

[Fluent][fluent] is used for localization. Translation files are in [i18n](./i18n).

## Packaging

For Linux distribution packaging:

```sh
just vendor
just build-vendored
just rootdir=debian/cosmic-task-monitor prefix=/usr install
```

## License

This project is licensed under [MPL-2.0](./LICENSE.md).

[fluent]: https://projectfluent.org/
[just]: https://github.com/casey/just
