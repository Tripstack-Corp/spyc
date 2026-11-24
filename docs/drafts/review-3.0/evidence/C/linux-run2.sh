B=/home/rev/.cache/spyc/target/spyc-76690fe5/debug/spyc
mode=$1; lim=$2; name=$3
mkdir -p /out/$name
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin python3 -I /unitc/drive_v7.py $B /out/$name $mode $lim
echo "== $name exit $?"
