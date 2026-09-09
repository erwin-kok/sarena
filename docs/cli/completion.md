# `completion` — shell completion

```text
sarena-cli completion <SHELL>
```

Print a completion script for `<SHELL>` (one of `bash`, `zsh`, `fish`) to
stdout. Redirect it to the location your shell loads completions from, e.g.:

```console
$ sarena-cli completion bash > /etc/bash_completion.d/sarena-cli
```
