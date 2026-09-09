# Configuration

`sarena-cli` builds its effective configuration from three layers. Later layers
override earlier ones:

1. **Configuration file** — `--config <path>` if given, otherwise `~/.sarena`
   when that file exists (a missing `~/.sarena` is not an error).
2. **Environment variables** — `SARENA_*` (see below).
3. **Command-line flags** — the [global options](../cli/README.md#global-options)
   (`--host`, `--debug`, `--log-file`, `--log-format`). A flag only overrides
   when it is actually passed.

## File format

The file is parsed by the [`config`](https://crates.io/crates/config) crate. The
format is inferred from the file extension, so a `--config` path should end in
`.toml`, `.yaml`/`.yml`, `.json`, etc. The extension-less `~/.sarena` is read by
trying the supported formats in turn — using an explicit extension avoids the
ambiguity.

TOML example (`~/.sarena` or `--config sarena.toml`):

```toml
# Daemon address: "unix://<path>" or "tcp://<host>:<port>".
host = "unix:///tmp/sarena.sock"

# Emit debug-level log messages.
debug = false

# Write logs to this file instead of stderr.
log_file = "/var/log/sarena-cli.log"

# Log format: "text" or "json".
log_format = "text"

# BPF filesystem directory holding the datapath's pinned maps
# (used by the `bpf` subcommands).
pin_root = "/sys/fs/bpf/sarena"
```

## Keys

| Key | Type | Default | Overriding flag | Description |
| --- | --- | --- | --- | --- |
| `host` | string | `unix:///tmp/sarena.sock` | `-H, --host` | Address of the daemon to connect to. `unix://<path>` for a Unix domain socket, `tcp://<host>:<port>` for TCP. |
| `debug` | bool | `false` | `-D, --debug` | Emit debug-level log messages. |
| `log_file` | string | *(unset — log to stderr)* | `-L, --log-file` | Path a log file to append log lines to. |
| `log_format` | string | `text` | `-F, --log-format` | `text` (one line per event, for a terminal) or `json` (structured, for log aggregation). |
| `pin_root` | path | `/sys/fs/bpf/sarena` | *(none)* | BPF filesystem directory holding the datapath's pinned maps. Only the `bpf` subcommands use it, and it can only be set from the config file. |

## Environment variables

Variables are read with the `SARENA_` prefix, so for example `SARENA_HOST` maps to `host`
and `SARENA_DEBUG` maps to `debug`:

```console
$ SARENA_HOST=tcp://127.0.0.1:8080 SARENA_DEBUG=true sarena-cli service ls
```

Keys whose names contain an underscore (`log_file`, `log_format`, `pin_root`)
are not currently reachable through the environment — set those in the config
file or, where available, via the command-line flag.
