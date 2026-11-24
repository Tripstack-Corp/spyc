# Runs in localhost/spyc-review-linux (aarch64), in the probe container's pid namespace.
S=$1
i=0; while [ ! -f $S/attach-now ]; do sleep 0.1; i=$((i+1)); [ $i -gt 600 ] && exit 3; done
pid=$(cat $S/jarpid)
echo "tracing $pid: $(tr '\0' ' ' < /proc/$pid/cmdline)"
timeout 600 strace -f -tt -p $pid -e trace=sendmsg,kill,tgkill,exit_group -e signal=SIGPIPE -e inject=sendmsg:delay_enter=300000 -o $S/strace.log &
sleep 2; touch $S/attached; wait
echo "strace done"
