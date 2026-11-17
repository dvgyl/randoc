# Environment of the build on the gpt/* branches.
# Source it from the root of the clone or of a worker worktree:
#   . tools/autobuild/env.sh
. "/Volumes/Extreme Pro/Randoc/env/env.sh"
export RANDOC_GPT="/Volumes/Extreme Pro/Randoc/gpt"
# The build sandbox can write only below RANDOC_GPT and the Rust tool folders.
export TMPDIR="$RANDOC_GPT/tmp/"
export RANDOC_RUNS="$RANDOC_GPT/runs"
export RANDOC_CORPUS="$RANDOC_GPT/corpus"
export RANDOC_WT="$RANDOC_GPT/wt"
mkdir -p "$TMPDIR" "$RANDOC_RUNS"
_randoc_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
export PATH="$_randoc_root/tools/autobuild/bin:$PATH"
unset _randoc_root
