use anyhow::{Result, bail};

pub fn run(shell: &str) -> Result<()> {
    match shell {
        "zsh" => {
            print!("{ZSH_INIT}");
            Ok(())
        }
        "bash" => {
            print!("{BASH_INIT}");
            Ok(())
        }
        other => bail!("Unsupported shell `{other}`. Supported shells: zsh, bash"),
    }
}

const ZSH_INIT: &str = r#"# wt shell integration: ambient worker identity binding
wt-env() {
  eval "$(wt env)"
}

wt-coord-use() {
  eval "$(wt coord use "$@")"
}

wt-coord-exit() {
  eval "$(wt coord exit)"
}

typeset -ga chpwd_functions
if [[ ${chpwd_functions[(Ie)wt-env]} -eq 0 ]]; then
  chpwd_functions+=(wt-env)
fi
wt-env
"#;

const BASH_INIT: &str = r#"# wt shell integration: ambient worker identity binding
wt-env() {
  eval "$(wt env)"
}

wt-coord-use() {
  eval "$(wt coord use "$@")"
}

wt-coord-exit() {
  eval "$(wt coord exit)"
}

case ";${PROMPT_COMMAND:-};" in
  *";wt-env;"*) ;;
  *)
    if [ -n "${PROMPT_COMMAND:-}" ]; then
      PROMPT_COMMAND="${PROMPT_COMMAND}; wt-env"
    else
      PROMPT_COMMAND="wt-env"
    fi
    ;;
esac
wt-env
"#;
