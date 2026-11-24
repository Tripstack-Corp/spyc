#!/bin/sh
ls /home/rev/.cache/spyc/target/
du -sh /home/rev/.cache/spyc/target/*
find /home/rev/.cache/spyc/target -maxdepth 7 -type f -perm -u+x | grep -E 'jar_process-|spyc_jar-|/spyc-jar$' | grep -vE '\.(d|o|rlib|rmeta)$' | head -20
