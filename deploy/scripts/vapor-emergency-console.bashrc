# At an interactive prompt, close the shell after ten minutes without input.
# The systemd unit independently enforces the 20-minute absolute lifetime.
readonly TMOUT=600
export TMOUT
HISTFILE=/dev/null
readonly HISTFILE
HISTSIZE=0
HISTFILESIZE=0
