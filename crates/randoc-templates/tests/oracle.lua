-- Makes tests/oracle.json with the oracle:
--   pandoc lua tests/oracle.lua > tests/oracle.json
-- Each case has a template, a context in JSON, a line width, partial files
-- and the oracle result: the rendered text or the error text.
-- In a context, {"@words": "a b"} is a Doc of words with breakable spaces.
-- An empty JSON array is an empty Lua table, which the oracle reads as a map.

local C = [==[{
  "x": "hello", "y": "world", "e": "", "t": true, "f": false,
  "list": ["a", "b", "c"], "one": ["solo"],
  "map": {"k": "v", "n": {"deep": "d"}},
  "people": [{"name": "Ann", "age": "30"}, {"name": "Bob"}],
  "num": "7", "big": "1994", "huge": "4000", "zero": "0",
  "ml": "line1\nline2", "nl": "ends\n", "nl2": "two\n\n",
  "words": {"@words": "alpha beta gamma delta epsilon zeta eta theta iota kappa"},
  "nested": [["a", "b"], ["c"]], "sp": "  padded  ",
  "upper": "MiXeD Case ß", "café": "cafe value",
  "cases": "\u03a3\u0391\u03a3 \u00df \ufb03 \u0130 \u01c6 \u01c4 \u038c\u03a3\u039f\u03a3",
  "outer": "O", "mm": {"a": {"b": ["p", "q"], "k": "K"}},
  "grid": [["1", "2"], ["3"]],
  "pp": {"q": [{"name": "ann"}, {"name": "bob"}], "r": {"z": "zed"}},
  "x-y": "dash", "x_1": "under", "digits": ["1", "5", "20", "27"]
}]==]

local P = {
  ["item.txt"] = "($it$)",
  ["wrap.txt"] = "A\nB",
  ["item.tex"] = "TEX $x$",
  ["final.txt"] = "line\n",
  ["bad.txt"] = "ok\n$if(x$",
  ["loop.txt"] = "L$loop()$",
  ["person.txt"] = "$it.name$<$it.age$>",
  ["block.txt"] = "first\nsecond",
  ["empty.txt"] = "",
  ["toggle.txt"] = "$~$a b$~$",
  ["sub/inner.txt"] = "inner $x$",
  ["cmt.txt"] = "$-- c\nafter comment",
  ["a2.txt"] = "x\ny",
  ["p.txt"] = "($it$)",
  ["zz.txt"] = "x\ny",
}

-- { id, template [, width] [, context] }
local cases = {
  {"var", "$x$"}, {"var-braces", "${x}"}, {"var-spaces", "$ x $"},
  {"var-braces-spaces", "${ x }"}, {"var-tab", "$\tx\t$"},
  {"field", "$map.k$"}, {"field-deep", "${map.n.deep}"},
  {"missing", "[$missing$]"}, {"bools", "$t$ $f$"}, {"map", "$map$"},
  {"list", "$list$"}, {"list-maps", "$people$"}, {"list-field", "[$people.name$]"},
  {"empty", "[$e$]"}, {"escape-around", "$$x$$"}, {"escape-mid", "a$$b"},
  {"escape-triple", "$$$x$$$"}, {"adjacent-braces", "${x}${y}"},
  {"adjacent", "$x$$y$"}, {"field-of-text", "[$x.y$]"}, {"nested-list", "$nested$"},
  {"multiline", "$ml$"}, {"multiline-indent", "  $ml$"},
  {"multiline-inline", "pre $ml$ post"}, {"multiline-after", "  $ml$ after"},
  {"final-newline-value", "[$nl$]"}, {"final-newlines-value", "[$nl2$]"},
  {"multiline-second-line", "x\n  $ml$\n"}, {"multiline-tab", "\t$ml$"},
  {"it-top", "[$it$]"}, {"dash-name", "$x-y$"}, {"underscore-name", "$x_1$"},
  {"unicode-name", "$caf\u{e9}$"}, {"var-final-newline", "$x$\n"},
  {"blank-lines", "\n\n$x$\n\n"}, {"spaces-line", "   \n$x$"},
  {"padded-value", "[$sp$]"}, {"text-only", "plain text\nsecond line\n"},
  {"err-digit-start", "$1x$"}, {"err-open-brace", "${x"}, {"err-open", "$x"},
  {"err-if-alone", "$if$"}, {"err-empty-braces", "${}"}, {"err-close-brace", "$}"},
  {"err-space", "$ $"}, {"err-pipe-empty", "$x/$"}, {"err-pipe-unknown", "$x/foo$"},
  {"err-pipe-left", "$x/left$"}, {"err-pipe-left-a", "$x/left a$"},
  {"err-dollar", "$"}, {"err-brace-alone", "${"}, {"err-mid", "a $ b"},
  {"err-tabs-line3", "a\n\tb\n\t$if(x$"}, {"err-braces-endif", "${if(x)}a${endif"},
  {"err-unicode", "\u{e9}\u{e9}$if(x$"}, {"err-reserved-sep", "$sep$"},
  {"err-reserved-it-field", "$if(it.)$"}, {"err-dot-end", "$x.$"},
  {"err-paren", "$x()$"},
  {"comment-line", "$-- comment\nnext"}, {"comment-inline", "a $-- comment\nnext"},
  {"comment-two", "$--\n$--\nx"}, {"comment-end", "a$--x"}, {"err-dash", "$-x$"},
  {"comment-only", "$-- only"}, {"comment-indented", "  $-- indented\nz"},
  {"comment-after-var", "$x$$-- c\ny"},
  {"if", "$if(x)$yes$endif$"}, {"if-missing", "$if(missing)$yes$else$no$endif$"},
  {"if-empty", "$if(e)$yes$else$no$endif$"}, {"if-false", "$if(f)$yes$else$no$endif$"},
  {"if-true", "$if(t)$yes$endif$"}, {"if-list", "$if(list)$L$endif$"},
  {"if-map", "$if(map)$M$endif$"}, {"if-field", "$if(map.k)$K$endif$"},
  {"if-field-missing", "$if(map.zz)$Z$else$noZ$endif$"},
  {"if-multiline", "$if(x)$\nyes\n$endif$\nafter"},
  {"if-else-multiline", "$if(x)$\nyes\n$else$\nno\n$endif$\nafter"},
  {"if-else-multiline-false", "$if(missing)$\nyes\n$else$\nno\n$endif$\nafter"},
  {"elseif", "$if(missing)$a$elseif(x)$b$else$c$endif$"},
  {"elseif-else", "$if(missing)$a$elseif(missing)$b$else$c$endif$"},
  {"elseif-chain", "$if(missing)$a$elseif(e)$b$elseif(t)$c$endif$"},
  {"elseif-multiline", "$if(missing)$\na\n$elseif(x)$\nb\n$else$\nc\n$endif$\nend"},
  {"if-braces", "${if(x)}yes${else}no${endif}"}, {"if-spaces", "$ if(x) $yes$ endif $"},
  {"err-if-inner-space", "$if( x )$yes$endif$"}, {"err-if-open", "$if(x)$yes"},
  {"err-if-else-open", "$if(x)$yes$else$no"}, {"err-endif", "$endif$"},
  {"err-else", "$else$"}, {"if-nested", "$if(x)$$if(y)$both$endif$$endif$"},
  {"if-in-else", "$if(missing)$a$else$$if(y)$b$endif$$endif$"},
  {"if-indented", "  $if(x)$\n  indented\n  $endif$\nend"},
  {"if-trailing-spaces", "$if(x)$  \nyes\n$endif$"},
  {"if-blank-after", "$if(x)$yes$endif$\n\nafter"}, {"if-pipe", "$if(x/length)$len$endif$"},
  {"if-it", "$if(it)$i$else$no$endif$"}, {"if-list-maps", "$if(people)$P$endif$"},
  {"if-space-body", "[$if(x)$ $endif$]"}, {"err-elseif-bare", "$if(x)$a$elseif$b$endif$"},
  {"if-crlf", "$if(x)$\r\nyes\r\n$endif$\r\nend"},
  {"if-text-after-open", "$if(x)$ text\nmore\n$endif$\n"},
  {"for", "$for(list)$$list$$endfor$"}, {"for-sep", "$for(list)$$it$$sep$, $endfor$"},
  {"for-multiline", "$for(list)$\n- $list$\n$endfor$\nend"},
  {"for-sep-multiline", "$for(list)$\n- $it$\n$sep$\n--\n$endfor$\nend"},
  {"for-fields", "$for(people)$$people.name$ ($people.age$)$sep$; $endfor$"},
  {"for-it-field", "$for(people)$$it.name$$endfor$"}, {"for-scalar", "$for(x)$[$x$]$endfor$"},
  {"for-missing", "[$for(missing)$m$endfor$]"}, {"for-map", "$for(map)$$map.k$$endfor$"},
  {"for-true", "$for(t)$T$endfor$"}, {"for-false", "$for(f)$F$endfor$"},
  {"for-empty", "$for(e)$E$endfor$"}, {"for-nested-list", "$for(nested)$($it$)$sep$,$endfor$"},
  {"for-for-it", "$for(nested)$$for(it)$<$it$>$endfor$$sep$|$endfor$"},
  {"for-same-var", "$for(list)$$for(list)$$list$$endfor$;$endfor$"},
  {"bracket-sep", "$list[, ]$"}, {"bracket-empty", "$list[]$"},
  {"bracket-field", "[$people.name[, ]$]"}, {"bracket-newline", "$list[\n]$"},
  {"bracket-scalar", "$x[,]$"}, {"for-empty-sep", "$for(list)$a$sep$$endfor$"},
  {"for-braces", "${for(list)}${it}${sep}-${endfor}"}, {"err-for-open", "$for(list)$x"},
  {"err-for-endif", "$for(list)$x$endif$"}, {"err-endfor", "$endfor$"},
  {"err-for-sep-open", "$for(list)$$it$$sep$"},
  {"for-reverse", "$for(list/reverse)$$it$$endfor$"},
  {"for-it-upper", "$for(list)$$it/uppercase$$endfor$"},
  {"for-indented", "  $for(list)$\n  * $it$\n  $endfor$\n"},
  {"for-outer-var", "$for(list)$$list$ and $x$\n$endfor$"},
  {"for-if", "$for(people)$$if(people.age)$$people.name$ is $people.age$$else$$people.name$ unknown$endif$\n$endfor$"},
  {"for-maps-it", "$for(people)$$it$$endfor$"}, {"for-item-length", "$for(list)$$list/length$$endfor$"},
  {"for-multiline-values", "$for(list)$\n  $ml$\n$endfor$"},
  {"for-nested-column", "ab $for(list)$$ml$$sep$, $endfor$"},
  {"for-sep-column", "$for(list)$$it$$sep$\n  $endfor$"},
  {"nest-caret", "$^$a\nb"}, {"nest-caret-value", "item: $^$$ml$"},
  {"nest-caret-indented", "  $^$x\ny\nz"},
  {"nest-caret-blank", "- $^$first\nsecond\n\nthird\nfourth"},
  {"nest-caret-end", "> $^$a\nb\n$endif$"},
  {"nest-two", "  $ml$\n  $ml$"}, {"nest-braces", "  ${ml}"}, {"nest-text-before", "abc $ml$"},
  {"nest-tab-two", "\t$x$\n\t$ml$"}, {"nest-for", "   $for(list)$$ml$$endfor$"},
  {"nest-prefix", "> $ml$"}, {"nest-for-caret", "* $^$$for(list)$$it$\n$endfor$"},
  {"breaking", "$~$a b c d e f g h i j k l m n o p$~$", 10},
  {"breaking-long", "$~$this is a long line that should wrap at the given width$~$ not here at all ok", 20},
  {"breaking-newline", "$~$a\nb c$~$", 3}, {"breaking-value", "$~$$words$$~$", 20},
  {"words", "$words$", 20}, {"words-nowrap", "$words/nowrap$", 20},
  {"breaking-lead", "$~$  lead and trail  $~$|", 10},
  {"breaking-vars", "$~$${x} ${y} ${x} ${y}$~$", 12},
  {"breaking-open", "$~$a b c d e f g h", 5}, {"breaking-braces", "${~}a b c${~}", 2},
  {"words-indent", "  $words$", 20}, {"words-block", "[$words/left 12$]", 40},
  {"pipe-upper", "$x/uppercase$"}, {"pipe-lower", "$upper/lowercase$"},
  {"pipe-upper-sharp-s", "$upper/uppercase$"},
  {"pipe-length", "$list/length$ $x/length$ $map/length$ $missing/length$ $t/length$ $upper/length$"},
  {"pipe-reverse", "$x/reverse$ $list/reverse$ $ml/reverse$"},
  {"pipe-first-last", "$list/first$ $list/last$ $list/rest$ $list/allbutlast$"},
  {"pipe-rest-one", "[$one/rest$|$one/allbutlast$]"}, {"pipe-first-text", "$x/first$ $x/last$"},
  {"pipe-numbers", "$num/alpha$ $num/roman$ $big/roman$ $huge/roman$ $zero/roman$ $zero/alpha$"},
  {"pipe-numbers-text", "$x/alpha$ $x/roman$"}, {"pipe-digits", "$digits/alpha[ ]$ $digits/roman[ ]$"},
  {"pipe-pairs-map", "$for(map/pairs)$$it.key$=$it.value$;$endfor$"},
  {"pipe-pairs-list", "$for(list/pairs)$$it.key$:$it.value$ $endfor$"},
  {"pipe-chomp", "[$nl/chomp$|$nl2/chomp$]"},
  {"pipe-left", "[$x/left 10$]"}, {"pipe-right", "[$x/right 10$]"},
  {"pipe-center", "[$x/center 11$]"}, {"pipe-left-borders", "$x/left 10 \"|\" \"|\"$"},
  {"pipe-left-ml", "$ml/left 8 \"| \" \" |\"$"}, {"pipe-right-narrow", "$x/right 3$"},
  {"pipe-block-missing", "$missing/left 5 \"[\" \"]\"$"},
  {"pipe-chain-list", "$list/uppercase/reverse[,]$"}, {"pipe-pairs-length", "$map/pairs/length$"},
  {"pipe-roman-upper", "$num/roman/uppercase$"}, {"pipe-nowrap", "$x/nowrap$"},
  {"pipe-chomp-upper", "$x/chomp/uppercase$"}, {"pipe-left-two-spaces", "[$x/left  5$]"},
  {"pipe-left-border-no-space", "$x/left 5\"<\"$"}, {"pipe-pairs-first", "$people/pairs/first$"},
  {"pipe-pairs-people", "$for(people/pairs)$$it.key$ $it.value.name$ $endfor$"},
  {"pipe-border-escape", "$x/center 10 \"\\\"\" \"x\"$"},
  {"pipe-left-one-border", "$x/left 7 \"<<\"$"}, {"pipe-bool-upper", "$t/uppercase$"},
  {"pipe-pairs-text", "$x/pairs$"}, {"pipe-left-list", "$list/left 3[,]$"},
  {"pipe-cblock-ml", "$ml/center 9 \"(\" \")\"$"},
  {"partial-bare", "$item()$"}, {"partial-apply", "$list:item()$"},
  {"partial-apply-sep", "$list:item()[, ]$"}, {"partial-for-it", "$for(list)$$it:item()$$endfor$"},
  {"partial-scalar", "$x:item()$"}, {"partial-pipe", "$item()/uppercase$"},
  {"partial-sep-pipe", "$list:item()[, ]/uppercase$"}, {"partial-indent", "  $wrap()$"},
  {"partial-indent-text", "x $wrap()$ y"}, {"partial-ext", "$item.tex()$"},
  {"partial-final-newline", "[$final()$]"}, {"partial-error", "$bad()$"},
  {"partial-loop", "$loop()$"}, {"partial-braces", "${item()}"}, {"partial-spaces", "$ item() $"},
  {"partial-people", "$people:person()[; ]$"},
  {"partial-line", "$wrap()$\nafter"}, {"partial-line-indented", "  $wrap()$\nafter"},
  {"partial-empty", "[$empty()$]"}, {"partial-toggle", "$toggle()$ x y", 2},
  {"partial-subdir", "$sub/inner()$"}, {"partial-comment", "$cmt()$"},
  {"partial-block-pipe", "$block()/left 8 \"> \"$"},
  {"crlf", "a\r\nb\r\n$x$"}, {"cr", "a\rb$x$"}, {"price", "$x$ costs $$5"},
  {"dollars", "$$"}, {"braces-mix", "${x}y${x}"}, {"tab-var", "\t$x$"},
  {"deep-mix", "$if(x)$$for(list)$$if(t)$[$it$]$endif$$endfor$$endif$"},
  {"it-in-loop-field", "$for(people)$$it.name$=$it.age$ $endfor$"},
  {"for-it-name", "$for(it)$x$endfor$|"},
  {"if-ml-indent", "  $if(x)$$ml$$endif$"},
  {"pipe-lower-per-char", "$cases/lowercase$"}, {"pipe-upper-per-char", "$cases/uppercase$"},
  {"newline-length", "[$nl/length$]"}, {"newline-reverse", "[$nl/reverse$]"},
  {"newline-upper", "[$nl/uppercase$]"}, {"newline-left", "[$nl/left 6 \"|\" \"|\"$]"},
  {"pipe-left-huge", "[$x/left 100000$]"}, {"pipe-left-giant", "[$x/left 999999999999$]"},
  {"pipe-left-wrap", "[$x/left 99999999999999999999$]"}, {"pipe-left-zero", "[$x/left 0$]"},
  {"pipe-right-huge", "[$x/right 100000 \"|\"$]"},
}

-- Scope cases: loop variables and fields inside loops.
local scope_cases = {
  {"scope-field-path", "$for(map.n)$[$map.n.deep$|$map.k$|$it.deep$]$endfor$"},
  {"scope-prefix", "$for(map.n)$$map/length$:$map.n$$endfor$"},
  {"scope-nested-same", "$for(people)$$for(people)$<$people.name$>$endfor$$endfor$"},
  {"scope-it-field", "$for(people)$$for(it.name)$[$it$]$endfor$$endfor$"},
  {"scope-missing-pipe", "$for(nothere/length)$[$nothere$|$it$]$endfor$"},
  {"scope-sep-outer", "$for(list)$$it$$sep$($it$)$endfor$"},
}
for _, c in ipairs(scope_cases) do cases[#cases + 1] = c end

-- Error probes: each base template cut at each position, and each base
-- template with one character replaced by "!" or ".".
local bases = {
  "$if(x)$a$elseif(y)$b$else$c$endif$",
  "$for(list)$a$sep$b$endfor$",
  "${x/left 3 \"<\" \">\"}",
  "$x/uppercase/right 4$",
  "$list:item()[, ]/uppercase$",
  "$item()/left 3$",
  "$it.name/length$",
  "$^$a",
  "$~$a b$~$",
  "$s/center 5 \"(\"$",
}
local seen = {}
local function add_probe(src)
  if seen[src] then return end
  seen[src] = true
  cases[#cases + 1] = {"probe-" .. (#cases + 1), src}
end
for _, base in ipairs(bases) do
  for k = 1, #base - 1 do add_probe(base:sub(1, k)) end
  for k = 2, #base - 1 do
    add_probe(base:sub(1, k - 1) .. "!" .. base:sub(k + 1))
    add_probe(base:sub(1, k - 1) .. "." .. base:sub(k + 1))
  end
end

-- Partials whose names sort before and after "main".
local partial_probes = {
  "$a2()[a]x$", "$zz()[a]x$", "$p()[a]x$", "$l:p()[a][b]$", "$s:p()x$",
  "$a2()x$", "$zz()x$", "$item()[a]x$", "$a2()$$if(x$", "$zz()$$if(x$",
  "$list:a2()[,]x$", "$list:zz()[,]x$",
  "$if(x)$a$else$b$for(y)$c", "$if(x)$a$elseif(y)$b$for(z)$c",
  "$if(x)$a$else$b$if(y)$c$endif", "$if(x)$a$elseif(y)$b$elseif(z)$c$for(w)$d",
  "$for(x)$a$sep$b$if(y)$c", "$if(x)$a$else$$x/left 3.5$$endif$",
}
for _, src in ipairs(partial_probes) do add_probe(src) end

-- Loop probes: each loop variable with each body and each separator.
local loop_vars = {"list", "people", "map", "nested", "one", "x", "missing",
  "mm.a.b", "mm.a", "grid", "people/reverse", "map/pairs"}
local loop_bodies = {"$it$", "[VAR]", "[VAR.name]", "$it.name$", "$x$",
  "$outer$", "$for(it)$<$it$>$endfor$", "$it:item()$",
  "$if(it.age)$A$else$-$endif$", "[VAR/length]", "$mm.a.k$[PREFIX]"}
local loop_seps = {"", ", ", "|$x$|"}
local function var_name(v) return (v:gsub("/.*", "")) end
for _, v in ipairs(loop_vars) do
  local name = var_name(v)
  local prefix = name:match("^(.-)%.[^.]*$") or name
  for _, b in ipairs(loop_bodies) do
    for _, sep in ipairs(loop_seps) do
      local body = b:gsub("%[VAR%]", "$" .. name .. "$"):gsub("%[VAR%.name%]", "$" .. name .. ".name$")
        :gsub("%[VAR/length%]", "$" .. name .. "/length$"):gsub("%[PREFIX%]", "$" .. prefix .. "/length$")
      local src
      if sep == "" then
        src = "$for(" .. v .. ")$" .. body .. "$endfor$"
      else
        src = "$for(" .. v .. ")$" .. body .. "$sep$" .. sep .. "$endfor$"
      end
      add_probe(src)
    end
  end
end
local nested_loops = {
  "$for(list)$$for(people)$$list$:$people.name$:$outer$ $endfor$$endfor$",
  "$for(people)$$for(list)$$people.name$$list$$it$$endfor$;$endfor$",
  "$for(x)$[$x$]$for(x)$($x$)$endfor$$endfor$",
  "$for(map)$$for(map.n)$$map.k$$map.n.deep$$it.deep$$endfor$$endfor$",
  "$for(grid)$$for(it)$$it$$grid$$endfor$/$endfor$",
  "$for(mm.a.b)$$for(mm.a)$$mm.a.b$$mm.a.k$$endfor$$endfor$",
  "$for(mm.a.b)$$mm$$mm.a$$mm.a/length$$mm.a.b$$endfor$",
  "$for(people)$$people:person()$$sep$$x$$endfor$",
  "$for(list)$$list:item()[,]$$endfor$",
  "$for(one)$$one$$one/length$$it/length$$endfor$",
  "$for(mm.a.b)$$for(mm.a/pairs)$$mm.a.key$=$mm.a.value$;$endfor$$endfor$",
  "$for(pp.q)$$for(pp.q.name/uppercase)$$pp.q.name$$endfor$$endfor$",
  "$for(pp.q)$$for(pp.r/pairs)$$pp.r.key$$pp.r.value$$pp.q.name$$endfor$$endfor$",
  "$for(pp.q)$$for(pp.r.z/uppercase)$$pp.r.z$$endfor$$endfor$",
  "$if(zero/roman)$y$else$n$endif$", "$if(zero/alpha)$y$else$n$endif$",
  "$if(x/uppercase)$y$endif$", "$if(e/uppercase)$y$else$n$endif$",
  "$if(nl/chomp)$y$endif$", "$if(missing/left 3)$y$else$n$endif$",
  "$if(e/left 3)$y$else$n$endif$", "$if(list/uppercase)$y$endif$",
  "$if(nested/rest/rest)$y$else$n$endif$", "$if(map/pairs/rest)$y$endif$",
  "$x/uppercase/length$ $upper/lowercase/length$ $upper/uppercase/length$",
  "$for(map/pairs/reverse)$$it.key$$sep$,$endfor$", "$for(list/reverse/rest)$$it$$endfor$",
  "$map/pairs/rest/first$|$list/allbutlast/reverse/first$|$people/reverse/last$",
  "$for(people/uppercase)$$it.name$$endfor$", "$for(map/lowercase/pairs)$$it.key$=$it.value$;$endfor$",
}
for _, src in ipairs(nested_loops) do add_probe(src) end

local function decode_ctx(v)
  if type(v) == "table" then
    if v["@words"] then
      local parts = {}
      for w in v["@words"]:gmatch("%S+") do
        if #parts > 0 then parts[#parts + 1] = pandoc.layout.space end
        parts[#parts + 1] = pandoc.layout.literal(w)
      end
      return pandoc.layout.concat(parts)
    end
    local out = {}
    for k, x in pairs(v) do out[k] = decode_ctx(x) end
    return out
  end
  return v
end

local tmp = os.tmpname()
os.remove(tmp)
pandoc.system.make_directory(tmp, true)
pandoc.system.make_directory(tmp .. "/sub", true)
for name, text in pairs(P) do
  local f = assert(io.open(tmp .. "/" .. name, "w"))
  f:write(text)
  f:close()
end

local out = {}
local ctx = decode_ctx(pandoc.json.decode(C, false))
pandoc.system.with_working_directory(tmp, function()
  for _, c in ipairs(cases) do
    local id, src, width = c[1], c[2], c[3]
    local ok, res = pcall(pandoc.template.compile, src, "main.txt")
    local entry = {id = id, template = src, width = width}
    if ok then
      entry.output = pandoc.layout.render(pandoc.template.apply(res, ctx), width)
    else
      entry.error = tostring(res)
    end
    out[#out + 1] = entry
  end
end)

-- Default templates: three contexts each.
local CONTEXTS = {
  {},
  {title = "A Title", author = {"Ann Author", "Bob Writer"}, date = "2024-01-02"},
  {
    title = "A Title", subtitle = "Sub", author = {"Ann Author", "Bob Writer"},
    date = "2024-01-02", abstract = "The abstract.", keywords = {"k1", "k2"},
    lang = "en-US", dir = "ltr", toc = true, ["toc-title"] = "Contents",
    ["table-of-contents"] = "<ul><li>One</li></ul>", body = "Body text.\n\nSecond paragraph.",
    ["header-includes"] = {"<!-- h1 -->", "<!-- h2 -->"},
    ["include-before"] = "Before.", ["include-after"] = "After.",
    css = {"a.css", "b.css"}, pagetitle = "Page", institute = "Inst",
    numbersections = true, fontsize = "11pt", geometry = {"margin=1in"},
    documentclass = "article", classoption = {"twocolumn"}, mainfont = "Serif",
    linkcolor = "blue", papersize = "a4", links_as_notes = true,
    ["author-meta"] = "Ann Author; Bob Writer", ["date-meta"] = "2024-01-02",
    ["title-meta"] = "A Title", subject = "Subj", description = "Desc",
    section = "1", header = "Head", footer = "Foot", adjusting = true,
    hyphenate = true, version = "1.0", copyright = {holder = "Me", year = "2024"},
    math = "<script></script>", ["highlighting-macros"] = "% macros",
    tables = true, graphics = true, ["csl-css"] = true, ["document-css"] = true,
    ["quotes"] = true, ["verbatim-in-note"] = true, ["beamer"] = false,
    theme = "default", revealjs_url = "reveal", ["slidy-url"] = "slidy",
    ["s5-url"] = "s5", ["dzslides-core"] = "core", babel = "english",
    indent = true, ["cover-image"] = "cover.png", ["epub3"] = true,
  },
}

local formats = {}
local f = assert(io.open("spec/inventory/output-formats.json"))
local list = pandoc.json.decode(f:read("a"), false)
f:close()
local templates = {}
for _, fmt in ipairs(list) do
  local ok, src = pcall(pandoc.template.default, fmt)
  if ok and src ~= "" then
    formats[#formats + 1] = fmt
    local tpl = pandoc.template.compile(src)
    local results = {}
    for i, cx in ipairs(CONTEXTS) do
      results[i] = pandoc.layout.render(pandoc.template.apply(tpl, cx), 72)
    end
    templates[#templates + 1] = {format = fmt, outputs = results}
  end
end

io.write(pandoc.json.encode({
  context = C, partials = P, cases = out,
  default_contexts = CONTEXTS, defaults = templates,
}))
io.write("\n")
