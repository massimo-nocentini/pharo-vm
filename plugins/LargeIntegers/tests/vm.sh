# vm.sh - sourced by run-image-tests.sh and bench.sh: run a Smalltalk script in a VM
#
# Needs IMAGE (the .image to copy), WORK (where the runs go) and limit (a timeout
# command prefix, or empty) to be set by the caller.

# the VM executable in a VM directory
vmexe() {
    if test -x "$1/pharo"; then echo "$1/pharo"
    elif test -x "$1/Pharo.app/Contents/MacOS/Pharo"; then echo "$1/Pharo.app/Contents/MacOS/Pharo"
    else return 1
    fi
}

# run NAME VM-EXE VM-DIR CHUNK FILE...: the FILEs, then the CHUNK, on a fresh copy of the
# image in $WORK/NAME; stdout in $WORK/NAME/out, stderr in $WORK/NAME/err, the outcome
# in $outcome: OK, CRASH, TIMEOUT or ERROR <exit status>, the seconds it took in $secs.
# LD_LIBRARY_PATH names the VM directory: the pharo of a build may otherwise load the
# libraries of another (its RUNPATH names the build directory)
run() {
    run_d=$WORK/$1 run_exe=$2 run_dir=$3 run_chunk=$4
    shift 4
    rm -rf "$run_d" && mkdir -p "$run_d" || exit 2
    cp "$IMAGE" "$run_d/li.image" || exit 2
    if test -f "${IMAGE%.image}.changes"; then cp "${IMAGE%.image}.changes" "$run_d/li.changes"; else : > "$run_d/li.changes"; fi
    for s in "$(dirname "$IMAGE")"/*.sources; do
        test -f "$s" && { ln -s "$s" "$run_d/" 2>/dev/null || cp "$s" "$run_d/"; }
    done
    cat "$@" > "$run_d/script.st"
    printf '\n%s\n' "$run_chunk" >> "$run_d/script.st"
    run_start=$(date +%s)
    (cd "$run_d" && LD_LIBRARY_PATH="$run_dir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
        $limit "$run_exe" --headless li.image st --quit script.st > out 2> err < /dev/null)
    run_rc=$?
    secs=$(( $(date +%s) - run_start ))
    if test $run_rc = 124 || test $run_rc = 137; then
        outcome=TIMEOUT
    elif test -f "$run_d/crash.dmp" || grep -qE '^(Segmentation fault|Bus error|Illegal instruction|Floating point exception|Abort)' "$run_d/out" "$run_d/err"; then
        outcome=CRASH
    elif test $run_rc != 0; then
        outcome="ERROR $run_rc"
    else
        outcome=OK
    fi
}
