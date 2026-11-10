#!/usr/bin/env bash
# The Box, paired and used over TCP: the invitation, the claim, the welcome,
# the device list and the Box's relay (docs/BOX-RULES.md sections 1 to 3 and 5).
#
# Question: does a Box that starts with nothing offer nothing to a stranger,
# pair from an invitation typed into it and a welcome made by the Key app,
# fingerprint words that agree on both sides, pass sealed messages between
# the devices on its list and refuse the others, follow new settings (a device
# taken off the list loses its mailbox), keep its key and its members across a
# restart, refuse a second process on its directory, turn itself off when the
# settings drop it, and refuse a mistyped or a different invitation?
#
# No Bitcoin Core is needed. The Box is the `openbmt-box` binary. The Key app
# and the owner phone are the `box_pair` example of `openbmt-box`, which uses
# the core functions and the link's client; it makes the invitation, checks
# the claim, builds the first settings (the Box listed as a Box, signed by both
# roots) and sends the welcome. The script carries nothing between the two:
# the invitation is typed into the Box's console (its standard input), and
# everything else goes through the Box's TCP port.
#
# The steps (printed as a table at the end):
#   1. A stranger can say hello to an unpaired Box and gets "no claim"; it
#      cannot make a mailbox or put a message.
#   2. An invitation is entered, the claim is fetched and verified, the
#      welcome is sent, the Box says paired, and the four fingerprint words
#      the Box printed are the ones the Key app side shows.
#   3. Two devices on the settings' list make mailboxes and pass sealed
#      messages through the Box; a device not on the list cannot.
#   4. New settings, sent as a sealed Settings message from the owner, remove
#      one device: its mailbox is gone and it is refused.
#   5. A restart on the same directory: still paired, the same key, the same
#      members.
#   6. A second `openbmt-box` on the same directory is refused.
#   7. Settings that drop the Box: its state is "turned off", its members are
#      empty and its POS token file is erased.
#   9. A stranger sends welcomes while an invitation is open: refused, the
#      same claim is still offered, and the real welcome then still pairs.
#   8. On a second Box: an invitation typed with one character changed is
#      refused by its check, and a different valid invitation makes the Key
#      app side reject the claim.
#
# Environment:
#   DRILL_DIR      scratch directory (default: a new one under ~/.cache/openbmt)
#   DRILL_PORTS    first of two free ports on 127.0.0.1 (default 18986)
#   DRILL_KEEP=1   keep the scratch directory afterwards
#   BOX_BIN        the `openbmt-box` binary
#   PAIR_BIN       the `box_pair` example binary
#   CARGO_TARGET_DIR  where a build goes (default ~/.cache/openbmt/target/box-e2e)
#
# Everything here is on 127.0.0.1 with keys made from fixed test names or
# from the system's randomness. No real keys or funds are involved. The
# processes and the scratch directory are removed on every exit.

set -uo pipefail

P0=${DRILL_PORTS:-18986}
PORT1=$P0
PORT2=$((P0 + 1))
mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/box-e2e-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
B1="$BASE/box1"
B2="$BASE/box2"
K1="$BASE/keyapp1"
K2="$BASE/keyapp2"
K3="$BASE/keyapp3"

say() { printf '\n== %s\n' "$*"; }
note() { printf '      %s\n' "$*"; }
die() {
  printf 'ABORT %s\n' "$*" >&2
  exit 2
}

declare -A RESULT MESSAGE
pass() { RESULT[$1]=PASS; MESSAGE[$1]=$2; printf 'PASS  step %s: %s\n' "$1" "$2"; }
bad() { RESULT[$1]=FAIL; MESSAGE[$1]=$2; printf 'FAIL  step %s: %s\n' "$1" "$2"; }
notrun() { RESULT[$1]="NOT RUN"; MESSAGE[$1]=$2; printf 'NOT RUN  step %s: %s\n' "$1" "$2"; }

mkdir -p "$B1" "$B2" "$K1" "$K2" "$K3"
declare -A PID LOG FDN
PID[1]=; PID[2]=
FDN[1]=3; FDN[2]=4
LAST=

stop_pid() { # pid
  local pid=$1
  [ -n "$pid" ] || return 0
  if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
    for _ in $(seq 1 20); do
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.25
    done
    kill -9 "$pid" 2>/dev/null || true
  fi
}

cleanup() {
  exec 3>&- 4>&- 2>/dev/null || true
  stop_pid "${PID[1]:-}"
  stop_pid "${PID[2]:-}"
  if [ "${DRILL_KEEP:-0}" != 1 ]; then rm -rf -- "$BASE"; fi
}
trap cleanup EXIT

TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/box-e2e}
build() {
  local guard=$HOME/.cache/openbmt/guard.sh
  if [ -x "$guard" ]; then
    "$guard" env CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-box --bins --example box_pair ||
      die "the build failed"
  else
    CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-box --bins --example box_pair ||
      die "the build failed"
  fi
}
if [ -z "${BOX_BIN:-}" ] || [ -z "${PAIR_BIN:-}" ]; then
  say "Build the Box and the Key app example"
  build
  [ -n "${BOX_BIN:-}" ] || BOX_BIN="$TARGET/debug/openbmt-box"
  [ -n "${PAIR_BIN:-}" ] || PAIR_BIN="$TARGET/debug/examples/box_pair"
fi
for bin in BOX_BIN PAIR_BIN; do
  [ -x "${!bin}" ] || die "no binary at ${!bin} ($bin)"
done

# ---- helpers ------------------------------------------------------------------

# Start a Box. Its standard input is a fifo held open on descriptor 3 (Box 1)
# or 4 (Box 2), so that console commands can be sent.
start_box() { # n data-dir port
  local n=$1 fifo="$BASE/stdin-$1"
  LOG[$n]="$BASE/box-$n.log"
  : >"${LOG[$n]}"
  rm -f "$fifo"
  mkfifo "$fifo"
  if [ "$n" = 1 ]; then exec 3<>"$fifo"; else exec 4<>"$fifo"; fi
  if [ "$n" = 1 ]; then
    "$BOX_BIN" "$2" "127.0.0.1:$3" <&3 >"${LOG[$n]}" 2>&1 &
  else
    "$BOX_BIN" "$2" "127.0.0.1:$3" <&4 >"${LOG[$n]}" 2>&1 &
  fi
  PID[$n]=$!
  for _ in $(seq 1 40); do
    grep -q '^state: ' "${LOG[$n]}" && return 0
    kill -0 "${PID[$n]}" 2>/dev/null || return 1
    sleep 0.25
  done
  return 1
}
# Send a console command and wait for a new line that starts with `prefix`.
# LAST is the newest such line.
ask() { # n command prefix
  local n=$1 before i fd=${FDN[$1]}
  before=$(grep -c "^$3" "${LOG[$n]}")
  echo "$2" >&"$fd"
  for i in $(seq 1 40); do
    if [ "$(grep -c "^$3" "${LOG[$n]}")" -gt "$before" ]; then
      LAST=$(grep "^$3" "${LOG[$n]}" | tail -1)
      return 0
    fi
    sleep 0.25
  done
  LAST=
  return 1
}
state_of() { # n -> prints the state words
  ask "$1" state 'state: ' || return 1
  printf '%s' "${LAST#state: }"
}
# Wait for the Box's state to be `want`.
await_state() { # n want seconds
  local i s
  for i in $(seq 1 $(($3 * 4))); do
    s=$(state_of "$1")
    [ "$s" = "$2" ] && return 0
    sleep 0.25
  done
  return 1
}
# The member keys the Box reports, sorted, one per line.
members_of() { # n
  local before
  before=$(grep -c '^members: ' "${LOG[$1]}")
  ask "$1" members 'members: ' || return 1
  sleep 0.3
  local count=${LAST#members: }
  grep '^member: ' "${LOG[$1]}" | tail -n "$count" | sed 's/^member: //' | sort
}
member_count() { # n
  ask "$1" members 'members: ' || return 1
  printf '%s' "${LAST#members: }"
}
stop_box() { # n
  local n=$1
  echo quit >&"${FDN[$n]}"
  for _ in $(seq 1 40); do
    kill -0 "${PID[$n]}" 2>/dev/null || break
    sleep 0.25
  done
  wait "${PID[$n]}" 2>/dev/null
  local rc=$?
  PID[$n]=
  if [ "$n" = 1 ]; then exec 3>&-; else exec 4>&-; fi
  return $rc
}
pair() { "$PAIR_BIN" "$@" 2>&1; }

# ---- 1 ---------------------------------------------------------------------------

say "1. An unpaired Box and a stranger"
start_box 1 "$B1" "$PORT1" || die "the Box did not start: $(cat "${LOG[1]}")"
FP1=$(sed -n 's/^fingerprint: //p' "${LOG[1]}")
KEY1=$(sed -n 's/^key: //p' "${LOG[1]}")
note "Box key $KEY1, fingerprint words: $FP1"
S1=$(state_of 1)
CLAIM_NONE=$(pair claim "$K1" "127.0.0.1:$PORT1")
MB_STRANGER=$(pair mailbox "$K1" "127.0.0.1:$PORT1" stranger)
SEND_STRANGER=$(pair send "$K1" "127.0.0.1:$PORT1" stranger owner hello)
note "claim: $CLAIM_NONE"
note "mailbox: $MB_STRANGER"
note "put: $SEND_STRANGER"
if [ "$S1" = unpaired ] && grep -q 'no open invitation' <<<"$CLAIM_NONE" &&
  grep -q '^refused' <<<"$MB_STRANGER" && grep -q '^refused' <<<"$SEND_STRANGER"; then
  pass 1 "an unpaired Box said hello (the Key app example checks its signature), answered a claim request with no claim, and refused a mailbox and a message"
else
  bad 1 "state '$S1'; claim: $CLAIM_NONE; mailbox: $MB_STRANGER; put: $SEND_STRANGER"
fi

# ---- 2 ---------------------------------------------------------------------------

say "2. Invitation, claim, welcome"
INV=$(pair invite "$K1")
note "invitation (typed into the Box's console): $INV"
ask 1 "invite $INV" 'invitation' || note "no answer to the invitation"
S2a=$(state_of 1)
CLAIM_OUT=$(pair claim "$K1" "127.0.0.1:$PORT1")
note "$(sed 's/^/key app: /' <<<"$CLAIM_OUT")"
KEYAPP_FP=$(sed -n 's/^box fingerprint: //p' <<<"$CLAIM_OUT")
WELCOME_OUT=$(pair welcome "$K1" "127.0.0.1:$PORT1")
note "welcome: $WELCOME_OUT"
S2b=$(state_of 1)
NOW_CLAIM=$(pair claim "$K1" "127.0.0.1:$PORT1")
if [ "$S2a" = "invitation open" ] && grep -q '^claim verified' <<<"$CLAIM_OUT" &&
  [ -n "$KEYAPP_FP" ] && [ "$KEYAPP_FP" = "$FP1" ] && [ "$WELCOME_OUT" = welcomed ] &&
  [ "$S2b" = paired ]; then
  pass 2 "the state went unpaired, invitation open, paired; the claim verified; both sides show the words: $FP1"
else
  bad 2 "state before '$S2a', after '$S2b'; key app words '$KEYAPP_FP' vs Box '$FP1'; welcome: $WELCOME_OUT"
fi

# ---- 3 ---------------------------------------------------------------------------

say "3. Devices on the list pass messages; others cannot"
A="127.0.0.1:$PORT1"
M1=$(pair mailbox "$K1" "$A" c1)
M2=$(pair mailbox "$K1" "$A" c2)
MO=$(pair mailbox "$K1" "$A" owner)
SEND1=$(pair send "$K1" "$A" c1 owner "hello-from-c1")
RECV1=$(pair recv "$K1" "$A" owner)
SEND2=$(pair send "$K1" "$A" owner c2 "hello-to-c2")
RECV2=$(pair recv "$K1" "$A" c2)
MBS=$(pair mailbox "$K1" "$A" stranger)
SENDS=$(pair send "$K1" "$A" stranger owner "no")
C2KEY=$(pair key "$K1" c2)
note "$RECV1"
note "$RECV2"
note "stranger: $MBS / $SENDS"
if grep -q '^mailbox made' <<<"$M1" && grep -q '^mailbox made' <<<"$M2" && grep -q '^mailbox made' <<<"$MO" &&
  grep -q 'message from c1: hello-from-c1' <<<"$RECV1" && grep -q 'message from owner: hello-to-c2' <<<"$RECV2" &&
  grep -q '^refused' <<<"$MBS" && grep -q '^refused' <<<"$SENDS" && [ -e "$B1/relay/$C2KEY.mbx" ]; then
  pass 3 "c1 to the owner and the owner to c2 went through the Box and opened; a device not on the list was refused a mailbox and a message"
else
  bad 3 "mailboxes: $M1 / $M2 / $MO; recv: $RECV1 / $RECV2; stranger: $MBS / $SENDS"
fi

# ---- 4 ---------------------------------------------------------------------------

say "4. New settings take a device off the list"
N_BEFORE=$(member_count 1)
SET2=$(pair settings "$K1" "$A" 2 drop-c2)
note "$SET2"
GONE=
for _ in $(seq 1 40); do
  N=$(member_count 1)
  if [ -n "$N" ] && [ "$N" -lt "${N_BEFORE:-0}" ]; then GONE=yes; break; fi
  sleep 0.5
done
RECV_C2=$(pair recv "$K1" "$A" c2)
MB_C2=$(pair mailbox "$K1" "$A" c2)
note "members before $N_BEFORE, after $N"
note "c2: $RECV_C2 / $MB_C2"
if [ -n "$GONE" ] && [ ! -e "$B1/relay/$C2KEY.mbx" ] && grep -q '^refused' <<<"$RECV_C2" &&
  grep -q '^refused' <<<"$MB_C2" && grep -q 'mailbox made for c1' <<<"$(pair mailbox "$K1" "$A" c1)"; then
  pass 4 "settings version 2 (c2 off the list) went in as a sealed message; the members fell from $N_BEFORE to $N, c2's mailbox file is gone and c2 is refused; c1 still works"
else
  bad 4 "members $N_BEFORE -> ${N:-?}; mailbox file present: $([ -e "$B1/relay/$C2KEY.mbx" ] && echo yes || echo no); c2: $RECV_C2 / $MB_C2"
fi

# ---- 5 ---------------------------------------------------------------------------

say "5. A restart on the same directory"
MEM_BEFORE=$(members_of 1)
stop_box 1
start_box 1 "$B1" "$PORT1" || die "the Box did not start again: $(cat "${LOG[1]}")"
KEY1_AGAIN=$(sed -n 's/^key: //p' "${LOG[1]}")
S5=$(state_of 1)
MEM_AFTER=$(members_of 1)
note "state $S5, key $KEY1_AGAIN"
if [ "$S5" = paired ] && [ "$KEY1_AGAIN" = "$KEY1" ] && [ -n "$MEM_BEFORE" ] && [ "$MEM_BEFORE" = "$MEM_AFTER" ]; then
  pass 5 "after the restart the Box is paired, has the same key and the same $(wc -l <<<"$MEM_AFTER") members"
else
  bad 5 "state '$S5'; key before $KEY1, after $KEY1_AGAIN; members equal: $([ "$MEM_BEFORE" = "$MEM_AFTER" ] && echo yes || echo no)"
fi

# ---- 6 ---------------------------------------------------------------------------

say "6. A second process on the same directory"
SECOND=$(timeout 20 "$BOX_BIN" "$B1" "127.0.0.1:$PORT2" </dev/null 2>&1)
RC=$?
note "exit $RC: $SECOND"
if [ "$RC" != 0 ] && grep -q 'another OpenBMT process' <<<"$SECOND"; then
  pass 6 "the second process was refused with: $SECOND"
else
  bad 6 "the second process was not refused (exit $RC): $SECOND"
fi

# ---- 7 ---------------------------------------------------------------------------

say "7. Settings that drop the Box"
printf 'token' >"$B1/pos.token"
SET3=$(pair settings "$K1" "$A" 3 drop-box)
note "$SET3"
if await_state 1 "turned off" 20; then OFF=yes; else OFF=; fi
N7=$(member_count 1)
note "members after: $N7"
if [ -n "$OFF" ] && [ "$N7" = 0 ] && [ ! -e "$B1/pos.token" ] &&
  grep -q '^refused' <<<"$(pair mailbox "$K1" "$A" c1)"; then
  pass 7 "the state is turned off, the member list is empty, pos.token is erased and a mailbox is refused"
else
  bad 7 "turned off: ${OFF:-no}; members $N7; pos.token present: $([ -e "$B1/pos.token" ] && echo yes || echo no)"
fi
stop_box 1

# ---- 8 ---------------------------------------------------------------------------

say "8. A mistyped invitation and a different invitation"
start_box 2 "$B2" "$PORT2" || die "the second Box did not start: $(cat "${LOG[2]}")"
INV2=$(pair invite "$K2")
FIRST=${INV2:0:1}
if [ "$FIRST" = 0 ]; then OTHER=1; else OTHER=0; fi
TYPO="$OTHER${INV2:1}"
ask 2 "invite $TYPO" 'invitation' || true
note "typed with one character changed: $LAST"
TYPO_ANSWER=$LAST
S8a=$(state_of 2)
CLAIM8=$(pair claim "$K2" "127.0.0.1:$PORT2")
ask 2 "invite $INV2" 'invitation' || true
S8b=$(state_of 2)
# The Key app makes a different invitation, replacing the one it held.
INV3=$(pair invite "$K2")
CLAIM_WRONG=$(pair claim "$K2" "127.0.0.1:$PORT2")
WELCOME_AFTER=$(pair welcome "$K2" "127.0.0.1:$PORT2")
note "different invitation: $CLAIM_WRONG"
if grep -q 'check characters do not match' <<<"$TYPO_ANSWER" && [ "$S8a" = unpaired ] &&
  grep -q 'no open invitation' <<<"$CLAIM8" && [ "$S8b" = "invitation open" ] &&
  grep -q 'claim REJECTED' <<<"$CLAIM_WRONG" && ! grep -q '^welcomed' <<<"$WELCOME_AFTER"; then
  pass 8 "the typo was refused by its check and left the Box unpaired with no claim; with a valid invitation open, the Key app holding another one rejected the claim and sent no welcome"
else
  bad 8 "typo answer '$TYPO_ANSWER' (state $S8a); claim after typo: $CLAIM8; state with real invitation $S8b; wrong-invitation claim: $CLAIM_WRONG; welcome: $WELCOME_AFTER"
fi
stop_box 2

# ---- 9 ---------------------------------------------------------------------------

say "9. A stranger's welcome while the invitation is open"
A2="127.0.0.1:$PORT2"
start_box 2 "$B2" "$PORT2" || die "the second Box did not start again: $(cat "${LOG[2]}")"
INV9=$(pair invite "$K3")
ask 2 "invite $INV9" 'invitation' || true
S9a=$(state_of 2)
BAD1=$(pair badwelcome "$K3" "$A2")
sleep 2
BAD2=$(pair badwelcome "$K3" "$A2")
sleep 2
S9b=$(state_of 2)
CLAIM9=$(pair claim "$K3" "$A2")
WELCOME9=$(pair welcome "$K3" "$A2")
S9c=$(state_of 2)
note "stranger: $BAD1 / $BAD2"
note "state after: $S9b; claim: $(head -1 <<<"$CLAIM9"); welcome: $WELCOME9"
if [ "$S9a" = "invitation open" ] && grep -q '^refused' <<<"$BAD1" && grep -q '^refused' <<<"$BAD2" &&
  [ "$S9b" = "invitation open" ] && grep -q '^claim verified' <<<"$CLAIM9" &&
  [ "$WELCOME9" = welcomed ] && [ "$S9c" = paired ]; then
  pass 9 "two welcomes from a stranger were refused and left the invitation open; the real claim then verified and the real welcome paired the Box"
else
  bad 9 "states '$S9a' '$S9b' '$S9c'; stranger: $BAD1 / $BAD2; claim: $CLAIM9; welcome: $WELCOME9"
fi
stop_box 2

# ---- result ----------------------------------------------------------------------

say "Result"
failed=0
notrun=0
for n in 1 2 3 4 5 6 7 8 9; do
  printf '%s  step %s\n' "${RESULT[$n]:-FAIL}" "$n"
  case "${RESULT[$n]:-FAIL}" in
    PASS) ;;
    "NOT RUN") notrun=$((notrun + 1)) ;;
    *) failed=1 ;;
  esac
done
if [ "$failed" != 0 ]; then
  echo "SOME STEPS FAILED"
  exit 1
elif [ "$notrun" != 0 ]; then
  echo "PASS for the steps that ran; $notrun steps NOT RUN"
else
  echo "ALL PASS"
fi
