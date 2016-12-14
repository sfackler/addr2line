#!/bin/bash
if [[ $# -le 1 ]]; then
	echo "Usage: $0 <executable> [<addresses>] REFS..."
	exit 1
fi
target="$1"
shift

addresses=""
if [[ -e "$1" ]]; then
	addresses="$1"
	shift
fi

# path to "us"
# readlink -f, but more portable:
dirname=$(perl -e 'use Cwd "abs_path";print abs_path(shift)' "$(dirname "$0")")

# compile all refs
pushd "$dirname" > /dev/null
if ! git diff-index --quiet HEAD --; then
	echo "Refusing to overwrite outstanding git changes"
	exit 2
fi
current=$(git symbolic-ref --short HEAD)
for ref in "$@"; do
	echo "==> Compiling $ref"
	git checkout -q "$ref"
	commit=$(git rev-parse HEAD)
	fn="target/release/addr2line-$commit"
	if [[ ! -e "$fn" ]]; then
		cargo build --release
		cp target/release/addr2line "$fn"
	fi
	if [[ "$ref" != "$commit" ]]; then
		ln -sfn "addr2line-$commit" target/release/addr2line-"$ref"
	fi
done
git checkout -q "$current"
popd > /dev/null

# get us some addresses to look up
if [[ -z "$addresses" ]]; then
	echo "==> Looking for benchmarking addresses (this may take a while)"
	addresses=$(mktemp tmp.XXXXXXXXXX)
	objdump -C -x --disassemble -l "$target" \
		| grep -P '0[048]:' \
		| awk '{print $1}' \
		| sed 's/:$//' \
		> "$addresses"
	echo "  -> Addresses stored in $addresses; you should re-use it next time"
fi

run() {
	func="$1"
	name="$2"
	cmd="$3"
	args="$4"
	printf "%s\t%s\t" "$name" "$func"
	/usr/bin/time -f '%e\t%M' "$cmd" $args -e "$target" < "$addresses" 2>&1 >/dev/null
}

log=$(mktemp tmp.XXXXXXXXXX)

# run without functions
echo "==> Benchmarking"
run nofunc binutils addr2line > "$log"
for ref in "$@"; do
	run nofunc "$ref" "$dirname/target/release/addr2line-$ref" >> "$log"
done

# run with functions
echo "==> Benchmarking with -f"
run func binutils addr2line -f >> "$log"
for ref in "$@"; do
	run func "$ref" "$dirname/target/release/addr2line-$ref" -f >> "$log"
done

echo "==> Plotting"
Rscript --no-readline --no-restore --no-save "$dirname/bench.plot.r" < "$log"

echo "==> Cleaning up"
rm "$log"