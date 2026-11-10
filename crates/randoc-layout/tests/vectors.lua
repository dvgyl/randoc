-- Writes the test vectors of randoc-layout. Run it with Pandoc 3.13:
--   pandoc lua crates/randoc-layout/tests/vectors.lua > crates/randoc-layout/tests/vectors.txt
--
-- Line formats of the output:
--   width START END A B C D E   widths for each code point from START to END
--   len "TEXT" N                real_length of TEXT
--   doc EXPR                    a Doc expression, followed by its results
--   meta EMPTY OFFSET MIN_OFFSET HEIGHT COLUMN0 COLUMN5
--   plain WIDTH "TEXT"          plain render; WIDTH "-" means no wrapping
--   ansi WIDTH "TEXT"           ANSI render
-- In width lines, A is the width of the code point alone, B of "a" and the
-- code point, C of the code point and U+FE0F, D of the code point and
-- U+1F3FB, and E of the code point, U+200D and "a".

local L = pandoc.layout

local function quote(s)
  local out = { '"' }
  for _, cp in utf8.codes(s) do
    if cp == 0x22 then
      out[#out + 1] = '\\"'
    elseif cp == 0x5C then
      out[#out + 1] = '\\\\'
    elseif cp >= 0x20 and cp <= 0x7E then
      out[#out + 1] = string.char(cp)
    else
      out[#out + 1] = string.format('\\u{%X}', cp)
    end
  end
  out[#out + 1] = '"'
  return table.concat(out)
end

-- Width sweep over all scalar values, run-length encoded.
local rl = L.real_length
local run_start, run_key
local function flush_run(last)
  if run_key then
    print(string.format('width %X %X %s', run_start, last, run_key))
  end
end
local prev = 0
for cp = 0, 0x10FFFF do
  if cp < 0xD800 or cp > 0xDFFF then
    local c = utf8.char(cp)
    local key = table.concat({
      rl(c), rl('a' .. c), rl(c .. '\u{FE0F}'), rl(c .. '\u{1F3FB}'),
      rl(c .. '\u{200D}a') }, ' ')
    if key ~= run_key or cp ~= prev + 1 then
      flush_run(prev)
      run_start, run_key = cp, key
    end
    prev = cp
  end
end
flush_run(prev)

local samples = {
  '', 'abc', 'This is going to be too long anyway', 'Circle\u{25EF}Circle',
  '*a', '*\u{FE0F}\u{20E3}a', '\u{231A}', '\u{A9}\u{FE0F}', '\u{A9}',
  '\u{1F170}\u{FE0F}\u{1F1E6}\u{1F1E8}', '\u{1F590}\u{1F3FF}', '1\u{1F3FF}',
  '\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}',
  '\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}', '\u{1F44B}\u{1F3FD}',
  '\u{1F469}\u{1F3FD}\u{200D}\u{1F4BB}', 'e\u{301}\u{302}', '\u{301}a',
  '\u{301}\u{301}', '日本語', 'ｱｲｳ', '한국어', '\u{20000}\u{2A6D6}',
  'a\u{200B}b', 'a\u{200D}b', 'a\u{FE0E}', '\u{2764}\u{FE0F}', '\u{2764}\u{FE0E}',
  'a\tb', 'a\0b', '\u{7F}\u{80}\u{9F}', '\u{AD}', 'x\u{1F600}\u{200D}',
  '\u{FE0F}\u{200D}', '\u{1F3FB}\u{200D}\u{1F3FB}', 'a\u{FE0F}\u{200D}\u{200D}',
  '\u{E0001}\u{E0100}', '\u{F0000}', '\u{1F1FA}\u{1F1F8}', 'नमस्ते', 'สวัสดี',
  'مرحبا\u{64B}', 'Ελληνικά', 'Привет\u{483}', '\u{3099}\u{309A}', '〇〆',
}
for _, s in ipairs(samples) do
  print('len ' .. quote(s) .. ' ' .. rl(s))
end

-- Doc expressions. Atoms are quoted literals or bare names; lists are
-- (NAME ARG ...).
local function tokenize(src)
  local tokens, i = {}, 1
  while i <= #src do
    local ch = src:sub(i, i)
    if ch:match('%s') then
      i = i + 1
    elseif ch == '(' or ch == ')' then
      tokens[#tokens + 1] = ch
      i = i + 1
    elseif ch == '"' then
      local j, buf = i + 1, {}
      while src:sub(j, j) ~= '"' do
        local c = src:sub(j, j)
        if c == '\\' then
          local n = src:sub(j + 1, j + 1)
          if n == 'u' then
            local close = src:find('}', j, true)
            buf[#buf + 1] = utf8.char(tonumber(src:sub(j + 3, close - 1), 16))
            j = close + 1
          else
            buf[#buf + 1] = n
            j = j + 2
          end
        else
          buf[#buf + 1] = c
          j = j + 1
        end
      end
      tokens[#tokens + 1] = { str = table.concat(buf) }
      i = j + 1
    else
      local j = i
      while j <= #src and not src:sub(j, j):match('[%s()]') do j = j + 1 end
      tokens[#tokens + 1] = src:sub(i, j - 1)
      i = j
    end
  end
  return tokens
end

local function parse(tokens, pos)
  local t = tokens[pos]
  if t == '(' then
    local list = {}
    pos = pos + 1
    while tokens[pos] ~= ')' do
      local item
      item, pos = parse(tokens, pos)
      list[#list + 1] = item
    end
    return list, pos + 1
  end
  return t, pos + 1
end

local function foldr(f, docs)
  local acc = L.empty
  for i = #docs, 1, -1 do acc = f(docs[i], acc) end
  return acc
end

local eval
local unary = {
  flush = L.flush, nowrap = L.nowrap, chomp = L.chomp, nestle = L.nestle,
  braces = L.braces, brackets = L.brackets, parens = L.parens,
  quotes = L.quotes, double_quotes = L.double_quotes,
  before_non_blank = L.before_non_blank, bold = L.bold, italic = L.italic,
  underlined = L.underlined, strikeout = L.strikeout,
}
local blocks = { lblock = L.lblock, cblock = L.cblock, rblock = L.rblock }

eval = function(e)
  if type(e) == 'table' and e.str then return L.literal(e.str) end
  if e == 'space' then return L.space end
  if e == 'cr' then return L.cr end
  if e == 'blankline' then return L.blankline end
  if e == 'empty' then return L.empty end
  local name = e[1]
  local function arg(i) return eval(e[i]) end
  local function args(from)
    local r = {}
    for i = from, #e do r[#r + 1] = eval(e[i]) end
    return r
  end
  if name == 'cat' then return foldr(function(a, b) return a .. b end, args(2)) end
  if name == 'sp' then return arg(2) + arg(3) end
  if name == 'above' then return arg(2) / arg(3) end
  if name == 'above_blank' then return arg(2) // arg(3) end
  if name == 'hsep' then return foldr(function(a, b) return a + b end, args(2)) end
  if name == 'vcat' then return foldr(function(a, b) return a / b end, args(2)) end
  if name == 'vsep' then return foldr(function(a, b) return a // b end, args(2)) end
  if name == 'blanklines' then return L.blanklines(tonumber(e[2])) end
  if name == 'nest' then return L.nest(arg(3), tonumber(e[2])) end
  if name == 'hang' then return L.hang(arg(4), tonumber(e[2]), arg(3)) end
  if name == 'prefixed' then return L.prefixed(arg(3), e[2].str) end
  if name == 'inside' then return L.inside(arg(4), arg(2), arg(3)) end
  if name == 'after_break' then return L.after_break(e[2].str) end
  if name == 'vfill' then return L.vfill(e[2].str) end
  if name == 'fg' then return L.fg(arg(3), e[2]) end
  if name == 'bg' then return L.bg(arg(3), e[2]) end
  if blocks[name] then return blocks[name](arg(3), tonumber(e[2])) end
  if unary[name] then return unary[name](arg(2)) end
  error('unknown expression: ' .. tostring(name))
end

local function serialize(e)
  if type(e) == 'table' and e.str then return quote(e.str) end
  if type(e) == 'string' then return e end
  local parts = {}
  for i, x in ipairs(e) do parts[i] = serialize(x) end
  return '(' .. table.concat(parts, ' ') .. ')'
end

local fixed = {
  -- The test cases of doclayout 0.6, test/test.hs.
  '(blanklines 0)',
  '(cat (nest 3 (cat "**" "thisIsGoingToBeTooLongAnyway" "**")) blankline)',
  '(cat "hello" (nest 2 "world"))',
  '(cat "aa" (nest 2 (cat "bb" cr "cc")))',
  '(lblock 4 "hi there")',
  '(lblock 3 "abcdefg")',
  '(lblock 2 "ab\\u{302}cd")',
  '(lblock 4 (bold "hi there"))',
  '(nest 3 "abcdefghi")',
  '(vcat (chomp "aaa") (chomp "bbb") (chomp "ccc"))',
  '(above (chomp "aaa") (above (chomp "bbb") (chomp "ccc")))',
  '(sp "hello" "there")',
  '(cat "hello" cr "there")',
  '(above "x" cr)',
  '(hsep "hello" "there" "this" "is" "a" "test")',
  '(cat (lblock 3 "aa") (lblock 3 "bb") (lblock 3 (sp "aa" "bbbb")))',
  '(prefixed "> " (cat (sp "foo" "bar") cr "baz" blankline (sp "bim" "bam")))',
  '(prefixed "> " (hsep "a" "b" "c"))',
  '(nest 4 "aa\\u{A}\\u{A}bb\\u{A}cc")',
  '(cat (hang 4 "  - " (cat (chomp "aa\\u{A}\\u{A}bb\\u{A}cc") cr)) (hang 4 "  - " (cat (chomp "dd\\u{A}") cr)))',
  '(cat "aa" (lblock 2 (above "bb" "cc")) "dd")',
  '(cat "aa" (cblock 4 (above "bb" "cc")))',
  '(cat (blanklines 2) "aa")',
  '(cat "aa" (blanklines 2))',
  '(cat "aa" cr (blanklines 2) (blanklines 0))',
  '(cat (nest 2 (cat (nest 3 (cat "aa" blankline)) cr (blanklines 2))) (blanklines 2))',
  '(cat "aa" blankline cr blankline "bb")',
  '(vcat (hang 2 "- " (chomp (cat "aaaaa" space "bbb"))) (hang 2 "- " "aoeu"))',
  '(cat "aaaaa" space "bbbbb" blankline "ccccc")',
  '(cat (chomp (cat (cat "aa" space) blankline)) "bb")',
  '(cat (chomp (cat "aa" space)) "bb")',
  '(chomp "aa")',
  '(chomp (nest 3 (cat "aa" blankline)))',
  '(cat "aa" cr (chomp (cat (lblock 2 (cat "aa" blankline)) blankline)))',
  '(above "aa" (above empty "bb"))',
  '(vcat (cat (rblock 4 "aa") (lblock 3 " | ") (cblock 4 "bb") (lblock 3 " | ") (lblock 4 "cc")) (cat (rblock 4 "----") (lblock 3 " | ") (cblock 4 "----") (lblock 3 " | ") (lblock 4 "----")) (cat (rblock 4 "dd") (lblock 3 " | ") (cblock 4 "ee") (lblock 3 " | ") (lblock 4 "ff")))',
  '(cat "aa" space "bbbbb" "ccccc")',
  '(cat (nest 5 (hsep "hi" "there" "my" "friend")) cr)',
  '(cblock 7 (sp "hi" "there"))',
  '(cat (sp "hi" (after_break "!")) (after_break "?") "x" (after_break "?"))',
  '(cat "[" (nest 1 (above "aaaaaaaaa" "bbbbbbb")) "]")',
  '(above "aa" (above (nest 3 empty) "bb"))',
  '(hsep "a" empty "b")',
  '(sp "a" (sp empty "b"))',
  '(cat (vcat "aa" "bb") "cc")',
  '(vcat)',
  '(nestle (above (blanklines 2) (above "aa" (cat (blanklines 2) cr))))',
  '(prefixed "> " (cblock 4 (above "aa" "bb")))',
  '(cat empty space "x")',
  '(hang 5 (nowrap "xxx") empty)',
  '(above (cat (before_non_blank "!!") " ab") (cat (before_non_blank "!!") "a b"))',
  '(cat (vfill "|") (cblock 5 (vcat "a" "bbb" "ccccc")) (lblock 3 "dd") (vfill "+"))',
  '(cat (vfill "| ") (cblock 5 (vcat "a" "bbb")) (vfill " | ") (lblock 2 (vcat "dd" "ee" "ff")) (vfill " |"))',
  '(cat "\\u{366}" space "a")',
  '(cat "a" cr "\\u{A}b" cr "c")',
  '(nowrap (bold (cat "aa" space "bb")))',
  '(nowrap (prefixed "> " (cat "aa" space "bb")))',
  '(sp "hi" (fg blue (sp "mom" (sp "and" "dad"))))',
  -- Further edge cases.
  'empty', 'space', 'cr', 'blankline', '(blanklines 3)', '""', '"\\u{A}"',
  '"a\\u{A}"', '"\\u{A}\\u{A}a"', '(cat cr cr "a")', '(cat space space "a" space space)',
  '(cat "a" space space "b")', '(cat "a" blankline blankline "b")',
  '(cat "a" (blanklines 2) blankline "b")', '(cat "a" cr blankline cr "b")',
  '(cat "a" "\\u{A}" blankline "b")', '(cat "a" space "\\u{A}" "b")',
  '(cat "a" space cr "b")', '(cat "a" space blankline "b")', '(cat "a" cr space "b")',
  '(prefixed "> " empty)', '(prefixed "> " blankline)', '(prefixed "> " cr)',
  '(prefixed "> " (cat "a" blankline "b" blankline))',
  '(prefixed "> " (prefixed "| " (cat "a" cr "" cr "b")))',
  '(prefixed "> " (cat blankline "a"))',
  '(cat "xx" (prefixed "> " (cat "a" cr "b")))',
  '(prefixed "  " (cat "a" blankline (nest 2 (cat "b" cr "c"))))',
  '(prefixed "> " (flush (cat "a" cr "b")))',
  '(prefixed "> " (cat "a" cr (flush "b") cr "c"))',
  '(flush empty)', '(flush (bold "a"))', '(nest 2 (flush (bold (cat "a" cr "b"))))',
  '(nest 2 (cat "x" cr (flush (fg red (cat "a" cr "b")))))',
  '(chomp empty)', '(chomp (cat "a" blankline cr space))', '(chomp (prefixed "> " (cat "a" blankline)))',
  '(chomp (cat (prefixed "> " blankline) blankline))',
  '(cat (chomp (cat "a" cr)) "b")', '(chomp (cat "a" "\\u{A}"))',
  '(nestle empty)', '(nestle cr)', '(nestle (cat cr cr "a"))', '(nestle (cat cr blankline))',
  '(nestle (cat blankline cr "a" blankline))', '(nestle (nest 2 (cat blankline "a")))',
  '(nestle "\\u{A}\\u{A}a")',
  '(cat (before_non_blank "x") "")', '(cat (before_non_blank "x") space "a")',
  '(cat (before_non_blank "x") cr "a")', '(cat (before_non_blank "x") (bold "a"))',
  '(cat (before_non_blank "x") (bold " a"))', '(cat (before_non_blank "x") (lblock 3 " a"))',
  '(cat (before_non_blank "x") (lblock 3 "a"))', '(cat (before_non_blank "x") (nest 2 "a"))',
  '(cat (before_non_blank "x") (nest 2 " a"))', '(cat (before_non_blank "x") (before_non_blank "y") "z")',
  '(cat (before_non_blank "x") (after_break "y") "z")', '(before_non_blank "x")',
  '(cat (before_non_blank (cat "x" space "y")) "z")', '(cat "a" (before_non_blank "-") "\\u{3000}b")',
  '(cat (before_non_blank "x") (vfill " ") (lblock 2 "a"))',
  '(after_break "")', '(after_break "x")', '(cat "a" cr (after_break ".") "b")',
  '(cat "a" (after_break ".") "b")', '(cat (after_break "x\\u{A}y") "z")',
  '(cat "aaaa" space (after_break "\\\\&") ".b")',
  '(nest 2 (cat "a" cr (after_break "!") "b"))',
  '(nowrap (cat "a" space "b" space "c"))', '(nowrap (flush (cat "a" space "b")))',
  '(nowrap (before_non_blank (cat "a" space "b")))', '(nowrap (lblock 3 (cat "a" space "b")))',
  '(lblock 0 "abc")', '(lblock 0 empty)', '(lblock 0 "\\u{A}a")', '(cblock 0 "\\u{A}a")',
  '(rblock 0 "\\u{A}a")', '(rblock 0 "a\\u{A}bb")', '(cblock 0 "abc")', '(rblock 1 "\\u{A}\\u{A}a")', '(rblock 0 empty)', '(lblock 1 "日本")',
  '(lblock 3 "日本語")', '(rblock 5 "日本")', '(cblock 5 "ab")', '(cblock 6 "abc")',
  '(lblock 4 (cat "a" blankline "b"))', '(lblock 4 (cat "a" cr "" cr "b"))',
  '(lblock 3 (nest 1 "abcdef"))', '(lblock 5 (prefixed "> " (cat "aaa" space "bbb")))',
  '(cat (lblock 3 "a") (lblock 3 (vcat "b" "c" "d")) "e")',
  '(cat "xx" (lblock 3 (vcat "b" "c")) (rblock 3 (vcat "d" "e" "f")))',
  '(cat (lblock 3 "a") space (lblock 3 "b"))', '(cat (lblock 3 "a") cr (lblock 3 "b"))',
  '(prefixed "> " (cat "x" (lblock 3 (vcat "a" "b"))))',
  '(nest 2 (cat (lblock 3 (vcat "a" "b")) (lblock 3 "c")))',
  '(cat (lblock 2 (bold (vcat "a" "b"))) (fg red (vfill "|")))',
  '(cat (vfill "|") (vfill "+"))', '(vfill "")', '(cat (vfill "ab") (lblock 2 empty))',
  '(lblock 6 (cat (vfill "|") (lblock 2 (vcat "a" "b"))))',
  '(cat "a" (lblock 2 "  "))', '(cat (before_non_blank "x") (lblock 2 "  "))',
  '(cat (lblock 4 "a b c d e f") (lblock 4 "g h i j k"))',
  '(rblock 4 (vcat "a" "bb" "ccc" "dddd" "eeeee"))',
  '(cat "aa" space (lblock 6 "b") space "cc")',
  '(braces "a")', '(brackets "a")', '(parens "a")', '(quotes "a")', '(double_quotes "a")',
  '(inside "<" ">" (sp "a" "b"))', '(braces empty)',
  '(bold "a")', '(italic "a")', '(underlined "a")', '(strikeout "a")', '(bold empty)',
  '(fg black "a")', '(fg green "a")', '(fg yellow "a")', '(fg magenta "a")', '(fg cyan "a")',
  '(fg white "a")', '(bg red "a")', '(bg blue (fg yellow "a"))',
  '(bold (italic (cat "a" (underlined "b") "c")))', '(bold (cat "a" cr "b"))',
  '(prefixed "> " (bold (cat "a" cr "b")))', '(nest 2 (fg red (hsep "aa" "bb" "cc" "dd" "ee" "ff")))',
  '(cat (bold "a") (bold "b"))', '(bold (cat "a" space (bold "b")))',
  '(strikeout (cat "x" blankline "y"))', '(bold (cat cr "a"))', '(bold (cat "a" (after_break "x")))',
  '(hsep "日本語" "日本語" "日本語" "日本語" "日本語")', '(hsep "e\\u{301}e\\u{301}" "x" "\\u{1F600}\\u{1F600}" "y")',
  '(hsep "aaaaaaaaaaaa" "b" "cccccccccccccccccccccc" "d")', '(cat "abc" space space space "def")',
  '(cat "abcdefghi" space "x")', '(cat "abcdefghij" space "x")', '(cat "abcdefghijk" space "x")',
  '(cat "a" space (nest 2 "bbbbbbbbbbbbbbbbbbbbbb"))', '(cat "a" space (prefixed "> " "bbbbbbbbbbbbb"))',
  '(cat "a" space (bold "bbbbbbbbbbbb") "cc")', '(cat "a" space (lblock 12 "b") "c")',
  '(cat "a" space (vfill "123456789012345") "b")', '(cat "a" space (after_break "xxxxxxxxxxxxxxxx") "b")',
  '(nest 4 (hsep "one" "two" "three" "four" "five" "six" "seven"))',
  '(hang 3 "1. " (hsep "one" "two" "three" "four" "five" "six" "seven" "eight"))',
  '(prefixed "> " (hsep "one" "two" "three" "four" "five" "six" "seven" "eight" "nine"))',
  '(prefixed ">   " (cat "a" blankline "b"))', '(prefixed "\\u{3000}" (cat "a" blankline "b"))',
  '(nest 2 "\\u{A}a")', '(cat "a" (nest 2 cr) "b")', '(cat "a" (nest 2 blankline) "b")',
  '(vsep "a" "b" "c")', '(vsep "a" empty "c")', '(above_blank "a" (above "b" "c"))',
  '(vcat "a" (nest 2 (vcat "b" "c")) "d")', '(vcat "a\\u{A}" "b")', '(vcat (cat "a" cr) "b")',
  '(hsep "a" space "b")', '(hsep (cat "a" space) "b")', '(hsep cr "a")',
  '(cat "\\u{9}a" space "b")', '(cat "a\\u{0}b" space "c")', '(cat "\\u{301}" space "b")',
  '(cat "\\u{1F468}\\u{200D}\\u{1F469}\\u{200D}\\u{1F467}" space "x")',
}

local docs = {}
for _, src in ipairs(fixed) do docs[#docs + 1] = src end

-- Random expressions with a fixed seed.
math.randomseed(20261109)
local words = {
  '"a"', '"bb"', '"ccc"', '"hello"', '"world"', '"x y"', '" lead"', '"trail "',
  '"日本"', '"e\\u{301}"', '"\\u{1F600}"', '"a\\u{A}b"', '""', '"supercalifragilistic"',
  '"-"', '"."', '"\\u{A}"',
}
local plain_atoms = { 'space', 'cr', 'blankline', 'empty', '(blanklines 2)' }
local colors = { 'red', 'blue', 'green' }
local function pick(t) return t[math.random(#t)] end
local gen
gen = function(depth)
  local r = math.random(100)
  if depth <= 0 or r <= 25 then return pick(words) end
  if r <= 35 then return pick(plain_atoms) end
  local d = depth - 1
  local forms = {
    function()
      local n, parts = math.random(2, 5), { 'cat' }
      for i = 1, n do parts[#parts + 1] = gen(d) end
      return '(' .. table.concat(parts, ' ') .. ')'
    end,
    function() return '(hsep ' .. gen(d) .. ' ' .. gen(d) .. ' ' .. gen(d) .. ')' end,
    function() return '(vcat ' .. gen(d) .. ' ' .. gen(d) .. ')' end,
    function() return '(vsep ' .. gen(d) .. ' ' .. gen(d) .. ')' end,
    function() return '(sp ' .. gen(d) .. ' ' .. gen(d) .. ')' end,
    function() return '(nest ' .. math.random(0, 4) .. ' ' .. gen(d) .. ')' end,
    function() return '(hang ' .. math.random(1, 4) .. ' "- " ' .. gen(d) .. ')' end,
    function() return '(prefixed "> " ' .. gen(d) .. ')' end,
    function() return '(flush ' .. gen(d) .. ')' end,
    function() return '(nowrap ' .. gen(d) .. ')' end,
    function() return '(chomp ' .. gen(d) .. ')' end,
    function() return '(nestle ' .. gen(d) .. ')' end,
    function() return '(before_non_blank ' .. gen(d) .. ')' end,
    function() return '(after_break "' .. pick({ '.', '\\\\&', '' }) .. '")' end,
    function() return '(' .. pick({ 'lblock', 'cblock', 'rblock' }) .. ' ' .. math.random(0, 9) .. ' ' .. gen(d) .. ')' end,
    function() return '(cat (vfill "|") (lblock 4 ' .. gen(d) .. ') (vfill "|"))' end,
    function() return '(' .. pick({ 'bold', 'italic', 'underlined', 'strikeout' }) .. ' ' .. gen(d) .. ')' end,
    function() return '(fg ' .. pick(colors) .. ' ' .. gen(d) .. ')' end,
    function() return '(braces ' .. gen(d) .. ')' end,
  }
  return pick(forms)()
end
for _ = 1, 160 do docs[#docs + 1] = gen(4) end

local widths = { nil, 10, 20, 40, 72 }
for _, src in ipairs(docs) do
  local expr = parse(tokenize(src), 1)
  local doc = eval(expr)
  print('doc ' .. serialize(expr))
  print(string.format('meta %s %d %d %d %d %d', tostring(L.is_empty(doc)),
    L.offset(doc), L.min_offset(doc), L.height(doc),
    L.update_column(doc, 0), L.update_column(doc, 5)))
  for i = 1, 5 do
    local w = widths[i]
    local label = w and tostring(w) or '-'
    print('plain ' .. label .. ' ' .. quote(L.render(doc, w)))
    print('ansi ' .. label .. ' ' .. quote(L.render(doc, w, 'ansi')))
  end
end
