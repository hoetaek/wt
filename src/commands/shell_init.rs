use crate::cli::ShellInitShell;

pub fn run(shell: ShellInitShell) {
    match shell {
        ShellInitShell::Zsh => print!("{ZSH_INIT}"),
        ShellInitShell::Bash => print!("{BASH_INIT}"),
    }
}

const ZSH_INIT: &str = r#"# wt shell integration: ambient worker identity binding
wt-env() {
  eval "$(wt env)"
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

if declare -p PROMPT_COMMAND 2>/dev/null | grep -q '^declare \-[^ ]*a'; then
  wt_prompt_has_env=0
  for wt_prompt_command in "${PROMPT_COMMAND[@]}"; do
    if [ "$wt_prompt_command" = "wt-env" ]; then
      wt_prompt_has_env=1
      break
    fi
  done
  if [ "$wt_prompt_has_env" -eq 0 ]; then
    PROMPT_COMMAND+=(wt-env)
  fi
  unset wt_prompt_has_env wt_prompt_command
else
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
fi
wt-env
"#;
