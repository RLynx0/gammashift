# gammashift

`gammashift` is a small daemon and command-line client for smoothly changing
display temperature, brightness, and gamma through
[`wl-gammarelay-rs`](https://github.com/Arctic-Hen7/wl-gammarelay-rs).

## Requirements

- Linux with a session D-Bus
- `wl-gammarelay-rs` running and exposing `rs.wl-gammarelay`
- `XDG_RUNTIME_DIR` set for the user's session

The daemon listens on `$XDG_RUNTIME_DIR/gammashift/gammashift.sock`. Start it
in a terminal with `gammashift start`, or run it as a systemd user service:

```ini
[Unit]
Description=Gammashift display transition service
Requires=wl-gammarelay.service
After=wl-gammarelay.service
PartOf=wl-gammarelay.service

[Service]
Type=simple
ExecStart=/path/to/gammashift start
Restart=on-failure
RuntimeDirectory=gammashift

[Install]
WantedBy=default.target
```

Replace `/path/to/gammashift` with the executable's path and adjust the backend
unit name if needed.

## Usage

```sh
gammashift shift temperature 4200
gammashift shift brightness +0.05 -t 0.1
gammashift shift gamma '*1.2' --time 1.5
gammashift shift brightness default
gammashift stop
```

Properties (`temperature`, `brightness`, `gamma`) are case-insensitive.
Targets may be absolute numbers, relative changes (`+0.05`, `-250`), linear
multipliers (`'*0.3'`), `default`, or `undo`. Quote multiplication targets so
the shell does not expand `*`.

The `-t`/`--time` duration defaults to two seconds. It accepts seconds by
default, or the suffixes `ms`, `s`, and `m`; for example `-t 0.3` or
`--time 200ms`. A duration of `0` applies the value immediately.

Undo history is in memory and scoped to both the property and the `--id` value.
The default id is empty; assign an id to a sequence of shifts that should have
independent undo history. For example, an idle-dimming client can use the same
id for dim and restore, without a manual brightness adjustment with the
default id replacing that history:

```sh
gammashift shift brightness '*0.3' -t 1.5 --id idle-dim
gammashift shift gamma '*1.2' -t 1.5 --id idle-dim

gammashift shift brightness undo -t 0.3 --id idle-dim
gammashift shift gamma undo -t 0.3 --id idle-dim
```

Restarting the daemon clears undo history. `gammashift stop` asks a running
daemon to stop; subsequent client commands report that the engine is
unavailable and suggest starting it.

## Build

```sh
nix build
nix develop --command cargo test --offline
```

The default package exposes the `gammashift` executable.
