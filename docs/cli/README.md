# Command-line interface

`sarena-cli` is the client for managing and inspecting a running Sarena daemon.

Every invocation has the form:

```text
sarena-cli [GLOBAL OPTIONS] <COMMAND> [SUBCOMMAND] [ARGS]
```

## Global options

These are accepted before or after the subcommand (`global = true`). Any value
given here overrides the same setting from the configuration file — see
[config](../config/README.md).

```text
      --config <CONFIG>          Path to a configuration file
  -D, --debug                    Emit debug-level log messages
  -H, --host <HOST>              Daemon address to connect to
  -L, --log-file <LOG_FILE>      Write logs to this file instead of stderr
  -F, --log-format <LOG_FORMAT>  Log format: text | json
  -h, --help                     Print help
```

| Option | Description |
| --- | --- |
| `--config <CONFIG>` | Path to a configuration file to load before the flags are applied. When omitted, `~/.sarena` is read if it exists. See [config](../config/README.md) for the file format and keys. |
| `-D, --debug` | Turn on debug-level log output. Equivalent to `debug = true` in the config file. |
| `-H, --host <HOST>` | Address of the daemon to talk to. Format: `unix://<path>` (e.g. `unix:///tmp/sarena.sock`) or `tcp://<host>:<port>` (e.g. `tcp://127.0.0.1:8080`). When neither this flag nor the config file sets it, the client defaults to `unix:///tmp/sarena.sock`. |
| `-L, --log-file <LOG_FILE>` | Path to a file that log lines are appended to. Without it, logs go to stderr. |
| `-F, --log-format <LOG_FORMAT>` | `text` for one line per event (intended for a terminal) or `json` for structured records (intended for production / log aggregation). Default: `text`. |
| `-h, --help` | Print help (also available per subcommand). |

## Output formatting

Read-only subcommands that list resources accept `-o, --output <FORMAT>`:

| Value | Meaning |
| --- | --- |
| `json` | Pretty-printed JSON. |
| `yaml` | YAML. |
| `jsonpath=<expression>` | Evaluate the JSONPath expression against the JSON result and print the match. |

Without `-o`, these commands print a human-readable table (or plain text).

> Note that `jsonpath=<expression>` is different then the usual Kubernetes jsonpath expressions.
