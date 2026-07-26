echo "running tool"
for f in alpha beta gamma; do
  echo "item $f"
done
i=0
while [ "$i" -lt 2 ]; do
  echo "tick $i"
  i=$((i + 1))
done
if [ "$i" -eq 2 ]; then
  echo "counted to $i"
else
  echo "unexpected $i"
fi
case "$i" in
  2) echo "case two" ;;
  *) echo "case other" ;;
esac
