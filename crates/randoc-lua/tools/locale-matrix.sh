#!/bin/sh
# The oracle matrix of the character locale (row I-LUA-13).
#
# Usage (after `. env.sh`, from the root of the worktree):
#
#     cargo build -p randoc-lua --example randoc-lua
#     sh crates/randoc-lua/tools/locale-matrix.sh SCRATCH_FOLDER
#
# The script writes a probe filter and an empty document to SCRATCH_FOLDER
# (it must exist) and runs the oracle and the driver of the crate under 20
# environments, one process of each at a time. It prints `equal` or the two
# outputs for each environment. It removes nothing.
set -eu
DIR=$1
DRIVER=target/debug/examples/randoc-lua
cat > "$DIR/locale-probe.lua" <<'LUA'
local out = {}
local function bytes(s) return (s:gsub('.', function(c) return c:byte() .. ' ' end)) end
local function p(label, fn) local ok, r = pcall(fn) out[#out+1] = label .. ' = ' .. tostring(r) end
p('q159', function() return bytes(string.format('%q', '\159')) end)
p('q233', function() return bytes(string.format('%q', '\233')) end)
for _, cls in ipairs{'a','c','l','u','w','p','s','g','x','d'} do
  p('class %' .. cls, function() local n = 0 for b = 128, 255 do if string.char(b):find('%' .. cls) then n = n + 1 end end return n end)
end
p('upper dash', function() return bytes(('\226\128\147'):upper()) end)
p('lower E9', function() return bytes(('\195\137'):lower()) end)
p('upper 233', function() return bytes(('\233'):upper()) end)
p('text.upper dash', function() return bytes(pandoc.text.upper('\226\128\147')) end)
p('%f', function() return string.format('%f', 1.5) end)
p('tostring', function() return tostring(1.5) end)
p('date', function() return os.date('%A %B %c', 0) end)
p('setlocale', function() return os.setlocale() end)
p('ctype', function() return os.setlocale(nil, 'ctype') end)
p('numeric', function() return os.setlocale(nil, 'numeric') end)
p('time', function() return os.setlocale(nil, 'time') end)
p('collate', function() return os.setlocale(nil, 'collate') end)
p('strcoll', function() return tostring('a' < 'B') end)
function Pandoc() return pandoc.Pandoc{pandoc.CodeBlock(table.concat(out, '\n'))} end
LUA
echo '{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}' > "$DIR/locale-empty.json"
for envs in "LANG=C" "LANG=en_US.UTF-8" "LC_ALL=C" "LC_CTYPE=en_US.UTF-8" "LANG=de_DE.UTF-8" "LC_ALL=de_DE.UTF-8" "LC_NUMERIC=de_DE.UTF-8 LANG=en_US.UTF-8" "LANG=en_US.ISO8859-1" "NONE=1" "LC_NUMERIC=C LANG=en_US.UTF-8" "LC_TIME=C LANG=en_US.UTF-8" "LC_CTYPE=C" "LC_CTYPE=C LANG=en_US.UTF-8" "LANG=xx_YY.UTF-8" "LANG=POSIX" "LC_NUMERIC=de_DE.UTF-8" "LC_ALL=en_US.UTF-8" "LC_ALL=en_US.ISO8859-1" "LC_CTYPE=de_DE.UTF-8 LANG=C" "LC_MONETARY=C LANG=de_DE.UTF-8"; do
  a=$(env -i HOME="$HOME" PATH="$PATH" GHCRTS="$GHCRTS" $envs limit-run 3500 60 "$RANDOC_ORACLE_PANDOC" -f json -t json --lua-filter "$DIR/locale-probe.lua" < "$DIR/locale-empty.json" 2>&1)
  b=$(env -i HOME="$HOME" PATH="$PATH" $envs limit-run 1500 60 "$DRIVER" filter "$DIR/locale-probe.lua" < "$DIR/locale-empty.json" 2>&1)
  if [ "$a" = "$b" ]; then echo "equal   $envs"; else echo "DIFFER  $envs"; echo " oracle $a" | cut -c1-400; echo " randoc $b" | cut -c1-400; fi
done
